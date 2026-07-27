mod db;
mod handlers;
mod models;

use std::sync::{Arc, Mutex};

use axum::{
    routing::{get, put},
    Router,
};
use rusqlite::Connection;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let conn = Connection::open("linkrs.db").expect("failed to open sqlite database");
    db::init_db(&conn).expect("failed to initialize database schema");
    let shared_db: handlers::SharedDb = Arc::new(Mutex::new(conn));

    let app = Router::new()
        .route("/", get(handlers::index))
        .route(
            "/api/links",
            get(handlers::list_links).post(handlers::create_link),
        )
        .route(
            "/api/links/:id",
            put(handlers::update_link).delete(handlers::delete_link),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(shared_db);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind to port 3000");
    tracing::info!("linkrs listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.expect("server error");
}
