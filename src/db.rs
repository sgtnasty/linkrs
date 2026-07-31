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
    conn.execute(
        "CREATE TABLE IF NOT EXISTS tags (
            id      INTEGER PRIMARY KEY AUTOINCREMENT,
            name    TEXT NOT NULL UNIQUE
        )",
        [],
    )?;
    // `link_tags` rows are deleted explicitly alongside their link (see
    // `delete_link`) rather than relying on `ON DELETE CASCADE` being
    // enforced, since that requires `PRAGMA foreign_keys = ON` to be set on
    // every connection — easy to lose track of. The REFERENCES clauses below
    // are kept as documentation of the relationship.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS link_tags (
            link_id INTEGER NOT NULL REFERENCES links(id),
            tag_id  INTEGER NOT NULL REFERENCES tags(id),
            PRIMARY KEY (link_id, tag_id)
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

/// Updates a user's stored password hash.
///
/// `password_hash` must already be a PHC-formatted Argon2 hash (see
/// [`crate::auth::hash_password`]) — this function does not hash it itself.
/// Returns `false` if no user with that username exists, rather than an
/// error.
///
/// # Errors
///
/// Returns an error if the update statement fails to execute.
pub fn update_password(conn: &Connection, username: &str, password_hash: &str) -> Result<bool> {
    let updated = conn.execute(
        "UPDATE users SET password_hash = ?1 WHERE username = ?2",
        params![password_hash, username],
    )?;
    Ok(updated > 0)
}

/// Lists links, optionally filtered by a case-insensitive substring match on
/// name or URL and/or by an exact (normalized, see [`set_tags_for_link`]) tag
/// name, ordered by most recently modified first. When both filters are
/// given, links must match both (AND, not OR).
///
/// Passing `None` (or an all-whitespace string) for either filter skips it.
///
/// # Errors
///
/// Returns an error if the query fails to prepare or execute.
pub fn list_links(conn: &Connection, search: Option<&str>, tag: Option<&str>) -> Result<Vec<Link>> {
    let mut conditions = Vec::new();
    let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(term) = search.map(str::trim).filter(|t| !t.is_empty()) {
        conditions.push("(name LIKE ? OR url LIKE ?)".to_string());
        let pattern = format!("%{}%", term);
        sql_params.push(Box::new(pattern.clone()));
        sql_params.push(Box::new(pattern));
    }
    if let Some(t) = tag.map(str::trim).filter(|t| !t.is_empty()) {
        conditions.push(
            "id IN (SELECT lt.link_id FROM link_tags lt
                     JOIN tags tg ON tg.id = lt.tag_id
                     WHERE tg.name = ?)"
                .to_string(),
        );
        sql_params.push(Box::new(t.to_lowercase()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "SELECT id, name, url, date_modified FROM links {where_clause} ORDER BY date_modified DESC"
    );

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = sql_params.iter().map(Box::as_ref).collect();
    let links = stmt
        .query_map(param_refs.as_slice(), row_to_link)?
        .collect::<Result<Vec<_>>>()?;

    links
        .into_iter()
        .map(|link| {
            let tags = get_tags_for_link(conn, link.id)?;
            Ok(Link { tags, ..link })
        })
        .collect()
}

/// Inserts a new link, stamping it with the current UTC time as
/// `date_modified`, associates it with `tags` (see
/// [`set_tags_for_link`]), and returns the row as stored.
///
/// # Errors
///
/// Returns an error if the insert fails.
pub fn create_link(conn: &Connection, name: &str, url: &str, tags: &[String]) -> Result<Link> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO links (name, url, date_modified) VALUES (?1, ?2, ?3)",
        params![name, url, now],
    )?;
    let id = conn.last_insert_rowid();
    set_tags_for_link(conn, id, tags)?;
    Ok(Link {
        id,
        name: name.to_string(),
        url: url.to_string(),
        date_modified: now,
        tags: get_tags_for_link(conn, id)?,
    })
}

/// Updates an existing link's name, URL, and tags, refreshing
/// `date_modified` to the current UTC time.
///
/// Returns `Ok(None)` if no link with `id` exists, rather than an error.
/// Checked before touching `link_tags`, so a nonexistent `id` leaves no
/// orphaned tag associations behind.
///
/// # Errors
///
/// Returns an error if the update statement fails to execute.
pub fn update_link(
    conn: &Connection,
    id: i64,
    name: &str,
    url: &str,
    tags: &[String],
) -> Result<Option<Link>> {
    let now = Utc::now().to_rfc3339();
    let updated = conn.execute(
        "UPDATE links SET name = ?1, url = ?2, date_modified = ?3 WHERE id = ?4",
        params![name, url, now, id],
    )?;
    if updated == 0 {
        return Ok(None);
    }
    set_tags_for_link(conn, id, tags)?;
    Ok(Some(Link {
        id,
        name: name.to_string(),
        url: url.to_string(),
        date_modified: now,
        tags: get_tags_for_link(conn, id)?,
    }))
}

/// Deletes the link with the given `id`, along with its tag associations.
///
/// Returns `true` if a row was deleted, `false` if no link with that `id`
/// existed.
///
/// # Errors
///
/// Returns an error if either delete statement fails to execute.
pub fn delete_link(conn: &Connection, id: i64) -> Result<bool> {
    conn.execute("DELETE FROM link_tags WHERE link_id = ?1", params![id])?;
    let deleted = conn.execute("DELETE FROM links WHERE id = ?1", params![id])?;
    Ok(deleted > 0)
}

/// Replaces the set of tags associated with `link_id`.
///
/// Tag names are trimmed and lowercased (so `"Rust"` and `"rust"` are the
/// same tag) and deduplicated; empty names are dropped. Unknown tag names
/// are created on the fly. Existing tag rows that end up with no links are
/// left in place rather than swept up, since they're harmless and may be
/// reused.
///
/// # Errors
///
/// Returns an error if any statement fails to execute.
fn set_tags_for_link(conn: &Connection, link_id: i64, tags: &[String]) -> Result<()> {
    conn.execute("DELETE FROM link_tags WHERE link_id = ?1", params![link_id])?;
    let mut seen = std::collections::HashSet::new();
    for raw in tags {
        let name = raw.trim().to_lowercase();
        if name.is_empty() || !seen.insert(name.clone()) {
            continue;
        }
        conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![name])?;
        let tag_id: i64 = conn.query_row(
            "SELECT id FROM tags WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO link_tags (link_id, tag_id) VALUES (?1, ?2)",
            params![link_id, tag_id],
        )?;
    }
    Ok(())
}

/// Returns the tag names associated with `link_id`, alphabetically sorted.
///
/// # Errors
///
/// Returns an error if the query fails.
fn get_tags_for_link(conn: &Connection, link_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name FROM tags t
         JOIN link_tags lt ON lt.tag_id = t.id
         WHERE lt.link_id = ?1
         ORDER BY t.name",
    )?;
    let names = stmt
        .query_map(params![link_id], |row| row.get(0))?
        .collect::<Result<Vec<_>>>()?;
    Ok(names)
}

/// Maps a `links` table row (in `id, name, url, date_modified` column order)
/// into a [`Link`]. `tags` is left empty — callers that need tags populate
/// it separately via [`get_tags_for_link`].
fn row_to_link(row: &rusqlite::Row) -> Result<Link> {
    Ok(Link {
        id: row.get(0)?,
        name: row.get(1)?,
        url: row.get(2)?,
        date_modified: row.get(3)?,
        tags: Vec::new(),
    })
}
