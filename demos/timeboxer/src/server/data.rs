//! `data/schema.ts` and `data/schedules.ts`: users, password records, schedules and their
//! blocks. Remix keeps these in SQLite through `remix/data-table`; this is an in-memory store
//! that enforces the same constraints (unique usernames, unique `(user_id, name)` schedules,
//! unique block client ids, soft deletes) and the same transaction boundaries.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::layout::Block;

/// A `users` row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub created_at: i64,
}

/// A `user_passwords` row.
#[derive(Clone, Debug)]
pub struct UserPassword {
    pub id: i64,
    pub user_id: i64,
    pub password_hash: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A `schedules` row, serialized as the list endpoint returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Schedule {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub revision: i64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A `schedule_blocks` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockRow {
    pub id: i64,
    pub client_id: String,
    pub schedule_id: i64,
    pub name: String,
    pub color: Option<String>,
    pub day_of_week: i64,
    pub start_minute: i64,
    pub end_minute: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

pub use crate::layout::ScheduleDocument;

/// `ScheduleReplaceInput`
#[derive(Clone, Debug)]
pub struct ReplaceInput {
    pub base_revision: i64,
    pub blocks: Vec<Block>,
    pub name: String,
}

/// `ScheduleDataError`: a message and the HTTP status it maps to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleDataError {
    pub message: String,
    pub status: u16,
    /// A unique constraint failed (Remix's `DataTableConstraintError`).
    pub constraint: bool,
}

impl ScheduleDataError {
    fn new(message: &str, status: u16) -> ScheduleDataError {
        ScheduleDataError { message: message.into(), status, constraint: false }
    }

    fn constraint(message: &str) -> ScheduleDataError {
        ScheduleDataError { message: message.into(), status: 409, constraint: true }
    }
}

impl std::fmt::Display for ScheduleDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

const ACTIVE: &str = "active";
const DELETED: &str = "deleted";

#[derive(Default)]
struct Tables {
    users: Vec<User>,
    passwords: Vec<UserPassword>,
    schedules: Vec<Schedule>,
    blocks: Vec<BlockRow>,
    next_id: HashMap<&'static str, i64>,
}

impl Tables {
    fn next_id(&mut self, table: &'static str) -> i64 {
        let id = self.next_id.entry(table).or_insert(0);
        *id += 1;
        *id
    }

    fn require_schedule(&self, user_id: i64, schedule_id: i64) -> Result<&Schedule, ScheduleDataError> {
        self.schedules
            .iter()
            .find(|s| s.id == schedule_id && s.status == ACTIVE && s.user_id == user_id)
            .ok_or_else(|| ScheduleDataError::new("Schedule not found.", 404))
    }

    fn block_rows(&self, schedule_id: i64) -> Vec<BlockRow> {
        let mut rows: Vec<BlockRow> = self.blocks.iter().filter(|b| b.schedule_id == schedule_id).cloned().collect();
        rows.sort_by_key(|b| (b.day_of_week, b.start_minute));
        rows
    }

    fn document(&self, schedule: &Schedule) -> ScheduleDocument {
        let blocks = self
            .block_rows(schedule.id)
            .into_iter()
            .map(|b| Block {
                color: b.color,
                day_of_week: b.day_of_week,
                end_minute: b.end_minute,
                id: b.client_id,
                name: b.name,
                start_minute: b.start_minute,
            })
            .collect();
        ScheduleDocument { blocks, id: schedule.id, name: schedule.name.clone(), revision: schedule.revision, updated_at: schedule.updated_at }
    }
}

/// The database: cheap to clone, shared by every request.
#[derive(Clone, Default)]
pub struct Db {
    tables: Arc<Mutex<Tables>>,
    mocked_now: Arc<Mutex<Option<i64>>>,
}

impl Db {
    pub fn new() -> Db {
        Db::default()
    }

    fn lock(&self) -> MutexGuard<'_, Tables> {
        self.tables.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `Date.now()`, unless a test fixed it with [`Db::mock_now`].
    pub fn now(&self) -> i64 {
        if let Some(now) = *self.mocked_now.lock().unwrap_or_else(|e| e.into_inner()) {
            return now;
        }
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
    }

    /// Fix `now()` (`withMockedNow` in the Remix tests); `None` restores the clock.
    pub fn mock_now(&self, now: Option<i64>) {
        *self.mocked_now.lock().unwrap_or_else(|e| e.into_inner()) = now;
    }

    // --- users ---------------------------------------------------------------------------

    pub fn find_user(&self, id: i64) -> Option<User> {
        self.lock().users.iter().find(|u| u.id == id).cloned()
    }

    pub fn find_user_by_username(&self, username: &str) -> Option<User> {
        self.lock().users.iter().find(|u| u.username == username).cloned()
    }

    pub fn password_for(&self, user_id: i64) -> Option<UserPassword> {
        self.lock().passwords.iter().find(|p| p.user_id == user_id).cloned()
    }

    /// Create a user and its password record in one transaction. `None` when the username
    /// is taken.
    pub fn create_user(&self, username: &str, password_hash: &str) -> Option<User> {
        let now = self.now();
        let mut t = self.lock();
        if t.users.iter().any(|u| u.username == username) {
            return None;
        }
        let user = User { id: t.next_id("users"), username: username.into(), created_at: now };
        let id = t.next_id("user_passwords");
        t.passwords.push(UserPassword { id, user_id: user.id, password_hash: password_hash.into(), created_at: now, updated_at: now });
        t.users.push(user.clone());
        Some(user)
    }

    /// Delete a user, cascading to passwords, schedules and blocks.
    pub fn delete_user(&self, id: i64) {
        let mut t = self.lock();
        let schedule_ids: Vec<i64> = t.schedules.iter().filter(|s| s.user_id == id).map(|s| s.id).collect();
        t.blocks.retain(|b| !schedule_ids.contains(&b.schedule_id));
        t.schedules.retain(|s| s.user_id != id);
        t.passwords.retain(|p| p.user_id != id);
        t.users.retain(|u| u.id != id);
    }

    // --- schedules -----------------------------------------------------------------------

    /// `listSchedules`: active schedules, newest first.
    pub fn list_schedules(&self, user_id: i64) -> Vec<Schedule> {
        let mut list: Vec<Schedule> = self.lock().schedules.iter().filter(|s| s.user_id == user_id && s.status == ACTIVE).cloned().collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        list
    }

    /// `createSchedule`. Deleted schedules keep their names reserved.
    pub fn create_schedule(&self, user_id: i64, name: &str) -> Result<ScheduleDocument, ScheduleDataError> {
        let now = self.now();
        let mut t = self.lock();
        if t.schedules.iter().any(|s| s.user_id == user_id && s.name == name) {
            return Err(ScheduleDataError::new("Name must be unique.", 409));
        }
        let schedule = Schedule {
            id: t.next_id("schedules"),
            user_id,
            name: name.into(),
            revision: 0,
            status: ACTIVE.into(),
            created_at: now,
            updated_at: now,
        };
        t.schedules.push(schedule.clone());
        Ok(t.document(&schedule))
    }

    /// `getScheduleDocument`
    pub fn schedule_document(&self, user_id: i64, schedule_id: i64) -> Result<ScheduleDocument, ScheduleDataError> {
        let t = self.lock();
        let schedule = t.require_schedule(user_id, schedule_id)?;
        Ok(t.document(schedule))
    }

    /// `deleteSchedule`: a soft delete that bumps the revision.
    pub fn delete_schedule(&self, user_id: i64, schedule_id: i64) -> Result<(), ScheduleDataError> {
        let now = self.now();
        let mut t = self.lock();
        t.require_schedule(user_id, schedule_id)?;
        let schedule = t.schedules.iter_mut().find(|s| s.id == schedule_id).expect("checked above");
        schedule.revision += 1;
        schedule.status = DELETED.into();
        schedule.updated_at = now;
        Ok(())
    }

    /// `replaceScheduleDocument`: check the revision and the block rules, then rename the
    /// schedule and sync its blocks by client id, all or nothing.
    pub fn replace_schedule_document(&self, user_id: i64, schedule_id: i64, input: ReplaceInput) -> Result<ScheduleDocument, ScheduleDataError> {
        let now = self.now();
        let mut t = self.lock();
        let schedule = t.require_schedule(user_id, schedule_id)?.clone();
        if input.base_revision != schedule.revision {
            return Err(ScheduleDataError::new("Schedule is out of date.", 409));
        }
        assert_no_overlaps(&input.blocks)?;
        // The rest of the checks run before any write, which is what the transaction's
        // rollback gives Remix.
        if t.schedules.iter().any(|s| s.user_id == user_id && s.name == input.name && s.id != schedule_id) {
            return Err(ScheduleDataError::constraint("Schedule data conflicts with an existing record."));
        }
        let mut seen = std::collections::HashSet::new();
        if !input.blocks.iter().all(|b| seen.insert(b.id.as_str())) {
            return Err(ScheduleDataError::new("Scheduled block ids must be unique.", 400));
        }

        let row = t.schedules.iter_mut().find(|s| s.id == schedule_id).expect("checked above");
        row.name = input.name;
        row.revision += 1;
        row.updated_at = now;
        let updated = row.clone();

        let existing = t.block_rows(schedule_id);
        let mut kept = Vec::new();
        for block in input.blocks {
            match existing.iter().find(|row| row.client_id == block.id) {
                Some(row) => {
                    let row = t.blocks.iter_mut().find(|r| r.id == row.id).expect("listed above");
                    row.name = block.name;
                    row.color = block.color;
                    row.day_of_week = block.day_of_week;
                    row.start_minute = block.start_minute;
                    row.end_minute = block.end_minute;
                    row.updated_at = now;
                    kept.push(row.id);
                }
                None => {
                    let id = t.next_id("schedule_blocks");
                    t.blocks.push(BlockRow {
                        id,
                        client_id: block.id,
                        schedule_id,
                        name: block.name,
                        color: block.color,
                        day_of_week: block.day_of_week,
                        start_minute: block.start_minute,
                        end_minute: block.end_minute,
                        created_at: now,
                        updated_at: now,
                    });
                    kept.push(id);
                }
            }
        }
        t.blocks.retain(|row| row.schedule_id != schedule_id || kept.contains(&row.id));
        Ok(t.document(&updated))
    }

    /// A schedule row in any status (`db.find(schedules, id)`).
    pub fn find_schedule(&self, id: i64) -> Option<Schedule> {
        self.lock().schedules.iter().find(|s| s.id == id).cloned()
    }

    /// Every block row of a schedule, ordered by client id.
    pub fn block_rows(&self, schedule_id: i64) -> Vec<BlockRow> {
        let mut rows = self.lock().block_rows(schedule_id);
        rows.sort_by(|a, b| a.client_id.cmp(&b.client_id));
        rows
    }

    pub fn find_block(&self, id: i64) -> Option<BlockRow> {
        self.lock().blocks.iter().find(|b| b.id == id).cloned()
    }

    pub fn schedule_count(&self) -> usize {
        self.lock().schedules.len()
    }

    pub fn block_count(&self) -> usize {
        self.lock().blocks.len()
    }
}

/// `assertNoOverlaps`: valid day and time bounds, and no overlaps within a day.
fn assert_no_overlaps(blocks: &[Block]) -> Result<(), ScheduleDataError> {
    for b in blocks {
        if !(0..=6).contains(&b.day_of_week) || b.start_minute < 0 || b.end_minute > 1440 || b.start_minute >= b.end_minute {
            return Err(ScheduleDataError::new("Scheduled blocks must have valid dayOfWeek, startMinute, and endMinute values.", 400));
        }
    }
    if !crate::layout::is_non_overlapping(blocks) {
        return Err(ScheduleDataError::new("Scheduled blocks cannot overlap.", 400));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! `data/schedules.test.ts`

    use super::*;

    fn block(id: &str, day: i64, start: i64, end: i64) -> Block {
        Block { id: id.into(), name: id.into(), color: None, day_of_week: day, start_minute: start, end_minute: end }
    }

    fn user(db: &Db, name: &str) -> User {
        db.create_user(name, "scrypt:64:x:y").unwrap()
    }

    #[test]
    fn excludes_soft_deleted_schedules_from_list_get_and_export_lookup_paths() {
        let db = Db::new();
        let u = user(&db, "test-user");
        let s = db.create_schedule(u.id, "temporary schedule").unwrap();
        db.replace_schedule_document(u.id, s.id, ReplaceInput { name: s.name.clone(), base_revision: s.revision, blocks: vec![block("read", 0, 540, 600)] })
            .unwrap();
        db.delete_schedule(u.id, s.id).unwrap();

        assert!(db.list_schedules(u.id).is_empty());
        let err = db.schedule_document(u.id, s.id).unwrap_err();
        assert_eq!((err.message.as_str(), err.status), ("Schedule not found.", 404));
        let row = db.find_schedule(s.id).unwrap();
        assert_eq!((row.status.as_str(), row.revision), ("deleted", 2));
    }

    #[test]
    fn keeps_deleted_schedule_names_reserved_for_the_same_user() {
        let db = Db::new();
        let u = user(&db, "test-user");
        let other = user(&db, "other-user");
        let s = db.create_schedule(u.id, "morning routine").unwrap();
        db.delete_schedule(u.id, s.id).unwrap();

        let err = db.create_schedule(u.id, &s.name).unwrap_err();
        assert_eq!((err.message.as_str(), err.status), ("Name must be unique.", 409));
        assert_eq!(db.create_schedule(other.id, &s.name).unwrap().name, s.name);
    }

    #[test]
    fn cascades_schedules_and_blocks_when_deleting_a_user() {
        let db = Db::new();
        let u = user(&db, "test-user");
        let s = db.create_schedule(u.id, "weekday routine").unwrap();
        db.replace_schedule_document(
            u.id,
            s.id,
            ReplaceInput { name: s.name.clone(), base_revision: s.revision, blocks: vec![block("breakfast", 0, 480, 510), block("commute", 0, 510, 540)] },
        )
        .unwrap();
        db.delete_user(u.id);
        assert_eq!((db.schedule_count(), db.block_count()), (0, 0));
    }

    #[test]
    fn replaces_schedule_blocks_by_client_id_and_deletes_omitted_blocks() {
        let db = Db::new();
        let u = user(&db, "test-user");
        db.mock_now(Some(1000));
        let s = db.create_schedule(u.id, "weekday routine").unwrap();
        db.mock_now(Some(2000));
        let first = db
            .replace_schedule_document(
                u.id,
                s.id,
                ReplaceInput { name: s.name.clone(), base_revision: s.revision, blocks: vec![block("focus", 0, 540, 600), block("admin", 0, 600, 630)] },
            )
            .unwrap();
        let initial = db.block_rows(s.id);
        let admin = initial.iter().find(|r| r.client_id == "admin").unwrap().clone();
        let focus = initial.iter().find(|r| r.client_id == "focus").unwrap().clone();

        db.mock_now(Some(3000));
        let deep_focus = Block { id: "focus".into(), name: "deep focus".into(), color: Some("blue".into()), day_of_week: 1, start_minute: 570, end_minute: 660 };
        let second = db
            .replace_schedule_document(u.id, s.id, ReplaceInput { name: "updated routine".into(), base_revision: first.revision, blocks: vec![deep_focus.clone()] })
            .unwrap();
        db.mock_now(None);

        assert_eq!(second.name, "updated routine");
        assert_eq!(second.revision, 2);
        assert_eq!(second.blocks, [deep_focus]);

        let rows = db.block_rows(s.id);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.id, focus.id);
        assert_eq!((row.client_id.as_str(), row.name.as_str(), row.day_of_week, row.start_minute, row.end_minute), ("focus", "deep focus", 1, 570, 660));
        assert_eq!((row.created_at, row.updated_at), (focus.created_at, 3000));
        assert_eq!(db.find_block(admin.id), None);
    }
}
