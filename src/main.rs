mod auth;
mod db;
mod handlers;
mod models;
mod state;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{
    middleware,
    routing::{get, post, put},
    Router,
};
use rusqlite::Connection;
use tower_http::trace::TraceLayer;

use state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let conn = Connection::open("linkrs.db").expect("failed to open sqlite database");
    db::init_db(&conn).expect("failed to initialize database schema");
    if let Some((username, password)) =
        auth::ensure_admin_user(&conn).expect("failed to create admin user")
    {
        tracing::warn!(
            "Generated admin credentials — username: {username}  password: {password}  \
             (set LINKRS_ADMIN_USER / LINKRS_ADMIN_PASSWORD to override)"
        );
    }

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        sessions: Arc::new(Mutex::new(HashMap::new())),
    };

    let protected = Router::new()
        .route("/api/links", post(handlers::create_link))
        .route(
            "/api/links/:id",
            put(handlers::update_link).delete(handlers::delete_link),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    let public = Router::new()
        .route("/", get(handlers::index))
        .route("/api/links", get(handlers::list_links))
        .route("/api/login", post(auth::login))
        .route("/api/logout", post(auth::logout))
        .route("/api/me", get(auth::me));

    let app = public
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind to port 3000");
    tracing::info!("linkrs listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.expect("server error");
}
