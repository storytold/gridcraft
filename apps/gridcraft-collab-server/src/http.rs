//! The HTTP surface of the server (`INTEGRATION.md` §4.1): the cabinet opens a session, inspects
//! it, commits it and closes it; health checks and metrics are open to the supervisor.
//!
//! Every `/v1` call is authenticated with the cabinet's service token. The session token of an
//! end user never travels over HTTP, only in the first WebSocket message.

use std::sync::Arc;

use axum::Router;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, body::Body};
use serde::Deserialize;

use crate::ws;
use crate::{ApiError, App, CreateSession, token_matches};

/// The routes of one process.
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/v1/sessions", post(create_session))
        .route("/v1/sessions/{id}", get(session_info).delete(close_session))
        .route("/v1/sessions/{id}/commit", post(commit))
        .route("/v1/sessions/{id}/ws", get(upgrade))
        .with_state(app)
}

/// The process is up. Storage checks come with the disk store (PR 4c).
async fn healthz() -> &'static str {
    "ok\n"
}

/// The process can serve: the routes are mounted and the state lock is not poisoned (a `std`
/// mutex would say so; the asynchronous one does not outlive a panic).
async fn readyz() -> &'static str {
    "ready\n"
}

async fn metrics(State(app): State<Arc<App>>) -> Response {
    let page = app.metrics_page().await;
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], Body::from(page)).into_response()
}

fn service_auth(app: &App, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(expected) = app.cfg.service_token.as_deref() else {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "forbidden", "this server has no service token configured"));
    };
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "the service token is missing"))?;
    if token_matches(presented, expected) {
        Ok(())
    } else {
        Err(ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "the service token is not accepted"))
    }
}

async fn create_session(State(app): State<Arc<App>>, headers: HeaderMap, Json(request): Json<CreateSession>) -> Result<Response, ApiError> {
    service_auth(&app, &headers)?;
    let created = app.create_session(request, crate::now_secs()).await?;
    Ok((StatusCode::CREATED, Json(created)).into_response())
}

async fn session_info(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<String>) -> Result<Response, ApiError> {
    service_auth(&app, &headers)?;
    let info = app.session_info(&id).await?;
    Ok(Json(info).into_response())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitQuery {
    expected_version: Option<u64>,
}

async fn commit(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<CommitQuery>,
) -> Result<Response, ApiError> {
    service_auth(&app, &headers)?;
    let reply = app.commit(&id, query.expected_version).await?;
    Ok(Json(reply).into_response())
}

async fn close_session(State(app): State<Arc<App>>, headers: HeaderMap, Path(id): Path<String>) -> Result<Response, ApiError> {
    service_auth(&app, &headers)?;
    app.close_session(&id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn upgrade(State(app): State<Arc<App>>, Path(id): Path<String>, upgrade: WebSocketUpgrade) -> Response {
    let exists = {
        let inner = app.inner.lock().await;
        inner.sessions.contains_key(&id)
    };
    if !exists {
        return ApiError::not_found("no such session").into_response();
    }
    let limits = (app.cfg.max_message, app.cfg.max_frame);
    upgrade.max_message_size(limits.0).max_frame_size(limits.1).on_upgrade(move |socket| ws::run(app, id, socket))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, JwtConfig};

    fn app(token: Option<&str>) -> Arc<App> {
        let cfg = Config { service_token: token.map(str::to_string), ..Config::default() };
        Arc::new(App::new(cfg, JwtConfig::hs256(b"test-secret").unwrap()))
    }

    #[test]
    fn a_missing_service_token_fails_closed() {
        let app = app(None);
        assert!(service_auth(&app, &HeaderMap::new()).is_err());
    }

    #[test]
    fn the_bearer_token_is_required() {
        let app = app(Some("service"));
        assert!(service_auth(&app, &HeaderMap::new()).is_err());
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer wrong".parse().unwrap());
        assert!(service_auth(&app, &headers).is_err());
        headers.insert(header::AUTHORIZATION, "Bearer service".parse().unwrap());
        assert!(service_auth(&app, &headers).is_ok());
    }
}
