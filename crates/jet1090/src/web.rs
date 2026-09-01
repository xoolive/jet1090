use rs1090::data::airports::{Airport, AIRPORTS};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::snapshot::Snapshot;
use crate::SharedState;

/// Information required to ask for a trajectory
#[derive(Deserialize)]
pub struct TrackQuery {
    icao24: String,
    since: Option<f64>,
}

/// Information required to search for airports
#[derive(Deserialize)]
pub struct AirportQuery {
    q: String,
}

/// An API error serializable to JSON
#[derive(Serialize)]
struct ErrorMessage {
    code: u16,
    message: String,
}

/// Returns all the ICAO 24-bit addresses of aircraft seen by jet1090
async fn icao24(State(shared): State<Arc<SharedState>>) -> impl IntoResponse {
    let state_vectors = shared.state_vectors.read().await;
    let keys: Vec<_> =
        state_vectors.keys().map(|key| key.to_string()).collect();
    Json(keys)
}

/// Returns all state vectors without any history information
async fn all(State(shared): State<Arc<SharedState>>) -> impl IntoResponse {
    let state_vectors = shared.state_vectors.read().await;
    let snapshots: Vec<&Snapshot> =
        state_vectors.values().map(|sv| &sv.cur).collect();
    Json(
        serde_json::to_value(snapshots)
            .unwrap_or(serde_json::Value::Array(vec![])),
    )
}

/// Returns the trajectory of a given aircraft matching the REST query
async fn track(
    State(shared): State<Arc<SharedState>>,
    Query(q): Query<TrackQuery>,
) -> impl IntoResponse {
    let state_vectors = shared.state_vectors.read().await;
    let res = state_vectors.get(&q.icao24).map(|sv| &sv.hist);
    match q.since {
        Some(since) => Json(serde_json::json!(res.map(|r| r
            .iter()
            .filter(|m| m.timestamp > since)
            .collect::<Vec<_>>()))),
        None => Json(serde_json::json!(res)),
    }
}

/// Returns decoding information about all sensors
async fn sensors(State(shared): State<Arc<SharedState>>) -> impl IntoResponse {
    Json(shared.sensors.clone())
}

/// Returns a list of potential airports matching the query string
async fn airports(Query(query): Query<AirportQuery>) -> impl IntoResponse {
    let lowercase = query.q.to_lowercase();
    let res: Vec<&Airport> = AIRPORTS
        .iter()
        .filter(|a| {
            a.name.to_lowercase().contains(&lowercase)
                || a.icao.to_lowercase().contains(&lowercase)
                || a.iata.to_lowercase().contains(&lowercase)
        })
        .collect();
    Json(res)
}

/// Home page with API documentation
async fn home() -> Html<&'static str> {
    Html(
        "Welcome to the jet1090 REST API!<br>\
        Try one of the following routes:<br>\
        <ul>\
        <li><a href=\"/all\">/all</a>: returns all current state vectors</li>\
        <li><a href=\"/icao24\">/icao24</a>: returns all ICAO 24-bit addresses seen</li>\
        <li>/track?icao24={icao24}&amp;since={timestamp}: returns the trajectory of a given aircraft since the given timestamp (optional)</li>\
        <li><a href=\"/sensors\">/sensors</a>: returns information about all sensors</li>\
        <li>/airports?q={string}: returns a list of potential airports matching the query string</li>\
        </ul>",
    )
}

/// Fallback handler for unknown routes
async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorMessage {
            code: StatusCode::NOT_FOUND.as_u16(),
            message: "Route not found, try one of /, /all, /icao24, /track?icao24={icao24}, /sensors or /airports?q={string}".into(),
        }),
    )
        .into_response()
}

pub async fn serve_web_api(shared: Arc<SharedState>, port: u16) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any);

    let app = Router::new()
        .route("/", get(home))
        .route("/icao24", get(icao24))
        .route("/all", get(all))
        .route("/track", get(track))
        .route("/sensors", get(sensors))
        .route("/airports", get(airports))
        .fallback(not_found)
        .with_state(shared)
        .layer(cors);

    let listener =
        tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port))
            .await
            .expect("failed to bind port");
    axum::serve(listener, app)
        .await
        .expect("web API server error");
}
