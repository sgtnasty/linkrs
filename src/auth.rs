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

pub const SESSION_COOKIE: &str = "linkrs_session";
const SESSION_TTL_DAYS: i64 = 7;

pub fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("password hashing failed")
        .to_string()
}

fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

fn random_token() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

pub fn random_password() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect()
}

/// Creates a default admin user if the users table is empty. Returns the
/// generated password when one had to be generated (i.e. it wasn't supplied
/// via LINKRS_ADMIN_PASSWORD), so the caller can print it once at startup.
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

pub async fn me(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    let username = jar
        .get(SESSION_COOKIE)
        .and_then(|c| current_username(&state, c.value()));
    match username {
        Some(username) => Json(CurrentUser { username }).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

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
