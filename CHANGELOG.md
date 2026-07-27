# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
