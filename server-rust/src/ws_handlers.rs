use crate::app_state::{AppState, BroadcastEnvelope};
use crate::models::{ClientEvent, ServerEvent};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: AppState, socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.broadcast_tx.subscribe();
    let subscribed_room_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let subscribed_room_id_tx = subscribed_room_id.clone();
    let state_for_send = state.clone();

    let send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(envelope) => {
                    let current_subscribed = subscribed_room_id_tx.lock().await.clone();
                    let allow = match (&envelope.room_id, &current_subscribed) {
                        (None, _) => true,
                        (Some(target), Some(current)) => target == current,
                        (Some(_), None) => false,
                    };

                    if !allow {
                        continue;
                    }

                    if sender
                        .send(Message::Text(envelope.payload.into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let room_id = subscribed_room_id_tx.lock().await.clone();
                    let Some(room_id) = room_id else {
                        continue;
                    };

                    // Re-sincroniza con un snapshot completo cuando el socket queda atrasado.
                    let Some(public) = state_for_send.engine.get_public_state(&room_id).await else {
                        continue;
                    };

                    let payload = serde_json::to_string(&ServerEvent::PublicState {
                        room_id: room_id.clone(),
                        state: public,
                    })
                    .unwrap_or_else(|_| {
                        "{\"type\":\"error\",\"message\":\"fallo serializando estado\"}".to_string()
                    });

                    if sender.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let mut current_room_id: Option<String> = None;
    let mut current_player_id: Option<String> = None;

    while let Some(Ok(msg)) = receiver.next().await {
        let Message::Text(text) = msg else {
            continue;
        };

        let event = match serde_json::from_str::<ClientEvent>(&text) {
            Ok(v) => v,
            Err(err) => {
                broadcast_error(&state, current_room_id.as_deref(), format!("evento invalido: {}", err));
                continue;
            }
        };

        match state
            .engine
            .process_event(current_room_id.clone(), current_player_id.clone(), event)
            .await
        {
            Ok((maybe_room_id, maybe_player_id, response)) => {
                if let Some(room_id) = maybe_room_id {
                    current_room_id = Some(room_id);
                }
                if let Some(room_id) = current_room_id.clone() {
                    *subscribed_room_id.lock().await = Some(room_id);
                }
                if let Some(pid) = maybe_player_id {
                    current_player_id = Some(pid);
                }

                let serialized = serde_json::to_string(&response).unwrap_or_else(|_| {
                    "{\"type\":\"error\",\"message\":\"error serializando respuesta\"}"
                        .to_string()
                });

                let event_room_id = extract_room_id_from_event(&response)
                    .or_else(|| current_room_id.clone());
                let _ = state.broadcast_tx.send(BroadcastEnvelope {
                    room_id: event_room_id,
                    payload: serialized,
                });

                if let Some(room_id) = current_room_id.as_deref() {
                    broadcast_state(&state, room_id).await;
                }
            }
            Err(message) => {
                broadcast_error(&state, current_room_id.as_deref(), message);
            }
        }
    }

    send_task.abort();
}

async fn broadcast_state(state: &AppState, room_id: &str) {
    let Some(public) = state.engine.get_public_state(room_id).await else {
        return;
    };
    let payload = serde_json::to_string(&ServerEvent::PublicState {
        room_id: room_id.to_string(),
        state: public,
    })
    .unwrap_or_else(|_| {
        "{\"type\":\"error\",\"message\":\"fallo serializando estado\"}".to_string()
    });

    let _ = state.broadcast_tx.send(BroadcastEnvelope {
        room_id: Some(room_id.to_string()),
        payload,
    });
}

fn broadcast_error(state: &AppState, room_id: Option<&str>, message: String) {
    let payload = serde_json::to_string(&ServerEvent::Error { message }).unwrap_or_else(|_| {
        "{\"type\":\"error\",\"message\":\"fallo serializando error\"}".to_string()
    });

    let _ = state.broadcast_tx.send(BroadcastEnvelope {
        room_id: room_id.map(|r| r.to_string()),
        payload,
    });
}

fn extract_room_id_from_event(event: &ServerEvent) -> Option<String> {
    match event {
        ServerEvent::Joined { room_id, .. } => Some(room_id.clone()),
        ServerEvent::PublicState { room_id, .. } => Some(room_id.clone()),
        ServerEvent::Info { room_id, .. } => room_id.clone(),
        ServerEvent::Error { .. } => None,
    }
}
