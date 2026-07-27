use crate::models::Link;
use chrono::Utc;
use rusqlite::{params, Connection, Result};

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
    Ok(())
}

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

pub fn delete_link(conn: &Connection, id: i64) -> Result<bool> {
    let deleted = conn.execute("DELETE FROM links WHERE id = ?1", params![id])?;
    Ok(deleted > 0)
}

fn row_to_link(row: &rusqlite::Row) -> Result<Link> {
    Ok(Link {
        id: row.get(0)?,
        name: row.get(1)?,
        url: row.get(2)?,
        date_modified: row.get(3)?,
    })
}
