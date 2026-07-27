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

### Fixed

- Login password input styling to match the other form fields (it was
  missing from the styled input selector and fell back to the browser
  default look).
