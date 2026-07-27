//! SQLite persistence for links and users.
//!
//! All functions here take a borrowed [`Connection`] rather than owning one,
//! since the connection is shared across requests behind a mutex in
//! [`crate::state::AppState`].

use crate::models::Link;
use chrono::Utc;
use rusqlite::{params, Connection, Result};

/// Creates the `links` and `users` tables if they don't already exist.
///
/// Safe to call on every startup: `CREATE TABLE IF NOT EXISTS` makes this
/// idempotent, so existing data is left untouched.
///
/// # Errors
///
/// Returns an error if the underlying SQLite statements fail to execute.
pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS links (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            name            TEXT NOT NULL,
            url             TEXT NOT NULL,
            date_modified   TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            username        TEXT NOT NULL UNIQUE,
            password_hash   TEXT NOT NULL
        )",
        [],
    )?;
    Ok(())
}

/// Returns the total number of registered users.
///
/// Used at startup to decide whether a bootstrap admin account needs to be
/// created (see [`crate::auth::ensure_admin_user`]).
///
/// # Errors
///
/// Returns an error if the query fails.
pub fn count_users(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
}

/// Inserts a new user with an already-hashed password.
///
/// `password_hash` must be a PHC-formatted Argon2 hash (see
/// [`crate::auth::hash_password`]) — this function does not hash it itself.
///
/// # Errors
///
/// Returns an error if `username` is already taken (violates the `UNIQUE`
/// constraint) or the insert otherwise fails.
pub fn create_user(conn: &Connection, username: &str, password_hash: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO users (username, password_hash) VALUES (?1, ?2)",
        params![username, password_hash],
    )?;
    Ok(())
}

/// Looks up a user's stored password hash by username.
///
/// Returns `Ok(None)` if no user with that username exists, rather than an
/// error, so callers can treat "unknown user" and "wrong password" the same
/// way without matching on error variants.
///
/// # Errors
///
/// Returns an error for any SQLite failure other than "no rows returned".
pub fn get_user_password_hash(conn: &Connection, username: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT password_hash FROM users WHERE username = ?1",
        params![username],
        |row| row.get(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        e => Err(e),
    })
}

/// Lists links, optionally filtered by a case-insensitive substring match on
/// name or URL, ordered by most recently modified first.
///
/// Passing `None` (or an all-whitespace string) returns every link.
///
/// # Errors
///
/// Returns an error if the query fails to prepare or execute.
pub fn list_links(conn: &Connection, search: Option<&str>) -> Result<Vec<Link>> {
    let mut stmt;
    let rows_iter = match search {
        Some(term) if !term.trim().is_empty() => {
            stmt = conn.prepare(
                "SELECT id, name, url, date_modified FROM links
                 WHERE name LIKE ?1 OR url LIKE ?1
                 ORDER BY date_modified DESC",
            )?;
            let pattern = format!("%{}%", term);
            stmt.query_map(params![pattern], row_to_link)?
                .collect::<Result<Vec<_>>>()?
        }
        _ => {
            stmt = conn.prepare(
                "SELECT id, name, url, date_modified FROM links ORDER BY date_modified DESC",
            )?;
            stmt.query_map([], row_to_link)?.collect::<Result<Vec<_>>>()?
        }
    };
    Ok(rows_iter)
}

/// Inserts a new link, stamping it with the current UTC time as
/// `date_modified`, and returns the row as stored.
///
/// # Errors
///
/// Returns an error if the insert fails.
pub fn create_link(conn: &Connection, name: &str, url: &str) -> Result<Link> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO links (name, url, date_modified) VALUES (?1, ?2, ?3)",
        params![name, url, now],
    )?;
    let id = conn.last_insert_rowid();
    Ok(Link {
        id,
        name: name.to_string(),
        url: url.to_string(),
        date_modified: now,
    })
}

/// Updates an existing link's name and URL, refreshing `date_modified` to
/// the current UTC time.
///
/// Returns `Ok(None)` if no link with `id` exists, rather than an error.
///
/// # Errors
///
/// Returns an error if the update statement fails to execute.
pub fn update_link(conn: &Connection, id: i64, name: &str, url: &str) -> Result<Option<Link>> {
    let now = Utc::now().to_rfc3339();
    let updated = conn.execute(
        "UPDATE links SET name = ?1, url = ?2, date_modified = ?3 WHERE id = ?4",
        params![name, url, now, id],
    )?;
    if updated == 0 {
        return Ok(None);
    }
    Ok(Some(Link {
        id,
        name: name.to_string(),
        url: url.to_string(),
        date_modified: now,
    }))
}

/// Deletes the link with the given `id`.
///
/// Returns `true` if a row was deleted, `false` if no link with that `id`
/// existed.
///
/// # Errors
///
/// Returns an error if the delete statement fails to execute.
pub fn delete_link(conn: &Connection, id: i64) -> Result<bool> {
    let deleted = conn.execute("DELETE FROM links WHERE id = ?1", params![id])?;
    Ok(deleted > 0)
}

/// Maps a `links` table row (in `id, name, url, date_modified` column order)
/// into a [`Link`].
fn row_to_link(row: &rusqlite::Row) -> Result<Link> {
    Ok(Link {
        id: row.get(0)?,
        name: row.get(1)?,
        url: row.get(2)?,
        date_modified: row.get(3)?,
    })
}
