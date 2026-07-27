//! Authentication: password hashing, session management, and the
//! login/logout/me/require-auth HTTP handlers.
//!
//! Sessions are opaque random tokens held in [`crate::state::AppState`]'s
//! in-memory map, referenced by an `HttpOnly` cookie on the client. There is
//! no persistence across restarts and no CSRF protection — see the
//! "Authentication" section of the README for the threat model this is
//! designed for (trusted/local use).

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use chrono::{Duration, Utc};
use rand::{distributions::Alphanumeric, Rng};

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

use crate::db;
use crate::models::{CurrentUser, LoginInput};
use crate::state::{AppState, Session};

/// Name of the cookie that carries the session token.
pub const SESSION_COOKIE: &str = "linkrs_session";
/// How long a session remains valid after login.
const SESSION_TTL_DAYS: i64 = 7;

/// Hashes a plaintext password into a PHC-formatted Argon2 string, suitable
/// for storing in the `users.password_hash` column.
///
/// Generates a fresh random salt each call, so hashing the same password
/// twice produces different output.
///
/// # Panics
///
/// Panics if Argon2 hashing fails, which in practice only happens for
/// pathological inputs (e.g. an empty output buffer) that can't occur here.
pub fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("password hashing failed")
        .to_string()
}

/// Checks a plaintext password against a stored PHC-formatted hash.
///
/// Returns `false` (rather than erroring) if `hash` isn't a validly-formatted
/// PHC string, treating a corrupt/foreign hash the same as a mismatch.
fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Generates a random alphanumeric session token (48 characters, ~285 bits
/// of entropy) to store as the value of the session cookie.
fn random_token() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

/// Generates a random alphanumeric password (16 characters) for bootstrapping
/// the admin account when `LINKRS_ADMIN_PASSWORD` isn't set.
pub fn random_password() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect()
}

/// Creates a default admin user if the users table is empty. Returns the
/// generated password when one had to be generated (i.e. it wasn't supplied
/// via `LINKRS_ADMIN_PASSWORD`), so the caller can print it once at startup.
///
/// Reads `LINKRS_ADMIN_USER` (default `"admin"`) and `LINKRS_ADMIN_PASSWORD`
/// from the environment; these are only consulted the first time (i.e. when
/// no users exist yet).
///
/// # Errors
///
/// Returns an error if checking the user count or inserting the new user
/// fails.
pub fn ensure_admin_user(
    conn: &rusqlite::Connection,
) -> rusqlite::Result<Option<(String, String)>> {
    if db::count_users(conn)? > 0 {
        return Ok(None);
    }
    let username =
        std::env::var("LINKRS_ADMIN_USER").unwrap_or_else(|_| "admin".to_string());
    let (password, generated) = match std::env::var("LINKRS_ADMIN_PASSWORD") {
        Ok(p) if !p.is_empty() => (p, false),
        _ => (random_password(), true),
    };
    let hash = hash_password(&password);
    db::create_user(conn, &username, &hash)?;
    if generated {
        Ok(Some((username, password)))
    } else {
        Ok(None)
    }
}

/// `POST /api/login` — verifies credentials and, on success, starts a new
/// session and sets the session cookie.
///
/// Returns `401 Unauthorized` for an unknown username or wrong password
/// (the two cases are indistinguishable in the response, to avoid leaking
/// which usernames exist).
pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(input): Json<LoginInput>,
) -> impl IntoResponse {
    let password_hash = {
        let conn = state.db.lock().unwrap();
        match db::get_user_password_hash(&conn, &input.username) {
            Ok(hash) => hash,
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
            }
        }
    };

    let valid = password_hash
        .as_deref()
        .map(|hash| verify_password(&input.password, hash))
        .unwrap_or(false);

    if !valid {
        return (StatusCode::UNAUTHORIZED, "invalid username or password").into_response();
    }

    let token = random_token();
    let expires_at = Utc::now() + Duration::days(SESSION_TTL_DAYS);
    state.sessions.lock().unwrap().insert(
        token.clone(),
        Session {
            username: input.username.clone(),
            expires_at,
        },
    );

    let cookie = Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Lax)
        .max_age(time::Duration::days(SESSION_TTL_DAYS))
        .build();

    (
        jar.add(cookie),
        Json(CurrentUser {
            username: input.username,
        }),
    )
        .into_response()
}

/// `POST /api/logout` — invalidates the current session (if any) and clears
/// the session cookie.
///
/// Always returns `204 No Content`, whether or not a session was actually
/// active — logging out an already-logged-out client isn't an error.
pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        state.sessions.lock().unwrap().remove(cookie.value());
    }
    let removal = Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .max_age(time::Duration::seconds(0))
        .build();
    (jar.add(removal), StatusCode::NO_CONTENT)
}

/// `GET /api/me` — returns the current session's username, or
/// `401 Unauthorized` if there is no valid session.
///
/// Used by the frontend on load to decide whether to show the login form or
/// the add/edit/delete controls.
pub async fn me(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    let username = jar
        .get(SESSION_COOKIE)
        .and_then(|c| current_username(&state, c.value()));
    match username {
        Some(username) => Json(CurrentUser { username }).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

/// Resolves a session token to its username, if the token exists and hasn't
/// expired.
///
/// A found-but-expired session is treated the same as an unknown one; it is
/// not proactively evicted from the map here (sessions are only ever removed
/// on logout), so expired entries just accumulate until the process
/// restarts. Acceptable for this app's scale and threat model.
fn current_username(state: &AppState, token: &str) -> Option<String> {
    let sessions = state.sessions.lock().unwrap();
    sessions.get(token).and_then(|session| {
        if session.expires_at > Utc::now() {
            Some(session.username.clone())
        } else {
            None
        }
    })
}

/// Axum middleware that rejects the request with `401 Unauthorized` unless
/// the session cookie names a valid, unexpired session.
///
/// Mounted in `main.rs` only on the mutating link routes (`POST`, `PUT`,
/// `DELETE`); reading and searching links stays open to anyone.
pub async fn require_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let jar = CookieJar::from_headers(request.headers());
    let authorized = jar
        .get(SESSION_COOKIE)
        .and_then(|c| current_username(&state, c.value()))
        .is_some();

    if authorized {
        next.run(request).await
    } else {
        (StatusCode::UNAUTHORIZED, "authentication required").into_response()
    }
}
