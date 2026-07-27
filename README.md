# linkrs

A small self-hosted link manager. Single-page web UI for creating, searching,
editing, and deleting bookmarks, backed by a local SQLite database.

Each link has:
- **Name**
- **URL**
- **Date modified** (set automatically on create/update)

## Requirements

- Rust and Cargo (stable toolchain) — install via [rustup](https://rustup.rs)
  if you don't have it: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

No system SQLite install is needed — `rusqlite` is built with the `bundled`
feature, so SQLite is compiled in.

## Setup

```bash
cd linkrs
cargo build
```

## Running

```bash
cargo run
```

The server listens on `http://0.0.0.0:3000`. Open that address in a browser
to use the app.

On first run it creates `linkrs.db` in the current directory and initializes
the `links` table automatically. The database file persists between runs —
delete it if you want to start fresh.

## Usage

- **Add a link**: fill in Name and URL in the form and click "Add link".
- **Search**: type in the search box to filter links by name or URL (matches
  as you type).
- **Edit**: click "Edit" on a row to load it into the form, make changes, and
  click "Save changes" (or "Cancel" to discard).
- **Delete**: click "Delete" on a row and confirm.

## API

The page is a thin client over a small JSON API, also usable directly:

| Method | Path              | Body                          | Description                          |
|--------|-------------------|--------------------------------|---------------------------------------|
| GET    | `/api/links`      | —                               | List all links, newest modified first |
| GET    | `/api/links?q=x`  | —                               | List links where name or URL contains `x` |
| POST   | `/api/links`      | `{"name": "...", "url": "..."}` | Create a link                         |
| PUT    | `/api/links/:id`  | `{"name": "...", "url": "..."}` | Update a link                         |
| DELETE | `/api/links/:id`  | —                               | Delete a link                         |

Example:

```bash
curl -X POST http://localhost:3000/api/links \
  -H 'Content-Type: application/json' \
  -d '{"name":"Anthropic","url":"https://anthropic.com"}'
```

## Project layout

```
src/
  main.rs      # server setup, routes
  handlers.rs  # HTTP handlers
  db.rs        # SQLite queries
  models.rs    # request/response types
static/
  index.html   # single-page UI (embedded in the binary at build time)
```

## Configuration

Currently fixed (no config file/env vars yet):
- Port: `3000`
- Database file: `linkrs.db` in the working directory the server is started from
