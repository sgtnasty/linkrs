use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::Connection;

#[derive(Clone)]
pub struct Session {
    pub username: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub sessions: Arc<Mutex<HashMap<String, Session>>>,
}
