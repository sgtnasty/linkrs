# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-07-30

### Added

- Sortable link list: click the Name, URL, or Modified column header to
  sort by it (Name/URL case-insensitively, Modified by timestamp), with a
  ▲/▼ indicator on the active column. Clicking the same header again
  reverses the direction; clicking a different header switches to it,
  starting ascending. Purely client-side over the already-fetched list, so
  it composes with the existing search and tag filters without extra
  requests.

## [0.3.0] - 2026-07-29

### Added

- Tags: links can now have zero or more tags, trimmed/lowercased/deduped on
  save. Tags render as clickable pill badges under each link's name;
  clicking one filters the list via a new `?tag=` query param on
  `GET /api/links` (combinable with `?q=` — a link must match both). The
  add/edit form gained a comma-separated Tags field.
- `CLAUDE.md` documenting the project's architecture for Claude Code.

### Changed

- The delete confirmation dialog now names the link being deleted, to make
  accidental clicks easier to catch.

## [0.2.0] - 2026-07-27

### Added

- HTTPS: linkrs now serves over TLS only (same port, 3000), using a
  self-signed certificate (`cert.pem`/`key.pem`) auto-generated on first run
  via `rcgen` and persisted alongside `linkrs.db`. Covers `localhost` and
  `127.0.0.1` by default; extra hostnames/IPs can be added via the new
  `LINKRS_TLS_SAN` env var before the certificate is first generated.
  Session cookies are now marked `Secure`. There is no HTTP fallback or
  redirect — `http://localhost:3000` fails to connect rather than
  redirecting.
- GitHub Actions workflow that builds a release binary and attaches it to a
  GitHub release when one is published.

## [0.1.0] - 2026-07-27

### Added

- Single-page web UI to create, read, update, and delete links (name, URL,
  date modified), backed by a local SQLite database.
- Search links by name or URL.
- Authentication: adding, editing, and deleting links requires a logged-in
  session; viewing and searching stay public. A bootstrap admin account is
  auto-created on first run, with credentials set via `LINKRS_ADMIN_USER` /
  `LINKRS_ADMIN_PASSWORD` (env vars or `.env`) or a randomly generated
  password printed to the console. Passwords are hashed with Argon2;
  sessions use `HttpOnly` cookies.
- Rate limiting on `/api/login`: 5 attempts per 60-second sliding window per
  client IP, returning `429 Too Many Requests` with a `Retry-After` header
  once tripped.
- `.env.example` documenting the admin credential variables.
- MIT license.
- Rustdoc documentation across all modules.
- `Dockerfile` and `docker-entrypoint.sh` for containerized deployment: a
  multi-stage build (`rust:1-slim-bookworm` compiles a release binary,
  `debian:bookworm-slim` runs it), with the entrypoint fixing up the mounted
  data directory's ownership before dropping to a non-root user, regardless
  of which UID owns the mounted volume or bind mount on the host.
- Graceful shutdown on `SIGTERM`/`Ctrl+C`, so in-flight requests finish and
  the process exits promptly (including under `docker stop`/`podman stop`)
  instead of waiting out the container runtime's kill timeout.
- GitHub Actions workflow running `cargo build` and `cargo test` on push and
  pull request.

### Fixed

- Login password input styling to match the other form fields (it was
  missing from the styled input selector and fell back to the browser
  default look).
- Container startup failure ("unable to open database file") when the data
  directory was mounted from a bind mount or volume not owned by the
  container's non-root user.
