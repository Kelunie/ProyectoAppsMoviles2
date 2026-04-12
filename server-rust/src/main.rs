mod app_state;
mod game_engine;
mod http_handlers;
mod models;
mod repository;
mod ws_handlers;

use app_state::AppState;
use axum::{routing::{get, post}, Router};
use http_handlers::{
    api_endpoints, close_room, create_room, db_status, get_my_role, get_open_rooms,
    get_room_actions, get_room_chat, get_room_state, health, reopen_room,
};
use repository::ActionRepository;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tracing::info;
use ws_handlers::ws_handler;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .compact()
        .init();

    let mongo_uri =
        std::env::var("MONGODB_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017/virus_game".to_string());

    let port = std::env::var("PORT")
        .ok()
        .and_then(|raw| raw.parse::<u16>().ok())
        .unwrap_or(3000);

    let repo = ActionRepository::connect(&mongo_uri).await;
    let state = AppState::new(repo);

    let app = Router::new()
        .route("/health", get(health))
        .route("/db/status", get(db_status))
        .route("/api/endpoints", get(api_endpoints))
        .route("/rooms", post(create_room))
        .route("/rooms/open", get(get_open_rooms))
        .route("/rooms/:room_id/close", post(close_room))
        .route("/rooms/:room_id/reopen", post(reopen_room))
        .route("/rooms/:room_id/state", get(get_room_state))
        .route("/rooms/:room_id/players/:player_id/role", get(get_my_role))
        .route("/rooms/:room_id/actions", get(get_room_actions))
        .route("/rooms/:room_id/chat", get(get_room_chat))
        .route("/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr: SocketAddr = format!("0.0.0.0:{}", port)
        .parse()
        .expect("valid bind address");
    info!("virus-game server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind tcp listener");

    axum::serve(listener, app).await.expect("server crashed");
}
