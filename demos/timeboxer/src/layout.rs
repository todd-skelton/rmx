//! `actions/schedules/public/schedule-layout.ts`: pure layout rules for the schedule grid
//! (move, group move, resize, copy across days, delete). Shared by the server tests and the
//! browser grid.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// `ScheduleDocument`: what the API and the grid exchange.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleDocument {
    pub blocks: Vec<Block>,
    pub id: i64,
    pub name: String,
    pub revision: i64,
    pub updated_at: i64,
}

/// A scheduled block (`ScheduleLayoutBlock`, also the JSON block document).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub color: Option<String>,
    pub day_of_week: i64,
    pub end_minute: i64,
    pub id: String,
    pub name: String,
    pub start_minute: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeEdge {
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reflow {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    pub day_minutes: i64,
    pub minimum_minute: i64,
    pub minimum_duration: i64,
    pub slot_minutes: i64,
}

impl Default for Policy {
    /// `defaultScheduleLayoutPolicy`
    fn default() -> Self {
        Policy { day_minutes: 1440, minimum_minute: 0, minimum_duration: 15, slot_minutes: 15 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Created,
    Deleted,
    Moved,
    Resized,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub after: Option<Block>,
    pub before: Option<Block>,
    pub id: String,
    pub kind: ChangeKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutResult {
    pub blocks: Vec<Block>,
    pub changes: Vec<Change>,
    pub unresolved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub day_of_week: i64,
    pub start_minute: i64,
}

struct Group {
    day_of_week: i64,
    end_minute: i64,
    start_minute: i64,
}

pub fn preview_delete_block(source: &[Block], block_id: &str) -> LayoutResult {
    let blocks = source.iter().filter(|b| b.id != block_id).cloned().collect();
    to_result(source, blocks, false)
}

/// # Panics
///
/// When `block_id` is not in `source` (`Unknown schedule block`).
pub fn preview_move_block(source: &[Block], block_id: &str, placement: Placement, policy: &Policy) -> LayoutResult {
    let original_blocks = source.to_vec();
    let moved_block = require_block(&original_blocks, block_id).clone();
    let duration = duration_of(&moved_block);

    let blocks: Vec<Block> = original_blocks.iter().filter(|b| b.id != block_id).cloned().collect();
    let mut moved_copy = moved_block.clone();
    moved_copy.day_of_week = clamp_day(placement.day_of_week as f64);
    moved_copy.start_minute = clamp_minute(placement.start_minute as f64, policy.minimum_minute, policy.day_minutes - duration, policy);
    moved_copy.end_minute = moved_copy.start_minute + duration;

    if let Some(swapped) = preview_horizontal_day_swap(&original_blocks, &moved_copy, &moved_block, policy) {
        return to_result(source, swapped, false);
    }

    let resolved = insert_block(&blocks, &moved_copy, policy, Some(&moved_block));
    let unresolved = resolved.is_none();
    to_result(source, resolved.unwrap_or(original_blocks), unresolved)
}

pub fn preview_move_block_group(source: &[Block], block_ids: &[String], anchor_block_id: &str, placement: Placement, policy: &Policy) -> LayoutResult {
    let mut unique: Vec<&str> = Vec::new();
    for id in block_ids {
        if !unique.contains(&id.as_str()) {
            unique.push(id);
        }
    }
    if unique.len() <= 1 {
        return preview_move_block(source, anchor_block_id, placement, policy);
    }

    let original_blocks = source.to_vec();
    let selected: Vec<Block> = unique.iter().map(|id| require_block(&original_blocks, id).clone()).collect();
    if !blocks_share_day(&selected) {
        return to_result(source, original_blocks, true);
    }

    let anchor = require_block(&selected, anchor_block_id).clone();
    let group = create_group(&selected);
    let group_duration = group.end_minute - group.start_minute;
    let target_day = clamp_day(placement.day_of_week as f64);
    let target_group_start = clamp_minute(
        (placement.start_minute - (anchor.start_minute - group.start_minute)) as f64,
        policy.minimum_minute,
        policy.day_minutes - group_duration,
        policy,
    );
    let moved: Vec<Block> = selected
        .iter()
        .map(|block| {
            let mut moved = block.clone();
            move_block_to(&mut moved, target_group_start + (block.start_minute - group.start_minute));
            moved.day_of_week = target_day;
            moved
        })
        .collect();
    let remaining: Vec<Block> = original_blocks.iter().filter(|b| !unique.contains(&b.id.as_str())).cloned().collect();
    let resolved = insert_block_group(&remaining, &moved, policy, &group);
    let unresolved = resolved.is_none();
    to_result(source, resolved.unwrap_or(original_blocks), unresolved)
}

pub fn preview_resize_block_time(source: &[Block], block_id: &str, edge: ResizeEdge, minute: f64, policy: &Policy) -> LayoutResult {
    let mut blocks = source.to_vec();
    let index = blocks.iter().position(|b| b.id == block_id).unwrap_or_else(|| panic!("Unknown schedule block: {block_id}"));
    let resized = &mut blocks[index];
    match edge {
        ResizeEdge::Start => {
            resized.start_minute = clamp_minute(minute, policy.minimum_minute, resized.end_minute - policy.minimum_duration, policy);
        }
        ResizeEdge::End => {
            resized.end_minute = clamp_minute(minute, resized.start_minute + policy.minimum_duration, policy.day_minutes, policy);
        }
    }
    let anchor = resized.clone();
    let direction = if edge == ResizeEdge::Start { Reflow::Up } else { Reflow::Down };
    let resolved = resolve_push(&blocks, &anchor, direction, policy);
    let unresolved = resolved.is_none();
    to_result(source, resolved.unwrap_or_else(|| source.to_vec()), unresolved)
}

/// `previewCopyBlockAcrossDays`: `create_id(source, day)` names each copy.
pub fn preview_copy_block_across_days(
    source: &[Block],
    block_id: &str,
    first_day: i64,
    last_day: i64,
    create_id: impl Fn(&Block, i64) -> String,
    policy: &Policy,
) -> LayoutResult {
    let source_block = require_block(source, block_id).clone();
    let first = clamp_day(first_day.min(last_day) as f64);
    let last = clamp_day(first_day.max(last_day) as f64);
    let mut blocks = source.to_vec();
    let mut unresolved = false;

    for day in first..=last {
        if day == source_block.day_of_week {
            continue;
        }
        let duplicate = Block { day_of_week: day, id: create_id(&source_block, day), ..source_block.clone() };
        match insert_block(&blocks, &duplicate, policy, None) {
            Some(resolved) => blocks = resolved,
            None => unresolved = true,
        }
    }
    to_result(source, blocks, unresolved)
}

struct Candidate {
    blocks: Vec<Block>,
    moved_count: usize,
    natural_distance: usize,
    total_distance: i64,
}

fn best(mut candidates: Vec<Candidate>) -> Option<Vec<Block>> {
    candidates.sort_by(|l, r| {
        l.moved_count.cmp(&r.moved_count).then(l.total_distance.cmp(&r.total_distance)).then(l.natural_distance.cmp(&r.natural_distance))
    });
    candidates.into_iter().next().map(|c| c.blocks)
}

fn insert_block(blocks: &[Block], inserted: &Block, policy: &Policy, original: Option<&Block>) -> Option<Vec<Block>> {
    let mut with_inserted = blocks.to_vec();
    with_inserted.push(inserted.clone());
    if collisions(&with_inserted, inserted).is_empty() {
        return is_valid_layout(&with_inserted, policy).then_some(with_inserted);
    }

    let mut target_day: Vec<Block> = blocks.iter().filter(|b| b.day_of_week == inserted.day_of_week).cloned().collect();
    target_day.sort_by_key(|b| b.start_minute);
    let others: Vec<Block> = blocks.iter().filter(|b| b.day_of_week != inserted.day_of_week).cloned().collect();
    let natural = insertion_index(&target_day, inserted, original);
    let mut candidates = Vec::new();

    for index in 0..=target_day.len() {
        let Some(before) = layout_before_anchor(&target_day[..index], inserted.start_minute, policy) else { continue };
        let Some(after) = layout_after_anchor(&target_day[index..], inserted.end_minute, policy) else { continue };
        let mut candidate = others.clone();
        candidate.extend(before.iter().cloned());
        candidate.push(inserted.clone());
        candidate.extend(after.iter().cloned());
        if !is_valid_layout(&candidate, policy) {
            continue;
        }
        let placed: Vec<Block> = before.into_iter().chain(after).collect();
        candidates.push(Candidate {
            blocks: candidate,
            moved_count: count_moved(&placed, &target_day),
            natural_distance: index.abs_diff(natural),
            total_distance: total_movement(&placed, &target_day),
        });
    }
    best(candidates)
}

fn preview_horizontal_day_swap(original_blocks: &[Block], moved: &Block, original: &Block, policy: &Policy) -> Option<Vec<Block>> {
    if moved.day_of_week == original.day_of_week || moved.start_minute != original.start_minute {
        return None;
    }
    let displaced: Vec<&Block> =
        original_blocks.iter().filter(|b| b.id != moved.id && b.day_of_week == moved.day_of_week && blocks_overlap(b, moved)).collect();
    let [displaced] = displaced.as_slice() else { return None };
    let candidate: Vec<Block> = original_blocks
        .iter()
        .map(|b| {
            if b.id == moved.id {
                moved.clone()
            } else if b.id == displaced.id {
                Block { day_of_week: original.day_of_week, ..b.clone() }
            } else {
                b.clone()
            }
        })
        .collect();
    is_valid_layout(&candidate, policy).then_some(candidate)
}

fn insert_block_group(blocks: &[Block], inserted: &[Block], policy: &Policy, original_group: &Group) -> Option<Vec<Block>> {
    let group = create_group(inserted);
    let mut with_inserted = blocks.to_vec();
    with_inserted.extend(inserted.iter().cloned());
    if is_valid_layout(&with_inserted, policy) {
        return Some(with_inserted);
    }

    let mut target_day: Vec<Block> = blocks.iter().filter(|b| b.day_of_week == group.day_of_week).cloned().collect();
    target_day.sort_by_key(|b| b.start_minute);
    let others: Vec<Block> = blocks.iter().filter(|b| b.day_of_week != group.day_of_week).cloned().collect();
    let natural = group_insertion_index(&target_day, &group, original_group);
    let mut candidates = Vec::new();

    for index in 0..=target_day.len() {
        let Some(before) = layout_before_anchor(&target_day[..index], group.start_minute, policy) else { continue };
        let Some(after) = layout_after_anchor(&target_day[index..], group.end_minute, policy) else { continue };
        let mut candidate = others.clone();
        candidate.extend(before.iter().cloned());
        candidate.extend(inserted.iter().cloned());
        candidate.extend(after.iter().cloned());
        if !is_valid_layout(&candidate, policy) {
            continue;
        }
        let placed: Vec<Block> = before.into_iter().chain(after).collect();
        candidates.push(Candidate {
            blocks: candidate,
            moved_count: count_moved(&placed, &target_day),
            natural_distance: index.abs_diff(natural),
            total_distance: total_movement(&placed, &target_day),
        });
    }
    best(candidates)
}

fn insertion_index(blocks: &[Block], inserted: &Block, original: Option<&Block>) -> usize {
    if let Some(original) = original.filter(|o| o.day_of_week == inserted.day_of_week) {
        let moving_down = inserted.start_minute > original.start_minute;
        let edge = if moving_down { inserted.end_minute } else { inserted.start_minute } as f64;
        return blocks.iter().position(|b| edge <= block_center(b)).unwrap_or(blocks.len());
    }
    blocks.iter().position(|b| inserted.start_minute <= b.start_minute).unwrap_or(blocks.len())
}

fn group_insertion_index(blocks: &[Block], inserted: &Group, original: &Group) -> usize {
    if original.day_of_week == inserted.day_of_week {
        let moving_down = inserted.start_minute > original.start_minute;
        let edge = if moving_down { inserted.end_minute } else { inserted.start_minute } as f64;
        return blocks.iter().position(|b| edge <= block_center(b)).unwrap_or(blocks.len());
    }
    blocks.iter().position(|b| inserted.start_minute <= b.start_minute).unwrap_or(blocks.len())
}

fn layout_before_anchor(blocks: &[Block], anchor_start: i64, policy: &Policy) -> Option<Vec<Block>> {
    let mut placed = Vec::with_capacity(blocks.len());
    let mut cursor = anchor_start;
    for block in blocks.iter().rev() {
        let mut next = block.clone();
        let start = next.start_minute.min(cursor - duration_of(&next));
        if start < policy.minimum_minute {
            return None;
        }
        move_block_to(&mut next, start);
        cursor = next.start_minute;
        placed.push(next);
    }
    placed.reverse();
    Some(placed)
}

fn layout_after_anchor(blocks: &[Block], anchor_end: i64, policy: &Policy) -> Option<Vec<Block>> {
    let mut placed = Vec::with_capacity(blocks.len());
    let mut cursor = anchor_end;
    for block in blocks {
        let mut next = block.clone();
        let start = next.start_minute.max(cursor);
        if start + duration_of(&next) > policy.day_minutes {
            return None;
        }
        move_block_to(&mut next, start);
        cursor = next.end_minute;
        placed.push(next);
    }
    Some(placed)
}

fn count_moved(placed: &[Block], original: &[Block]) -> usize {
    let by_id: HashMap<&str, &Block> = original.iter().map(|b| (b.id.as_str(), b)).collect();
    placed.iter().filter(|b| by_id.get(b.id.as_str()).is_some_and(|o| o.start_minute != b.start_minute)).count()
}

fn total_movement(placed: &[Block], original: &[Block]) -> i64 {
    let by_id: HashMap<&str, &Block> = original.iter().map(|b| (b.id.as_str(), b)).collect();
    placed.iter().map(|b| by_id.get(b.id.as_str()).map_or(0, |o| (o.start_minute - b.start_minute).abs())).sum()
}

fn resolve_push(blocks: &[Block], anchor: &Block, direction: Reflow, policy: &Policy) -> Option<Vec<Block>> {
    let mut candidate = blocks.to_vec();
    if collisions(&candidate, anchor).is_empty() {
        return is_valid_layout(&candidate, policy).then_some(candidate);
    }
    let mut day: Vec<&mut Block> = candidate.iter_mut().filter(|b| b.id != anchor.id && b.day_of_week == anchor.day_of_week).collect();
    match direction {
        Reflow::Down => {
            day.sort_by_key(|b| b.start_minute);
            let mut cursor = anchor.end_minute;
            for block in day {
                if block.end_minute <= anchor.start_minute {
                    continue;
                }
                if block.start_minute < cursor {
                    move_block_to(block, cursor);
                    cursor = block.end_minute;
                }
            }
        }
        Reflow::Up => {
            day.sort_by_key(|b| std::cmp::Reverse(b.start_minute));
            let mut cursor = anchor.start_minute;
            for block in day {
                if block.start_minute >= anchor.end_minute {
                    continue;
                }
                if block.end_minute > cursor {
                    let start = cursor - duration_of(block);
                    move_block_to(block, start);
                    cursor = block.start_minute;
                }
            }
        }
    }
    is_valid_layout(&candidate, policy).then_some(candidate)
}

fn to_result(before: &[Block], after: Vec<Block>, unresolved: bool) -> LayoutResult {
    let blocks = sort_blocks(after);
    let changes = changes(before, &blocks);
    LayoutResult { blocks, changes, unresolved }
}

fn changes(before_blocks: &[Block], after_blocks: &[Block]) -> Vec<Change> {
    let before_ids: HashSet<&str> = before_blocks.iter().map(|b| b.id.as_str()).collect();
    let after_by_id: HashMap<&str, &Block> = after_blocks.iter().map(|b| (b.id.as_str(), b)).collect();
    let mut changes = Vec::new();
    for before in before_blocks {
        let Some(after) = after_by_id.get(before.id.as_str()) else {
            changes.push(Change { after: None, before: Some(before.clone()), id: before.id.clone(), kind: ChangeKind::Deleted });
            continue;
        };
        if same_placement(before, after) {
            continue;
        }
        let kind = if duration_of(before) == duration_of(after) { ChangeKind::Moved } else { ChangeKind::Resized };
        changes.push(Change { after: Some((*after).clone()), before: Some(before.clone()), id: before.id.clone(), kind });
    }
    for after in after_blocks {
        if !before_ids.contains(after.id.as_str()) {
            changes.push(Change { after: Some(after.clone()), before: None, id: after.id.clone(), kind: ChangeKind::Created });
        }
    }
    changes
}

fn collisions<'a>(blocks: &'a [Block], anchor: &Block) -> Vec<&'a Block> {
    blocks.iter().filter(|b| b.id != anchor.id && b.day_of_week == anchor.day_of_week && blocks_overlap(b, anchor)).collect()
}

fn create_group(blocks: &[Block]) -> Group {
    let first = blocks.first().expect("Cannot create a schedule block group without blocks.");
    let mut group = Group { day_of_week: first.day_of_week, start_minute: first.start_minute, end_minute: first.end_minute };
    for block in blocks {
        assert_eq!(block.day_of_week, group.day_of_week, "Schedule block groups must be on one day.");
        group.start_minute = group.start_minute.min(block.start_minute);
        group.end_minute = group.end_minute.max(block.end_minute);
    }
    group
}

fn blocks_share_day(blocks: &[Block]) -> bool {
    blocks.first().is_some_and(|first| blocks.iter().all(|b| b.day_of_week == first.day_of_week))
}

fn is_valid_layout(blocks: &[Block], policy: &Policy) -> bool {
    blocks.iter().all(|b| is_valid_block(b, policy)) && is_non_overlapping(blocks)
}

fn is_valid_block(block: &Block, policy: &Policy) -> bool {
    (0..=6).contains(&block.day_of_week)
        && block.start_minute >= policy.minimum_minute
        && block.end_minute <= policy.day_minutes
        && duration_of(block) >= policy.minimum_duration
}

/// No two blocks on the same day overlap.
pub fn is_non_overlapping(blocks: &[Block]) -> bool {
    let mut by_day: HashMap<i64, Vec<&Block>> = HashMap::new();
    for block in blocks {
        by_day.entry(block.day_of_week).or_default().push(block);
    }
    by_day.into_values().all(|mut day| {
        day.sort_by_key(|b| b.start_minute);
        day.windows(2).all(|pair| !blocks_overlap(pair[0], pair[1]))
    })
}

fn same_placement(left: &Block, right: &Block) -> bool {
    left.color == right.color
        && left.day_of_week == right.day_of_week
        && left.end_minute == right.end_minute
        && left.name == right.name
        && left.start_minute == right.start_minute
}

fn blocks_overlap(left: &Block, right: &Block) -> bool {
    left.start_minute < right.end_minute && left.end_minute > right.start_minute
}

fn move_block_to(block: &mut Block, start: i64) {
    let duration = duration_of(block);
    block.start_minute = start;
    block.end_minute = start + duration;
}

fn block_center(block: &Block) -> f64 {
    block.start_minute as f64 + duration_of(block) as f64 / 2.0
}

fn duration_of(block: &Block) -> i64 {
    block.end_minute - block.start_minute
}

fn require_block<'a>(blocks: &'a [Block], id: &str) -> &'a Block {
    blocks.iter().find(|b| b.id == id).unwrap_or_else(|| panic!("Unknown schedule block: {id}"))
}

/// Day, then start, then id.
pub fn sort_blocks(mut blocks: Vec<Block>) -> Vec<Block> {
    blocks.sort_by(|l, r| l.day_of_week.cmp(&r.day_of_week).then(l.start_minute.cmp(&r.start_minute)).then_with(|| l.id.cmp(&r.id)));
    blocks
}

/// `Math.round`: halves round toward positive infinity.
pub fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

fn clamp_day(value: f64) -> i64 {
    (js_round(value) as i64).clamp(0, 6)
}

fn clamp_minute(value: f64, min: i64, max: i64, policy: &Policy) -> i64 {
    let slot = policy.slot_minutes as f64;
    let snapped = (js_round(value / slot) * slot) as i64;
    snapped.clamp(min, max.max(min))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str, day: i64, start: i64, end: i64) -> Block {
        Block { color: None, day_of_week: day, end_minute: end, id: id.into(), name: id.into(), start_minute: start }
    }

    fn times(blocks: &[Block]) -> Vec<(&str, i64, i64, i64)> {
        blocks.iter().map(|b| (b.id.as_str(), b.day_of_week, b.start_minute, b.end_minute)).collect()
    }

    fn at(day: i64, start: i64) -> Placement {
        Placement { day_of_week: day, start_minute: start }
    }

    fn policy() -> Policy {
        Policy::default()
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn moves_a_block_later_in_a_stack_by_reordering_the_stack() {
        let r = preview_move_block(&[block("one", 0, 555, 570), block("two", 0, 570, 585), block("three", 0, 585, 630)], "one", at(0, 570), &policy());
        assert_eq!(times(&r.blocks), [("two", 0, 555, 570), ("one", 0, 570, 585), ("three", 0, 585, 630)]);
    }

    #[test]
    fn moves_a_block_to_the_dragged_cell_and_shifts_the_destination_chain() {
        let r = preview_move_block(&[block("one", 0, 555, 570), block("two", 0, 570, 585), block("three", 0, 585, 630)], "two", at(0, 585), &policy());
        assert_eq!(times(&r.blocks), [("one", 0, 555, 570), ("two", 0, 585, 600), ("three", 0, 600, 645)]);
    }

    #[test]
    fn moves_a_block_upward_and_slides_the_collided_block_into_the_nearest_open_slot() {
        let r = preview_move_block(&[block("one", 0, 570, 585), block("make-breakfast", 0, 600, 630)], "make-breakfast", at(0, 570), &policy());
        assert_eq!(times(&r.blocks), [("one", 0, 555, 570), ("make-breakfast", 0, 570, 600)]);
    }

    #[test]
    fn keeps_a_taller_dragged_block_anchored_when_moving_up_into_a_shorter_block() {
        let r = preview_move_block(
            &[block("breakfast", 0, 465, 480), block("kids-to-school", 0, 480, 495), block("mail", 0, 495, 540)],
            "mail",
            at(0, 480),
            &policy(),
        );
        assert_eq!(times(&r.blocks), [("breakfast", 0, 465, 480), ("mail", 0, 480, 525), ("kids-to-school", 0, 525, 540)]);
    }

    #[test]
    fn moves_a_block_down_and_slides_the_collided_block_into_the_nearest_open_slot() {
        let r = preview_move_block(
            &[block("mail", 0, 420, 450), block("breakfast", 0, 480, 495), block("kids-to-school", 0, 495, 510)],
            "mail",
            at(0, 465),
            &policy(),
        );
        assert_eq!(times(&r.blocks), [("breakfast", 0, 450, 465), ("mail", 0, 465, 495), ("kids-to-school", 0, 495, 510)]);
    }

    #[test]
    fn moves_down_through_a_stack_and_slides_the_collided_item_into_the_nearest_open_slot() {
        let r = preview_move_block(
            &[block("make-lunches", 0, 450, 465), block("shower", 0, 465, 480), block("kids-to-school", 0, 480, 495)],
            "make-lunches",
            at(0, 465),
            &policy(),
        );
        assert_eq!(times(&r.blocks), [("shower", 0, 450, 465), ("make-lunches", 0, 465, 480), ("kids-to-school", 0, 480, 495)]);
    }

    #[test]
    fn swaps_days_for_a_clean_horizontal_move_without_mutating_the_source_blocks() {
        let blocks = [block("monday", 0, 540, 570), block("tuesday", 1, 540, 570)];
        let r = preview_move_block(&blocks, "monday", at(1, 540), &policy());
        assert!(!r.unresolved);
        assert_eq!(times(&r.blocks), [("tuesday", 0, 540, 570), ("monday", 1, 540, 570)]);
        assert_eq!(r.changes.iter().map(|c| c.kind).collect::<Vec<_>>(), [ChangeKind::Moved, ChangeKind::Moved]);
        assert_eq!(times(&blocks), [("monday", 0, 540, 570), ("tuesday", 1, 540, 570)]);
    }

    #[test]
    fn does_not_swap_horizontally_when_the_displaced_block_would_hit_a_third_block() {
        let r = preview_move_block(
            &[block("service", 0, 540, 570), block("source-conflict", 0, 570, 600), block("date-night", 1, 540, 600)],
            "service",
            at(1, 540),
            &policy(),
        );
        assert!(!r.unresolved);
        assert_eq!(times(&r.blocks), [("source-conflict", 0, 570, 600), ("service", 1, 540, 570), ("date-night", 1, 570, 630)]);
    }

    #[test]
    fn moves_across_the_week_without_filling_the_source_gap() {
        let r = preview_move_block(
            &[block("monday-morning", 0, 540, 570), block("monday-after", 0, 570, 600), block("friday-night", 4, 1200, 1230)],
            "monday-morning",
            at(4, 1200),
            &policy(),
        );
        assert_eq!(times(&r.blocks), [("monday-after", 0, 570, 600), ("monday-morning", 4, 1200, 1230), ("friday-night", 4, 1230, 1260)]);
    }

    #[test]
    fn moves_selected_adjacent_blocks_together_as_a_compound_block() {
        let r = preview_move_block_group(
            &[block("read", 0, 540, 555), block("mail", 0, 555, 585), block("workout", 0, 600, 630), block("work", 0, 630, 660)],
            &ids(&["mail", "workout"]),
            "mail",
            at(0, 585),
            &policy(),
        );
        assert!(!r.unresolved);
        assert_eq!(times(&r.blocks), [("read", 0, 540, 555), ("mail", 0, 585, 615), ("workout", 0, 630, 660), ("work", 0, 660, 690)]);
    }

    #[test]
    fn keeps_grouped_block_offsets_when_moving_selected_blocks_across_days() {
        let r = preview_move_block_group(
            &[block("plan", 0, 540, 555), block("read", 0, 555, 585), block("friday-work", 4, 555, 600)],
            &ids(&["plan", "read"]),
            "read",
            at(4, 600),
            &policy(),
        );
        assert_eq!(times(&r.blocks), [("friday-work", 4, 540, 585), ("plan", 4, 585, 600), ("read", 4, 600, 630)]);
    }

    #[test]
    fn marks_grouped_moves_unresolved_when_selected_blocks_span_multiple_days() {
        let blocks = [block("monday", 0, 540, 570), block("tuesday", 1, 540, 570)];
        let r = preview_move_block_group(&blocks, &ids(&["monday", "tuesday"]), "monday", at(2, 540), &policy());
        assert!(r.unresolved);
        assert_eq!(times(&r.blocks), times(&blocks));
    }

    #[test]
    fn marks_impossible_resizes_unresolved_without_returning_an_overlapping_layout() {
        let r = preview_resize_block_time(&[block("one", 0, 1380, 1425), block("two", 0, 1425, 1440)], "one", ResizeEdge::End, 1440.0, &policy());
        assert!(r.unresolved);
        assert_eq!(times(&r.blocks), [("one", 0, 1380, 1425), ("two", 0, 1425, 1440)]);
        assert!(r.changes.is_empty());
    }

    #[test]
    fn keeps_a_moved_stack_within_configured_visible_bounds() {
        let r = preview_move_block(
            &[
                block("read", 0, 390, 405),
                block("breakfast", 0, 405, 420),
                block("make-lunches", 0, 420, 435),
                block("mail", 0, 435, 465),
                block("weasel", 0, 465, 480),
                block("kids-to-school", 0, 480, 495),
            ],
            "weasel",
            at(0, 450),
            &Policy { minimum_minute: 390, ..policy() },
        );
        assert!(!r.unresolved);
        assert_eq!(
            times(&r.blocks),
            [
                ("read", 0, 390, 405),
                ("breakfast", 0, 405, 420),
                ("make-lunches", 0, 420, 435),
                ("weasel", 0, 450, 465),
                ("mail", 0, 465, 495),
                ("kids-to-school", 0, 495, 510)
            ]
        );
    }

    #[test]
    fn does_not_squeeze_collided_blocks_by_default() {
        let r = preview_resize_block_time(
            &[block("one", 0, 1380, 1425), block("two", 0, 1425, 1440)],
            "one",
            ResizeEdge::End,
            1430.0,
            &Policy { minimum_duration: 5, slot_minutes: 5, ..policy() },
        );
        assert!(r.unresolved);
        assert_eq!(times(&r.blocks), [("one", 0, 1380, 1425), ("two", 0, 1425, 1440)]);
        assert!(r.changes.is_empty());
    }

    #[test]
    fn moves_a_block_deeper_and_uses_nearby_open_space_for_displaced_items() {
        let r = preview_move_block(&[block("early", 0, 540, 570), block("middle", 0, 570, 600), block("late", 0, 660, 690)], "early", at(0, 660), &policy());
        assert_eq!(times(&r.blocks), [("middle", 0, 570, 600), ("late", 0, 630, 660), ("early", 0, 660, 690)]);
    }

    #[test]
    fn pushes_blocks_down_when_resizing_the_bottom_edge_into_them() {
        let r = preview_resize_block_time(&[block("one", 0, 540, 570), block("two", 0, 570, 600)], "one", ResizeEdge::End, 585.0, &policy());
        assert_eq!(times(&r.blocks), [("one", 0, 540, 585), ("two", 0, 585, 615)]);
    }

    #[test]
    fn pushes_blocks_up_when_resizing_the_top_edge_into_them() {
        let r = preview_resize_block_time(&[block("one", 0, 540, 570), block("two", 0, 570, 600)], "two", ResizeEdge::Start, 555.0, &policy());
        assert_eq!(times(&r.blocks), [("one", 0, 525, 555), ("two", 0, 555, 600)]);
    }

    #[test]
    fn copies_blocks_across_days_as_independent_events_and_reflows_each_target_day() {
        let r = preview_copy_block_across_days(
            &[block("breakfast", 0, 540, 570), block("tuesday-existing", 1, 540, 570)],
            "breakfast",
            0,
            2,
            |_, day| format!("breakfast-{day}"),
            &policy(),
        );
        assert_eq!(
            times(&r.blocks),
            [("breakfast", 0, 540, 570), ("breakfast-1", 1, 540, 570), ("tuesday-existing", 1, 570, 600), ("breakfast-2", 2, 540, 570)]
        );
    }

    #[test]
    fn keeps_successful_horizontal_copies_when_another_target_day_is_unresolved() {
        let blocks = [block("daily", 0, 0, 30), block("full-day-start", 1, 0, 30), block("full-day-end", 1, 30, 60)];
        let r = preview_copy_block_across_days(&blocks, "daily", 0, 2, |_, day| format!("daily-{day}"), &Policy { day_minutes: 60, ..policy() });
        assert!(r.unresolved);
        assert_eq!(times(&r.blocks), [("daily", 0, 0, 30), ("full-day-start", 1, 0, 30), ("full-day-end", 1, 30, 60), ("daily-2", 2, 0, 30)]);
        assert_eq!(r.changes.iter().map(|c| (c.id.as_str(), c.kind)).collect::<Vec<_>>(), [("daily-2", ChangeKind::Created)]);
        assert_eq!(times(&blocks), [("daily", 0, 0, 30), ("full-day-start", 1, 0, 30), ("full-day-end", 1, 30, 60)]);
    }

    #[test]
    fn honors_exact_first_and_last_day_time_boundaries_when_moving() {
        let blocks = [block("meeting", 3, 600, 630)];
        assert_eq!(times(&preview_move_block(&blocks, "meeting", at(0, 0), &policy()).blocks), [("meeting", 0, 0, 30)]);
        assert_eq!(times(&preview_move_block(&blocks, "meeting", at(6, 1410), &policy()).blocks), [("meeting", 6, 1410, 1440)]);
        assert_eq!(times(&blocks), [("meeting", 3, 600, 630)]);
    }

    #[test]
    fn clamps_moves_outside_the_start_and_end_of_the_day() {
        let blocks = [block("meeting", 3, 600, 630)];
        assert_eq!(times(&preview_move_block(&blocks, "meeting", at(-2, -45), &policy()).blocks), [("meeting", 0, 0, 30)]);
        assert_eq!(times(&preview_move_block(&blocks, "meeting", at(8, 1500), &policy()).blocks), [("meeting", 6, 1410, 1440)]);
    }

    #[test]
    fn snaps_resize_minutes_to_slots_while_preserving_minimum_duration() {
        let blocks = [block("task", 0, 60, 120)];
        let p = Policy { minimum_duration: 30, slot_minutes: 15, ..policy() };
        assert_eq!(times(&preview_resize_block_time(&blocks, "task", ResizeEdge::End, 101.0, &p).blocks), [("task", 0, 60, 105)]);
        assert_eq!(times(&preview_resize_block_time(&blocks, "task", ResizeEdge::Start, 104.0, &p).blocks), [("task", 0, 90, 120)]);
        assert_eq!(times(&blocks), [("task", 0, 60, 120)]);
    }

    #[test]
    fn clamps_resize_edges_to_configured_visible_day_bounds() {
        let blocks = [block("task", 0, 60, 120)];
        let start = preview_resize_block_time(&blocks, "task", ResizeEdge::Start, -30.0, &Policy { minimum_minute: 30, ..policy() });
        let end = preview_resize_block_time(&blocks, "task", ResizeEdge::End, 240.0, &Policy { day_minutes: 180, ..policy() });
        assert_eq!(times(&start.blocks), [("task", 0, 30, 120)]);
        assert_eq!(times(&end.blocks), [("task", 0, 60, 180)]);
    }

    #[test]
    fn deletes_one_block_without_changing_the_others() {
        let r = preview_delete_block(&[block("one", 0, 540, 570), block("two", 0, 570, 600)], "one");
        assert_eq!(times(&r.blocks), [("two", 0, 570, 600)]);
    }
}
