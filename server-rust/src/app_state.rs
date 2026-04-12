use crate::game_engine::GameEngine;
use crate::repository::ActionRepository;
use tokio::sync::broadcast;

#[derive(Clone, Debug)]
pub struct BroadcastEnvelope {
    pub room_id: Option<String>,
    pub payload: String,
}

#[derive(Clone)]
pub struct AppState {
    pub engine: GameEngine,
    pub repo: ActionRepository,
    pub broadcast_tx: broadcast::Sender<BroadcastEnvelope>,
}

impl AppState {
    pub fn new(repo: ActionRepository) -> Self {
        let (broadcast_tx, _) = broadcast::channel(128);
        let engine = GameEngine::new(repo.clone());

        Self {
            engine,
            repo,
            broadcast_tx,
        }
    }
}
