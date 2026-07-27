use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    Json,
};
use rusqlite::Connection;

use crate::db;
use crate::models::{LinkInput, SearchQuery};

pub type SharedDb = Arc<Mutex<Connection>>;

pub async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

pub async fn list_links(
    State(db): State<SharedDb>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    match db::list_links(&conn, params.q.as_deref()) {
        Ok(links) => Json(links).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn create_link(
    State(db): State<SharedDb>,
    Json(input): Json<LinkInput>,
) -> impl IntoResponse {
    if input.name.trim().is_empty() || input.url.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "name and url are required").into_response();
    }
    let conn = db.lock().unwrap();
    match db::create_link(&conn, input.name.trim(), input.url.trim()) {
        Ok(link) => (StatusCode::CREATED, Json(link)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn update_link(
    State(db): State<SharedDb>,
    Path(id): Path<i64>,
    Json(input): Json<LinkInput>,
) -> impl IntoResponse {
    if input.name.trim().is_empty() || input.url.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "name and url are required").into_response();
    }
    let conn = db.lock().unwrap();
    match db::update_link(&conn, id, input.name.trim(), input.url.trim()) {
        Ok(Some(link)) => Json(link).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "link not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn delete_link(State(db): State<SharedDb>, Path(id): Path<i64>) -> impl IntoResponse {
    let conn = db.lock().unwrap();
    match db::delete_link(&conn, id) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "link not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
