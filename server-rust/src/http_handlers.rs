use crate::app_state::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::json;

pub async fn health() -> Json<serde_json::Value> {
    Json(json!({"ok": true}))
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomRequest {
    pub name: String,
    pub host_user_id: String,
}

#[derive(Debug, Deserialize)]
pub struct HostActionRequest {
    pub requester_user_id: String,
}

pub async fn create_room(
    State(state): State<AppState>,
    Json(request): Json<CreateRoomRequest>,
) -> Json<serde_json::Value> {
    match state
        .engine
        .create_room(request.name, request.host_user_id)
        .await
    {
        Ok(room) => Json(json!({"ok": true, "room": room})),
        Err(message) => Json(json!({"ok": false, "message": message})),
    }
}

pub async fn get_open_rooms(State(state): State<AppState>) -> Json<serde_json::Value> {
    let rooms = state.engine.list_open_rooms().await;
    Json(json!({"ok": true, "rooms": rooms}))
}

pub async fn close_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(request): Json<HostActionRequest>,
) -> Json<serde_json::Value> {
    match state
        .engine
        .close_room(&room_id, &request.requester_user_id)
        .await
    {
        Ok(room) => Json(json!({"ok": true, "room": room})),
        Err(message) => Json(json!({"ok": false, "message": message})),
    }
}

pub async fn reopen_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(request): Json<HostActionRequest>,
) -> Json<serde_json::Value> {
    match state
        .engine
        .reopen_room(&room_id, &request.requester_user_id)
        .await
    {
        Ok(room) => Json(json!({"ok": true, "room": room})),
        Err(message) => Json(json!({"ok": false, "message": message})),
    }
}

pub async fn get_room_state(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Json<serde_json::Value> {
    match state.engine.get_public_state(&room_id).await {
        Some(public) => Json(json!({"ok": true, "state": public})),
        None => Json(json!({"ok": false, "message": "sala no encontrada"})),
    }
}

pub async fn get_my_role(
    State(state): State<AppState>,
    Path((room_id, player_id)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    let role = state.engine.get_role(&room_id, &player_id).await;

    match role {
        Some(role) => Json(json!({
            "ok": true,
            "room_id": room_id,
            "player_id": player_id,
            "role": role
        })),
        None => Json(json!({"ok": false, "message": "jugador o sala no encontrados"})),
    }
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub action_type: Option<String>,
}

pub async fn get_room_actions(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Json<serde_json::Value> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let offset = query.offset.unwrap_or(0);

    let Some(session_id) = state.engine.get_session_id(&room_id).await else {
        return Json(json!({"ok": false, "message": "sala no encontrada"}));
    };

    let (total, items) = state
        .repo
        .fetch_actions(&session_id, query.action_type.as_deref(), limit, offset)
        .await;

    Json(json!({
        "ok": true,
        "room_id": room_id,
        "session_id": session_id,
        "total": total,
        "limit": limit,
        "offset": offset,
        "items": items
    }))
}

pub async fn get_room_chat(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Json<serde_json::Value> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let offset = query.offset.unwrap_or(0);

    let Some(session_id) = state.engine.get_session_id(&room_id).await else {
        return Json(json!({"ok": false, "message": "sala no encontrada"}));
    };

    let (total, items) = state
        .repo
        .fetch_actions(&session_id, Some("chat_message"), limit, offset)
        .await;

    Json(json!({
        "ok": true,
        "room_id": room_id,
        "session_id": session_id,
        "total": total,
        "limit": limit,
        "offset": offset,
        "items": items
    }))
}

pub async fn api_endpoints() -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "http": {
            "health": {"method": "GET", "path": "/health"},
            "create_room": {"method": "POST", "path": "/rooms", "body": ["name", "host_user_id"]},
            "open_rooms": {"method": "GET", "path": "/rooms/open"},
            "close_room": {"method": "POST", "path": "/rooms/:room_id/close", "body": ["requester_user_id"]},
            "reopen_room": {"method": "POST", "path": "/rooms/:room_id/reopen", "body": ["requester_user_id"]},
            "room_state": {"method": "GET", "path": "/rooms/:room_id/state"},
            "my_role": {"method": "GET", "path": "/rooms/:room_id/players/:player_id/role"},
            "room_actions": {
                "method": "GET",
                "path": "/rooms/:room_id/actions",
                "query": ["limit", "offset", "action_type"]
            },
            "room_chat": {
                "method": "GET",
                "path": "/rooms/:room_id/chat",
                "query": ["limit", "offset"]
            }
        },
        "websocket": {
            "path": "/ws",
            "events_client_to_server": [
                {"type": "join", "required": ["room_id", "user_id", "name"]},
                {"type": "start_game"},
                {"type": "terror_infect", "required": ["target_id"]},
                {"type": "investigate", "required": ["target_id"]},
                {"type": "vote", "required": ["target_id"]},
                {"type": "send_chat", "required": ["message"]},
                {"type": "advance_phase"}
            ],
            "events_server_to_client": ["joined", "public_state", "info", "error"],
            "rules": {"chat_cooldown_seconds": 6, "voting_duration_seconds": 120}
        }
    }))
}
