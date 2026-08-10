//! HTTP handlers for serving the single-page UI and the link CRUD API.
//!
//! `create_link`, `update_link`, and `delete_link` are mounted behind the
//! [`crate::auth::require_auth`] middleware in `main.rs`; `index`,
//! `list_links`, and `export_links` are public.

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    Json,
};

use crate::db;
use crate::models::{Link, LinkInput, SearchQuery};
use crate::state::AppState;

/// Serves the single-page app shell.
///
/// The HTML is embedded into the binary at compile time via
/// [`include_str!`], so there's no filesystem lookup (or missing-file
/// failure mode) at runtime. The `{{VERSION}}` placeholder in its footer is
/// substituted with the crate version on every request — cheap enough for a
/// single small file that it isn't worth caching.
pub async fn index() -> Html<String> {
    Html(include_str!("../static/index.html").replace("{{VERSION}}", env!("CARGO_PKG_VERSION")))
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

/// `GET /api/links/export` — exports every link as a Netscape bookmark file.
///
/// Public, for the same reason [`list_links`] is: reading links is open to
/// anyone (see the README's "Authentication" section). Served with a
/// `Content-Disposition` attachment header so browsers download it rather
/// than rendering it.
pub async fn export_links(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match db::list_links(&conn, None, None) {
        Ok(links) => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"linkrs-bookmarks.html\"",
                ),
            ],
            render_netscape_bookmarks(&links),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Renders `links` as a Netscape bookmark file — the de-facto interchange
/// format every browser (and most bookmarking services) accepts on import.
///
/// The format is a flat `<DL>` list of `<DT><A>` entries; the odd unclosed
/// tags and the `<!DOCTYPE NETSCAPE-Bookmark-file-1>` line are load-bearing
/// for importers, so don't "tidy" them into well-formed HTML.
///
/// `ADD_DATE` and `LAST_MODIFIED` both carry `date_modified` as a Unix
/// timestamp, since there's no created-at column to draw a real `ADD_DATE`
/// from. A `date_modified` that doesn't parse as RFC 3339 (nothing writes
/// one today, but the column is plain TEXT) leaves both attributes off that
/// entry rather than failing the whole export.
fn render_netscape_bookmarks(links: &[Link]) -> String {
    let mut out = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n\
         <!-- This is an automatically generated file.\n\
         \x20    It will be read and overwritten.\n\
         \x20    DO NOT EDIT! -->\n\
         <META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n\
         <TITLE>Bookmarks</TITLE>\n\
         <H1>Bookmarks</H1>\n\
         <DL><p>\n",
    );
    for link in links {
        let mut attrs = format!("HREF=\"{}\"", html_escape(&link.url));
        if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&link.date_modified) {
            let epoch = ts.timestamp();
            attrs.push_str(&format!(" ADD_DATE=\"{epoch}\" LAST_MODIFIED=\"{epoch}\""));
        }
        if !link.tags.is_empty() {
            attrs.push_str(&format!(" TAGS=\"{}\"", html_escape(&link.tags.join(","))));
        }
        out.push_str(&format!(
            "    <DT><A {attrs}>{}</A>\n",
            html_escape(&link.name)
        ));
    }
    out.push_str("</DL><p>\n");
    out
}

/// Escapes the characters that would otherwise break out of HTML text or a
/// double-quoted attribute value. `&` is replaced first so the ampersands
/// introduced by the later replacements aren't escaped a second time.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
