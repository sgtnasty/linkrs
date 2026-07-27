//! Shared server state: the database handle and the in-memory session store.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::Connection;

/// A single logged-in session, keyed by session token in
/// [`AppState::sessions`].
#[derive(Clone)]
pub struct Session {
    pub username: String,
    /// When this session stops being valid, regardless of activity.
    pub expires_at: DateTime<Utc>,
}

/// Application state shared across all request handlers via axum's
/// [`axum::extract::State`] extractor.
///
/// Cloning an `AppState` is cheap: both fields are [`Arc`]-wrapped, so a
/// clone shares the same underlying connection and session map rather than
/// copying them.
#[derive(Clone)]
pub struct AppState {
    /// The single shared SQLite connection, guarded by a mutex since
    /// `rusqlite::Connection` is not `Sync`.
    pub db: Arc<Mutex<Connection>>,
    /// Active sessions, keyed by the random token stored in the client's
    /// session cookie. Not persisted — restarting the server clears it.
    pub sessions: Arc<Mutex<HashMap<String, Session>>>,
}
