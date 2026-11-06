//! `gridcraft-collab-server`: the process around the GridCraft collaboration protocol.
//!
//! ```text
//! GRIDCRAFT_COLLAB_JWT_SECRET=... \
//! GRIDCRAFT_COLLAB_SERVICE_TOKEN=... \
//! GRIDCRAFT_COLLAB_ADDR=127.0.0.1:8787 \
//! gridcraft-collab-server
//! ```
//!
//! The token and the secret are never printed; the listen address is.

use std::sync::Arc;

use gridcraft_collab_server::http::router;
use gridcraft_collab_server::{App, Config, JwtConfig};

#[tokio::main]
async fn main() {
    if let Err(message) = run().await {
        eprintln!("gridcraft-collab-server: {message}");
        std::process::exit(2);
    }
}

async fn run() -> Result<(), String> {
    let cfg = Config::from_env().map_err(|e| e.to_string())?;
    let jwt = JwtConfig::from_env().map_err(|e| e.to_string())?;
    if cfg.service_token.is_none() {
        return Err("GRIDCRAFT_COLLAB_SERVICE_TOKEN is required".into());
    }
    let addr = cfg.addr;
    let app = Arc::new(App::new(cfg, jwt));
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| format!("cannot bind {addr}: {e}"))?;
    eprintln!("gridcraft-collab-server listening on {addr}");
    axum::serve(listener, router(app)).with_graceful_shutdown(shutdown()).await.map_err(|e| format!("the server stopped: {e}"))
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}
