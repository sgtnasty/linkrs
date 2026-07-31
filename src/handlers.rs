//! HTTP handlers for serving the single-page UI and the link CRUD API.
//!
//! `create_link`, `update_link`, and `delete_link` are mounted behind the
//! [`crate::auth::require_auth`] middleware in `main.rs`; `index` and
//! `list_links` are public.

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    Json,
};

use crate::db;
use crate::models::{LinkInput, SearchQuery};
use crate::state::AppState;

/// Serves the single-page app shell.
///
/// The HTML is embedded into the binary at compile time via
/// [`include_str!`], so there's no filesystem lookup (or missing-file
/// failure mode) at runtime.
pub async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

/// `GET /favicon.ico` — serves the linkrs icon logo, embedded into the
/// binary at compile time via [`include_bytes!`] (same reasoning as
/// [`index`]).
pub async fn favicon() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/png")],
        include_bytes!("../media/linkrs-logo-icon-light.png").as_slice(),
    )
}

/// `GET /api/links` — lists all links, or those matching `?q=` if present.
///
/// Public: does not require authentication.
pub async fn list_links(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match db::list_links(&conn, params.q.as_deref(), params.tag.as_deref()) {
        Ok(links) => Json(links).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// `POST /api/links` — creates a link. Requires authentication.
///
/// Returns `400 Bad Request` if `name` or `url` is empty after trimming.
pub async fn create_link(
    State(state): State<AppState>,
    Json(input): Json<LinkInput>,
) -> impl IntoResponse {
    if input.name.trim().is_empty() || input.url.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "name and url are required").into_response();
    }
    let conn = state.db.lock().unwrap();
    match db::create_link(&conn, input.name.trim(), input.url.trim(), &input.tags) {
        Ok(link) => (StatusCode::CREATED, Json(link)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// `PUT /api/links/:id` — updates a link's name and URL. Requires
/// authentication.
///
/// Returns `400 Bad Request` if `name` or `url` is empty after trimming, or
/// `404 Not Found` if no link with `id` exists.
pub async fn update_link(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<LinkInput>,
) -> impl IntoResponse {
    if input.name.trim().is_empty() || input.url.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "name and url are required").into_response();
    }
    let conn = state.db.lock().unwrap();
    match db::update_link(&conn, id, input.name.trim(), input.url.trim(), &input.tags) {
        Ok(Some(link)) => Json(link).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "link not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// `DELETE /api/links/:id` — deletes a link. Requires authentication.
///
/// Returns `204 No Content` on success, or `404 Not Found` if no link with
/// `id` exists.
pub async fn delete_link(State(state): State<AppState>, Path(id): Path<i64>) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match db::delete_link(&conn, id) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "link not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
