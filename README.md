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
the `links` and `users` tables automatically. The database file persists
between runs — delete it if you want to start fresh (this also resets the
admin account).

## Authentication

Viewing and searching links is public. **Adding, editing, and deleting links
requires logging in.**

On first run, if no users exist yet, linkrs creates a single admin account
and prints the credentials to the console:

```
WARN linkrs: Generated admin credentials — username: admin  password: <random>  (set LINKRS_ADMIN_USER / LINKRS_ADMIN_PASSWORD to override)
```

Copy that password down — it's only shown once (though the account remains
usable and its password can be reset by deleting `linkrs.db` and restarting).
To pin known credentials instead of a random password, set env vars before
the first run:

```bash
LINKRS_ADMIN_USER=myuser LINKRS_ADMIN_PASSWORD=my-strong-password cargo run
```

These are only read when the `users` table is empty (i.e. once, at bootstrap)
— changing them later has no effect on an existing account.

Log in from the web UI's login form, or via the API (see below). Sessions are
stored server-side in memory and referenced by an `HttpOnly` cookie, valid for
7 days or until you log out; restarting the server invalidates all sessions.

Passwords are hashed with Argon2 before being stored — never in plaintext.

**Note:** this is intended for trusted/local use. There's no CSRF token, rate
limiting, or account management (registration, password reset) beyond the
single bootstrap admin. If exposing linkrs beyond localhost, put it behind
HTTPS (e.g. a reverse proxy) since the session cookie is sent over plain HTTP
otherwise.

## Usage

- **Add a link**: fill in Name and URL in the form and click "Add link"
  (requires login).
- **Search**: type in the search box to filter links by name or URL (matches
  as you type) — no login required.
- **Edit**: click "Edit" on a row to load it into the form, make changes, and
  click "Save changes" (or "Cancel" to discard).
- **Delete**: click "Delete" on a row and confirm.
- **Log in / out**: use the bar above the forms.

## API

The page is a thin client over a small JSON API, also usable directly.
Endpoints marked 🔒 require an authenticated session cookie.

| Method | Path              | Body                             | Description                                |
|--------|-------------------|-----------------------------------|---------------------------------------------|
| GET    | `/api/links`      | —                                  | List all links, newest modified first        |
| GET    | `/api/links?q=x`  | —                                  | List links where name or URL contains `x`    |
| POST 🔒 | `/api/links`      | `{"name": "...", "url": "..."}`   | Create a link                                |
| PUT 🔒  | `/api/links/:id`  | `{"name": "...", "url": "..."}`   | Update a link                                |
| DELETE 🔒 | `/api/links/:id`  | —                                | Delete a link                                |
| POST   | `/api/login`      | `{"username": "...", "password": "..."}` | Log in, sets session cookie           |
| POST   | `/api/logout`     | —                                  | Log out, clears session                      |
| GET    | `/api/me`         | —                                  | Current user, or 401 if not logged in        |

Example (using a cookie jar to carry the session):

```bash
curl -c cookies.txt -X POST http://localhost:3000/api/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"your-password"}'

curl -b cookies.txt -X POST http://localhost:3000/api/links \
  -H 'Content-Type: application/json' \
  -d '{"name":"Anthropic","url":"https://anthropic.com"}'
```

## Project layout

```
src/
  main.rs      # server setup, routes
  handlers.rs  # link CRUD HTTP handlers
  auth.rs      # login/logout/me handlers, session middleware, password hashing
  state.rs     # shared AppState (db handle + in-memory session store)
  db.rs        # SQLite queries
  models.rs    # request/response types
static/
  index.html   # single-page UI (embedded in the binary at build time)
```

## Configuration

- Port: `3000` (fixed)
- Database file: `linkrs.db` in the working directory the server is started from
- `LINKRS_ADMIN_USER` / `LINKRS_ADMIN_PASSWORD`: set the bootstrap admin
  account's credentials (only read once, when the `users` table is empty)
