//! Request and response payload types shared between [`crate::handlers`] and
//! [`crate::auth`].

use serde::{Deserialize, Serialize};

/// A saved link as stored in the database and returned to clients.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Link {
    pub id: i64,
    pub name: String,
    pub url: String,
    /// RFC 3339 UTC timestamp, set automatically on create/update.
    pub date_modified: String,
}

/// Request body for creating or updating a link.
#[derive(Debug, Deserialize)]
pub struct LinkInput {
    pub name: String,
    pub url: String,
}

/// Query parameters accepted by the list-links endpoint.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    /// Optional case-insensitive substring to filter links by name or URL.
    pub q: Option<String>,
}

/// Request body for `/api/login`.
#[derive(Debug, Deserialize)]
pub struct LoginInput {
    pub username: String,
    pub password: String,
}

/// Response body identifying the currently authenticated user, returned by
/// `/api/login` and `/api/me`.
#[derive(Debug, Serialize)]
pub struct CurrentUser {
    pub username: String,
}
