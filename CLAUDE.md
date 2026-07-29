# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

linkrs is a small self-hosted link manager: a single-page web UI (embedded in
the binary) over a JSON API, backed by a local SQLite database, served over
HTTPS with an auto-generated self-signed certificate. No JS framework or
build step on the frontend — `static/index.html` is one file with inline
`<style>`/`<script>`, embedded into the binary via `include_str!` in
`handlers::index`.

## Commands

```bash
cargo build              # debug build
cargo run                 # build + run; serves https://0.0.0.0:3000
cargo test                # run tests (CI: .github/workflows/rust.yml)
cargo clippy              # lint
```

There's no dedicated test suite in `src/` currently (`cargo test` runs 0
tests) — verification is done by exercising the running server (`curl -k`
against the HTTPS endpoints) and, for frontend changes, actually loading the
page in a browser.

Run from a scratch directory (not the repo root) when testing throwaway
changes, since `Connection::open("linkrs.db")` and the TLS cert paths in
`main.rs` are relative to the current working directory — running from repo
root reuses/mutates the real local `linkrs.db`/`cert.pem`/`key.pem` (all
gitignored).

## Architecture

Module layout (`src/`):
- `main.rs` — wires everything together: opens the DB, bootstraps the admin
  account, generates/loads the TLS cert, builds the two route groups, starts
  the HTTPS server with graceful shutdown on SIGINT/SIGTERM.
- `state.rs` — `AppState`, cloned into every handler via axum's `State`
  extractor. All fields are `Arc`-wrapped: a single shared `Mutex<Connection>`
  (rusqlite's `Connection` isn't `Sync`, and there's exactly one connection
  for the whole process — no pool), an in-memory session map, and an
  in-memory login-attempt tracker for rate limiting.
- `db.rs` — all SQL. Every function takes a borrowed `&Connection`. This is
  the only place `rusqlite` types appear outside `state.rs`.
- `models.rs` — request/response DTOs shared between `handlers.rs` and
  `auth.rs`.
- `handlers.rs` — link CRUD HTTP handlers.
- `auth.rs` — login/logout/me handlers, the `require_auth` middleware,
  password hashing (Argon2), session tokens, and the login rate limiter.

### Routing split (`main.rs`)

Routes are built as two separate `Router`s and merged:
- `protected` — `POST /api/links`, `PUT`/`DELETE /api/links/:id` — has
  `auth::require_auth` mounted via `route_layer`.
- `public` — `/`, `GET /api/links`, `/api/login`, `/api/logout`, `/api/me`.

When adding a new mutating link endpoint, mount it on `protected`, not
`public`. Reading/searching links must stay open to anyone (this is a
deliberate product decision — see README's Authentication section).

### Auth model

Opaque random session tokens (48 alphanumeric chars) in an in-memory
`HashMap`, referenced by an `HttpOnly`, `Secure` cookie (`linkrs_session`).
No persistence across restarts, no CSRF protection, no session pooling
across multiple server instances — this is intentional for the "trusted/
local self-hosted" threat model, not an oversight. See the README's
"Authentication" section before changing auth behavior.

The bootstrap admin account is created once, only when the `users` table is
empty (`auth::ensure_admin_user`), from `LINKRS_ADMIN_USER`/
`LINKRS_ADMIN_PASSWORD` env vars (or a random password if unset). These vars
have no effect after that first run.

### Database

SQLite via `rusqlite` with the `bundled` feature (no system SQLite
dependency). `db::init_db` runs `CREATE TABLE IF NOT EXISTS` on every
startup, so schema changes must be additive/idempotent this way rather than
via a migration framework — there isn't one. Foreign keys between tables
(e.g. `link_tags` → `links`/`tags`) are declared for documentation but not
enforced via `PRAGMA foreign_keys`; related rows are deleted explicitly in
the same function that deletes the parent (see `db::delete_link`).

### TLS

`main.rs::ensure_tls_cert` generates a self-signed cert/key pair on first run
if either file is missing (covers `localhost`/`127.0.0.1` plus
`LINKRS_TLS_SAN`), valid 825 days (kept under Apple's ~825-day cap on
self-signed leaf certs). There is no HTTP fallback — the server is
HTTPS-only on port 3000.

### Frontend

`static/index.html` talks to the JSON API with plain `fetch`, re-fetching
the full link list after any mutation rather than patching state locally.
`checkAuth()` (calls `/api/me`) drives which UI (login form vs. add/edit
form) is shown. There's no bundler — edit the file directly; changes take
effect on the next `cargo build`/`cargo run` since it's inlined via
`include_str!`.

## Docker

Multi-stage `Dockerfile`: `rust:1-slim-bookworm` compiles a release binary
(a C toolchain is needed there for `rusqlite`'s bundled SQLite, but not at
runtime), final image is `debian:bookworm-slim` running as non-root.
`docker-entrypoint.sh` `chown`s the mounted `/data` volume to the app user
before dropping privileges, since a fresh named volume or host bind mount's
UID commonly won't match the container user's. `/data` is where
`linkrs.db`/`cert.pem`/`key.pem` live in the container.
