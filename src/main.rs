//! linkrs: a small self-hosted link manager.
//!
//! Wires together the SQLite-backed [`db`] layer, the [`auth`] session
//! system, and the [`handlers`] that serve the single-page UI and its JSON
//! API, then starts an axum server on port 3000 over HTTPS with a
//! self-signed certificate.

mod auth;
mod db;
mod handlers;
mod models;
mod state;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::{
    middleware,
    routing::{get, post, put},
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use rcgen::{CertificateParams, KeyPair};
use rusqlite::Connection;
use time::{Duration, OffsetDateTime};
use tower_http::trace::TraceLayer;

use state::AppState;

/// How long a generated self-signed certificate remains valid.
///
/// Kept well under Apple platforms' ~825-day cap on certificate lifetime
/// (which they enforce even for manually-trusted self-signed leaf certs) —
/// `rcgen::CertificateParams::default()` (and thus
/// `generate_simple_self_signed`) would otherwise set a `not_after` of year
/// 4096.
const TLS_CERT_VALIDITY_DAYS: i64 = 825;

/// Opens the SQLite database, bootstraps the admin account if needed, and
/// starts the HTTPS server.
///
/// # Panics
///
/// Panics on startup failure (installing the TLS crypto provider, opening
/// the database, initializing its schema, creating the admin user or TLS
/// certificate, binding the listener, or the server itself erroring out) —
/// there's no graceful degradation for a server that can't reach its own
/// database or bind its port.
#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

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

    let (cert_path, key_path) = ensure_tls_cert(Path::new("."));

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        login_attempts: Arc::new(Mutex::new(HashMap::new())),
    };

    let protected = Router::new()
        .route("/api/links", post(handlers::create_link))
        .route(
            "/api/links/:id",
            put(handlers::update_link).delete(handlers::delete_link),
        )
        .route("/api/change-password", post(auth::change_password))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    let public = Router::new()
        .route("/", get(handlers::index))
        .route("/favicon.ico", get(handlers::favicon))
        .route("/api/links", get(handlers::list_links))
        .route("/api/login", post(auth::login))
        .route("/api/logout", post(auth::logout))
        .route("/api/me", get(auth::me));

    let app = public
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let tls_config = RustlsConfig::from_pem_file(&cert_path, &key_path)
        .await
        .expect("failed to load TLS certificate");

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let handle = axum_server::Handle::new();
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        shutdown_handle.graceful_shutdown(None);
    });

    tracing::info!(
        "linkrs v{} listening on https://0.0.0.0:3000",
        env!("CARGO_PKG_VERSION")
    );
    axum_server::tls_rustls::bind_rustls(addr, tls_config)
        .handle(handle)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .expect("server error");
}

/// Ensures a self-signed TLS certificate and private key exist in `dir`
/// (as `cert.pem` and `key.pem`), generating a fresh pair if either file is
/// missing, and returns their paths.
///
/// Regenerating whenever *either* file is absent (rather than only when
/// both are) means a process killed mid-write between the two `fs::write`
/// calls can't wedge future startups with a mismatched cert/key pair.
///
/// The certificate covers `localhost` and `127.0.0.1`, plus any extra
/// comma-separated hostnames/IPs from `LINKRS_TLS_SAN` (only read here, at
/// generation time — like `LINKRS_ADMIN_USER`/`LINKRS_ADMIN_PASSWORD`,
/// changing it later has no effect until the files are deleted).
///
/// # Panics
///
/// Panics if certificate generation or writing either file fails.
fn ensure_tls_cert(dir: &Path) -> (PathBuf, PathBuf) {
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");

    if !cert_path.exists() || !key_path.exists() {
        let mut sans = vec!["localhost".to_string(), "127.0.0.1".to_string()];
        if let Ok(extra) = std::env::var("LINKRS_TLS_SAN") {
            sans.extend(
                extra
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
            );
        }

        let key_pair = KeyPair::generate().expect("failed to generate TLS key pair");
        let mut params =
            CertificateParams::new(sans).expect("failed to build TLS certificate parameters");
        params.not_before = OffsetDateTime::now_utc() - Duration::days(1);
        params.not_after = OffsetDateTime::now_utc() + Duration::days(TLS_CERT_VALIDITY_DAYS);
        let cert = params
            .self_signed(&key_pair)
            .expect("failed to self-sign TLS certificate");

        std::fs::write(&cert_path, cert.pem()).expect("failed to write cert.pem");
        write_private_key_pem(&key_path, &key_pair.serialize_pem());

        tracing::warn!(
            "Generated self-signed TLS certificate at {} (valid {TLS_CERT_VALIDITY_DAYS} days) — \
             browsers will show a trust warning on first visit; set LINKRS_TLS_SAN \
             (comma-separated) to add extra hostnames/IPs before first run",
            cert_path.display()
        );
    }

    (cert_path, key_path)
}

/// Writes a PEM-encoded private key, restricting permissions to
/// owner-read/write (`0600`) on unix so other local users can't read it.
#[cfg(unix)]
fn write_private_key_pem(path: &Path, pem: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .expect("failed to create key.pem");
    file.write_all(pem.as_bytes())
        .expect("failed to write key.pem");
}

#[cfg(not(unix))]
fn write_private_key_pem(path: &Path, pem: &str) {
    std::fs::write(path, pem).expect("failed to write key.pem");
}

/// Resolves once a `Ctrl+C` (`SIGINT`) or `SIGTERM` is received, letting
/// [`main`] trigger `axum_server`'s [`Handle::graceful_shutdown`] so
/// in-flight requests finish instead of being cut off — and so `podman
/// stop`/`docker stop` and `systemctl stop` return promptly instead of
/// waiting out the SIGKILL grace period.
///
/// [`Handle::graceful_shutdown`]: axum_server::Handle::graceful_shutdown
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received, shutting down gracefully");
}
