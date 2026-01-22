use rs1090::data::airports::{Airport, AIRPORTS};

use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use warp::http::StatusCode;
use warp::reject::Rejection;
use warp::reply::Reply;
use warp::Filter;

use crate::snapshot::Snapshot;
use crate::SharedState;

/// Information required to ask for a trajectory
#[derive(Serialize, Deserialize)]
pub struct TrackQuery {
    icao24: String,
    since: Option<f64>,
}

/// Information required to search for airports
#[derive(Serialize, Deserialize)]
pub struct Query {
    q: String,
}

/// An API error serializable to JSON
#[derive(Serialize)]
struct ErrorMessage {
    code: u16,
    message: String,
}

/// Returns all the ICAO 24-bit addresses of aircraft seen by jet1090
pub async fn icao24(
    shared: &Arc<SharedState>,
) -> Result<warp::reply::Json, Infallible> {
    let state_vectors = shared.state_vectors.read().await;
    let keys: Vec<_> =
        state_vectors.keys().map(|key| key.to_string()).collect();
    Ok::<_, Infallible>(warp::reply::json(&keys))
}

/// Returns all state vectors without any history information
pub async fn all(
    shared: &Arc<SharedState>,
) -> Result<warp::reply::Json, Infallible> {
    let state_vectors = shared.state_vectors.read().await;
    Ok::<_, Infallible>(warp::reply::json(
        &state_vectors
            .values()
            .map(|sv| &sv.cur)
            .collect::<Vec<&Snapshot>>(),
    ))
}

/// Returns the trajectory of a given aircraft matching the REST query
pub async fn track(
    shared: &Arc<SharedState>,
    q: TrackQuery,
) -> Result<warp::reply::Json, Infallible> {
    let state_vectors = shared.state_vectors.read().await;
    let res = state_vectors.get(&q.icao24).map(|sv| &sv.hist);
    let res = match q.since {
        Some(since) => {
            let res = res.map(|res| {
                res.iter()
                    .filter(|m| m.timestamp > since)
                    .collect::<Vec<_>>()
            });
            // Option<Vec<&TimedMessage>>
            warp::reply::json(&res)
        }
        // Option<&Vec<TimedMessage>>
        None => warp::reply::json(&res),
    };
    Ok::<_, Infallible>(res)
}

/// Returns decoding information about all sensors
pub async fn sensors(
    shared: &Arc<SharedState>,
) -> Result<warp::reply::Json, Infallible> {
    Ok::<_, Infallible>(warp::reply::json(&shared.sensors))
}

/// Returns a list of poential airports matching the query string
pub async fn airports(query: Query) -> Result<warp::reply::Json, Infallible> {
    let lowercase = query.q.to_lowercase();
    let res: Vec<&Airport> = AIRPORTS
        .iter()
        .filter(|a| {
            a.name.to_lowercase().contains(&lowercase)
                || a.city.to_lowercase().contains(&lowercase)
                || a.icao.to_lowercase().contains(&lowercase)
                || a.iata.to_lowercase().contains(&lowercase)
        })
        .collect();
    Ok::<_, Infallible>(warp::reply::json(&res))
}

/// Returns proper error messages in JSON format
pub async fn handle_rejection(
    err: Rejection,
) -> Result<impl Reply, Infallible> {
    // https://github.com/seanmonstar/warp/blob/master/examples/rejections.rs

    let code;
    let message;

    if err.is_not_found() {
        code = StatusCode::NOT_FOUND;
        message = "Route not found, try one of /,\n\
            /all,\n\
            /track?icao24={icao24},\n\
            /sensors or\n\
            /airport?q={string}";
    } else if err.find::<warp::reject::MethodNotAllowed>().is_some() {
        code = StatusCode::METHOD_NOT_ALLOWED;
        message = "Only GET queries are supported";
    } else if err.find::<warp::reject::InvalidQuery>().is_some() {
        code = StatusCode::BAD_REQUEST;
        message = "Invalid query";
    } else {
        eprintln!("unhandled rejection: {err:?}");
        code = StatusCode::INTERNAL_SERVER_ERROR;
        message = "Unknown error";
    }

    let json = warp::reply::json(&ErrorMessage {
        code: code.as_u16(),
        message: message.into(),
    });

    Ok(warp::reply::with_status(json, code))
}

pub async fn serve_web_api(shared: Arc<SharedState>, port: u16) {
    let home = warp::path::end().and_then(|| async {
        Ok::<_, Infallible>(warp::reply::html(
            "Welcome to the jet1090 REST API!<br>\
            Try one of the following routes:<br>\
            <ul>\
            <li><a href=\"/all\">/all</a>: returns all current state vectors</li>\
            <li><a href=\"/icao24\">/icao24</a>: returns all ICAO 24-bit addresses seen</li>\
            <li>/track?icao24={icao24}&since={timestamp}: returns the trajectory of a given aircraft since the given timestamp (optional)</li>\
            <li><a href=\"/sensors\">/sensors</a>: returns information about all sensors</li>\
            <li>/airports?q={string}: returns a list of potential airports matching the query string</li>\
            </ul>",
        ))
    });

    let shared_home = shared.clone();
    let icao24 = warp::path::end()
        .and(warp::any().map(move || shared_home.clone()))
        .and_then(
            |shared: Arc<SharedState>| async move { icao24(&shared).await },
        );

    let shared_all = shared.clone();
    let all = warp::path("all")
        .and(warp::any().map(move || shared_all.clone()))
        .and_then(|shared: Arc<SharedState>| async move { all(&shared).await });

    let shared_track = shared.clone();
    let track = warp::get()
        .and(warp::path("track"))
        .and(warp::any().map(move || shared_track.clone()))
        .and(warp::query::<TrackQuery>())
        .and_then(|shared: Arc<SharedState>, q: TrackQuery| async move {
            track(&shared, q).await
        });

    let shared_sensors = shared.clone();
    let sensors = warp::path("sensors")
        .and(warp::any().map(move || shared_sensors.clone()))
        .and_then(
            |shared: Arc<SharedState>| async move { sensors(&shared).await },
        );

    let airports = warp::path("airports")
        .and(warp::query::<Query>())
        .and_then(|query: Query| async move { airports(query).await });

    let cors = warp::cors()
        .allow_any_origin()
        .allow_headers(vec!["*"])
        .allow_methods(vec!["GET"]);

    let routes = warp::get()
        .and(home.or(icao24).or(all).or(track).or(sensors).or(airports))
        .recover(handle_rejection)
        .with(cors);

    warp::serve(routes).run(([0, 0, 0, 0], port)).await;
}
