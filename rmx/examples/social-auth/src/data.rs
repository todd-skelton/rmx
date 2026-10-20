//! Users, linked provider accounts and password reset tokens (data/schema.ts, db/seed.sql),
//! kept in memory: the Remix demo uses SQLite through remix/data-table, which is not what
//! this port is about.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct User {
    pub id: u64,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A provider account linked to a user (the `auth_accounts` table).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AuthAccount {
    pub id: u64,
    pub user_id: u64,
    pub provider: String,
    pub provider_account_id: String,
    pub email: Option<String>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub profile_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// What a provider profile says about the person (resolve-external-auth.ts).
#[derive(Clone, Debug, Default)]
pub struct ExternalProfile {
    pub email: Option<String>,
    /// The email when it may link to an existing local account (verified addresses only).
    pub linkable_email: Option<String>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub profile_json: String,
}

#[derive(Clone, Debug)]
pub struct ResetToken {
    pub token: String,
    pub user_id: u64,
    pub expires_at: i64,
}

#[derive(Default)]
struct Tables {
    users: Vec<User>,
    auth_accounts: Vec<AuthAccount>,
    reset_tokens: HashMap<String, ResetToken>,
}

/// The database handle that `load_database` puts in each request's context.
#[derive(Clone, Default)]
pub struct Db(Arc<Mutex<Tables>>);

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn normalize_optional_text(value: &str) -> Option<String> {
    Some(value.trim().to_string()).filter(|v| !v.is_empty())
}

impl Db {
    /// The two demo accounts from db/seed.sql (password `password123`), with the hashes
    /// Node produced, so logging in also checks rmx-crypto against Web Crypto.
    pub fn seeded() -> Db {
        let seed = |id, email: &str, hash: &str, name: &str, avatar: &str| User {
            id,
            email: Some(email.into()),
            password_hash: Some(hash.into()),
            name: Some(name.into()),
            avatar_url: Some(avatar.into()),
            created_at: 1705276800000,
            updated_at: 1705276800000,
        };
        let db = Db::default();
        db.tables().users = vec![
            seed(1, "admin@example.com", "pbkdf2_sha256$100000$_QBCG3pPbKgGCIvh7Xat2A$cIkwiKE-rAEVcKacgbtYL5l2ab5lrQKiIkXPCry38Xo", "Demo Admin", "https://randomuser.me/api/portraits/women/44.jpg"),
            seed(2, "user@example.com", "pbkdf2_sha256$100000$rSZlEp0rbzaRXm8aefQWpA$vU8GiCvUW0Q3qwJHsOY3rv42iiFHaKfVQmkXtdYYPWA", "Demo User", "https://randomuser.me/api/portraits/men/32.jpg"),
        ];
        db
    }

    fn tables(&self) -> MutexGuard<'_, Tables> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn find_user(&self, id: u64) -> Option<User> {
        self.tables().users.iter().find(|u| u.id == id).cloned()
    }

    pub fn find_user_by_email(&self, email: &str) -> Option<User> {
        self.tables().users.iter().find(|u| u.email.as_deref() == Some(email)).cloned()
    }

    pub fn create_user(&self, email: &str, password_hash: String, name: &str) -> User {
        let mut t = self.tables();
        let now = now_ms();
        let user = User {
            id: t.users.iter().map(|u| u.id).max().unwrap_or(0) + 1,
            email: Some(normalize_email(email)),
            password_hash: Some(password_hash),
            name: Some(name.trim().to_string()),
            avatar_url: None,
            created_at: now,
            updated_at: now,
        };
        t.users.push(user.clone());
        user
    }

    pub fn find_auth_account(&self, id: u64) -> Option<AuthAccount> {
        self.tables().auth_accounts.iter().find(|a| a.id == id).cloned()
    }

    pub fn find_auth_account_by_provider(&self, provider: &str, provider_account_id: &str) -> Option<AuthAccount> {
        self.tables().auth_accounts.iter().find(|a| a.provider == provider && a.provider_account_id == provider_account_id).cloned()
    }

    /// Find or create the local user for a provider login (`resolveExternalAuth`): an account
    /// seen before keeps its user (recreated if it was deleted), a new account links to the
    /// user with the same verified email or creates one. Profile details are refreshed each
    /// time.
    pub fn resolve_external_auth(&self, provider: &str, provider_account_id: &str, profile: &ExternalProfile) -> (User, AuthAccount) {
        let mut t = self.tables();
        let now = now_ms();
        let existing = t.auth_accounts.iter().position(|a| a.provider == provider && a.provider_account_id == provider_account_id);
        let user_id = match existing.map(|i| t.auth_accounts[i].user_id) {
            Some(id) if t.users.iter().any(|u| u.id == id) => id,
            Some(_) => create_profile_user(&mut t, profile, now),
            None => match profile.linkable_email.as_deref().and_then(|e| t.users.iter().find(|u| u.email.as_deref() == Some(e))) {
                Some(user) => user.id,
                None => create_profile_user(&mut t, profile, now),
            },
        };
        let user = t.users.iter_mut().find(|u| u.id == user_id).expect("resolved user");
        let mut changed = false;
        for (field, value) in [(&mut user.email, &profile.linkable_email), (&mut user.name, &profile.display_name), (&mut user.avatar_url, &profile.avatar_url)] {
            if value.is_some() && field != value {
                field.clone_from(value);
                changed = true;
            }
        }
        if changed {
            user.updated_at = now;
        }
        let user = user.clone();
        let details = |a: &mut AuthAccount| {
            a.user_id = user_id;
            a.email = profile.email.as_deref().map(normalize_email);
            a.username = profile.username.as_deref().and_then(normalize_optional_text);
            a.display_name = profile.display_name.as_deref().and_then(normalize_optional_text);
            a.avatar_url = profile.avatar_url.as_deref().and_then(normalize_optional_text);
            a.profile_json = profile.profile_json.clone();
            a.updated_at = now;
        };
        let account = match existing {
            Some(i) => {
                details(&mut t.auth_accounts[i]);
                t.auth_accounts[i].clone()
            }
            None => {
                let mut account = AuthAccount {
                    id: t.auth_accounts.iter().map(|a| a.id).max().unwrap_or(0) + 1,
                    user_id,
                    provider: provider.to_string(),
                    provider_account_id: provider_account_id.to_string(),
                    email: None,
                    username: None,
                    display_name: None,
                    avatar_url: None,
                    profile_json: String::new(),
                    created_at: now,
                    updated_at: now,
                };
                details(&mut account);
                t.auth_accounts.push(account.clone());
                account
            }
        };
        (user, account)
    }

    pub fn update_password(&self, id: u64, password_hash: String) {
        if let Some(u) = self.tables().users.iter_mut().find(|u| u.id == id) {
            u.password_hash = Some(password_hash);
            u.updated_at = now_ms();
        }
    }

    pub fn create_reset_token(&self, user_id: u64) -> String {
        let token = rmx::crypto::random_uuid().replace('-', "");
        let row = ResetToken { token: token.clone(), user_id, expires_at: now_ms() + 1000 * 60 * 60 };
        self.tables().reset_tokens.insert(token.clone(), row);
        token
    }

    pub fn reset_token_for_user(&self, user_id: u64) -> Option<ResetToken> {
        self.tables().reset_tokens.values().find(|t| t.user_id == user_id).cloned()
    }

    /// A reset token that has not expired (expired ones are deleted).
    pub fn load_reset_token(&self, token: &str) -> Option<ResetToken> {
        let mut t = self.tables();
        let row = t.reset_tokens.get(token)?.clone();
        if row.expires_at <= now_ms() {
            t.reset_tokens.remove(token);
            return None;
        }
        Some(row)
    }

    pub fn delete_reset_token(&self, token: &str) {
        self.tables().reset_tokens.remove(token);
    }
}

fn create_profile_user(t: &mut Tables, profile: &ExternalProfile, now: i64) -> u64 {
    let id = t.users.iter().map(|u| u.id).max().unwrap_or(0) + 1;
    t.users.push(User {
        id,
        email: profile.linkable_email.as_deref().map(normalize_email),
        password_hash: None,
        name: profile.display_name.as_deref().map(|n| n.trim().to_string()),
        avatar_url: profile.avatar_url.as_deref().and_then(normalize_optional_text),
        created_at: now,
        updated_at: now,
    });
    id
}

// utils/password-hash.ts -----------------------------------------------------------------

const HASH_PREFIX: &str = "pbkdf2_sha256";
const ITERATIONS: u32 = 100_000;

/// `pbkdf2_sha256$iterations$salt$hash`, salt and hash base64url.
pub fn hash_password(password: &str) -> String {
    let salt = rmx::crypto::random_bytes::<16>();
    let hash = rmx::crypto::pbkdf2_sha256(password.as_bytes(), &salt, ITERATIONS, 32);
    format!(
        "{HASH_PREFIX}${ITERATIONS}${}${}",
        rmx::crypto::base64url_encode(&salt),
        rmx::crypto::base64url_encode(&hash)
    )
}

pub fn verify_password(password: &str, encoded: &str) -> bool {
    let mut parts = encoded.split('$');
    let (Some(HASH_PREFIX), Some(iterations), Some(salt), Some(hash)) = (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let Some(iterations) = iterations.parse::<u32>().ok().filter(|i| *i >= 1) else {
        return false;
    };
    let (Some(salt), Some(expected)) = (rmx::crypto::base64url_decode(salt), rmx::crypto::base64url_decode(hash)) else {
        return false;
    };
    if salt.is_empty() || expected.is_empty() {
        return false;
    }
    let actual = rmx::crypto::pbkdf2_sha256(password.as_bytes(), &salt, iterations, expected.len());
    rmx::crypto::constant_time_eq(&actual, &expected)
}
