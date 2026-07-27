# linkrs

[![Docker Pulls](https://img.shields.io/docker/pulls/sgtnasty/linkrs)](https://hub.docker.com/r/sgtnasty/linkrs)
[![Docker Image Version](https://img.shields.io/docker/v/sgtnasty/linkrs?sort=semver)](https://hub.docker.com/r/sgtnasty/linkrs)

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

Alternatively, download a prebuilt Linux (x86_64) binary from the
[releases page](https://github.com/sgtnasty/linkrs/releases) — each GitHub
release has one attached, built by `.github/workflows/release.yml`.

## Running

```bash
cargo run
```

The server listens on `https://0.0.0.0:3000`. Open that address in a browser
to use the app. It's HTTPS-only — there is no plain-HTTP fallback or
redirect, so `http://localhost:3000` will fail to connect rather than
redirect.

Since the certificate is self-signed, browsers show a trust warning on first
visit; click through it (or add a security exception) to continue.
Command-line tools need to skip verification too, e.g. `curl -k
https://localhost:3000/`.

On first run it creates `linkrs.db`, `cert.pem`, and `key.pem` in the current
directory and initializes the `links` and `users` tables automatically. These
files persist between runs — delete `linkrs.db` to reset the admin account
and links, or delete `cert.pem`/`key.pem` to generate a new TLS certificate
(see [Authentication](#authentication) for details on both).

## Running with Docker

Build the image:

```bash
docker build -t linkrs .
```

Run it, mounting a volume so `linkrs.db` (and the admin account) and the
generated TLS certificate survive container restarts:

```bash
docker run -d --name linkrs \
  -p 3000:3000 \
  -v linkrs-data:/data \
  linkrs
```

Open `https://localhost:3000` (expect a browser trust warning — see
[Authentication](#authentication)), then check the generated admin
credentials:

```bash
docker logs linkrs
```

To pin the admin credentials instead, pass them as env vars (only used the
first time, when the container starts with an empty database):

```bash
docker run -d --name linkrs \
  -p 3000:3000 \
  -v linkrs-data:/data \
  -e LINKRS_ADMIN_USER=myuser \
  -e LINKRS_ADMIN_PASSWORD=my-strong-password \
  linkrs
```

The image is a multi-stage build: a `rust:1-slim-bookworm` stage compiles a
release binary (SQLite is compiled in via `rusqlite`'s `bundled` feature, so
a C toolchain is needed there but not at runtime), and the final image is
`debian:bookworm-slim` running the binary as a non-root user.

`/data` is where `linkrs.db`, `cert.pem`, and `key.pem` live — mount a named
volume or a host directory there. The container's entrypoint starts as root
just long enough to `chown`
`/data` to the app's user before dropping privileges and running linkrs, so
it works regardless of what UID owns the mounted directory on the host
(fresh named volumes and host bind mounts are both commonly root-owned, or
owned by a host user whose UID doesn't match the container's).

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
To pin known credentials instead of a random password, either export env vars
before the first run:

```bash
LINKRS_ADMIN_USER=myuser LINKRS_ADMIN_PASSWORD=my-strong-password cargo run
```

or copy `.env.example` to `.env` and fill it in — linkrs loads `.env`
automatically at startup:

```bash
cp .env.example .env
# edit .env, then:
cargo run
```

`.env` is gitignored, so real credentials never get committed. These vars are
only read when the `users` table is empty (i.e. once, at bootstrap) —
changing them later has no effect on an existing account.

Log in from the web UI's login form, or via the API (see below). Sessions are
stored server-side in memory and referenced by an `HttpOnly`, `Secure`
cookie, valid for 7 days or until you log out; restarting the server
invalidates all sessions. The `Secure` flag means the cookie is only ever
sent over HTTPS — which is all linkrs serves, so this doesn't affect normal
use.

Passwords are hashed with Argon2 before being stored — never in plaintext.

`/api/login` is rate limited per client IP: at most 5 attempts per 60-second
sliding window (successful or not). Once tripped, further attempts get
`429 Too Many Requests` with a `Retry-After` header until the window clears.

**Note:** this is intended for trusted/local use. There's no CSRF token or
account management (registration, password reset) beyond the single
bootstrap admin, and the login rate limiter tracks state in memory per
process (so it resets on restart and isn't shared across multiple server
instances behind a load balancer). The self-signed certificate means clients
have to explicitly trust it (or skip verification) — it's fine for personal/
local use, but isn't a substitute for a CA-issued certificate (e.g. via
Let's Encrypt behind a reverse proxy) if you're exposing linkrs to other
people over the internet.

### TLS certificate

linkrs generates its own self-signed certificate (`cert.pem`/`key.pem`, in
the same directory as `linkrs.db`) the first time it starts with either file
missing, covering `localhost` and `127.0.0.1`. To reach it under a different
hostname or IP (e.g. a LAN address or a custom domain), set `LINKRS_TLS_SAN`
(comma-separated) before that first run — via env var or `.env`, same as the
admin credentials:

```bash
LINKRS_TLS_SAN=my-server.local,192.168.1.50 cargo run
```

`LINKRS_TLS_SAN` is only read at generation time; to pick up a change, delete
`cert.pem` and `key.pem` and restart (this generates a new certificate,
which browsers will need to re-trust).

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

Example (using a cookie jar to carry the session; `-k` skips certificate
verification, needed since it's self-signed):

```bash
curl -k -c cookies.txt -X POST https://localhost:3000/api/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"your-password"}'

curl -k -b cookies.txt -X POST https://localhost:3000/api/links \
  -H 'Content-Type: application/json' \
  -d '{"name":"Anthropic","url":"https://anthropic.com"}'
```

## Project layout

```
src/
  main.rs      # server setup, routes, TLS cert bootstrap
  handlers.rs  # link CRUD HTTP handlers
  auth.rs      # login/logout/me handlers, session middleware, password hashing
  state.rs     # shared AppState (db handle + in-memory session store)
  db.rs        # SQLite queries
  models.rs    # request/response types
static/
  index.html   # single-page UI (embedded in the binary at build time)
Dockerfile           # multi-stage build (rust:1-slim-bookworm -> debian:bookworm-slim)
docker-entrypoint.sh # fixes /data ownership at container start, then drops to a non-root user
.github/workflows/
  rust.yml           # cargo build + cargo test on push/PR
  release.yml        # builds a release binary and attaches it when a GitHub release is published
```

## Configuration

- Port: `3000` (fixed), HTTPS only
- Database file: `linkrs.db` in the working directory the server is started
  from (`/data` when run via the Dockerfile)
- TLS certificate/key: `cert.pem` / `key.pem`, same directory as `linkrs.db`;
  auto-generated (self-signed) if either is missing
- `LINKRS_ADMIN_USER` / `LINKRS_ADMIN_PASSWORD`: set the bootstrap admin
  account's credentials (only read once, when the `users` table is empty)
- `LINKRS_TLS_SAN`: comma-separated extra hostnames/IPs for the TLS
  certificate, beyond the default `localhost`/`127.0.0.1` (only read once,
  when generating `cert.pem`/`key.pem`)
