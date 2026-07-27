//! Shared server state: the database handle, the in-memory session store,
//! and the login rate limiter.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

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
/// Cloning an `AppState` is cheap: all fields are [`Arc`]-wrapped, so a
/// clone shares the same underlying connection, session map, and rate
/// limiter rather than copying them.
#[derive(Clone)]
pub struct AppState {
    /// The single shared SQLite connection, guarded by a mutex since
    /// `rusqlite::Connection` is not `Sync`.
    pub db: Arc<Mutex<Connection>>,
    /// Active sessions, keyed by the random token stored in the client's
    /// session cookie. Not persisted — restarting the server clears it.
    pub sessions: Arc<Mutex<HashMap<String, Session>>>,
    /// Recent login attempt timestamps, keyed by client IP, used to rate
    /// limit `/api/login` in [`crate::auth`].
    pub login_attempts: Arc<Mutex<HashMap<IpAddr, VecDeque<Instant>>>>,
}
