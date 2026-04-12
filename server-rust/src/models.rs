use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const CHAT_COOLDOWN_SECONDS: i64 = 6;
pub const VOTING_DURATION_SECONDS: i64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Role {
    Terrorist,
    Citizen,
    Investigator,
    Fanatic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub alive: bool,
    pub infected_counter: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Phase {
    SecretActions,
    Discussion,
    Voting,
    Resolution,
    Ended,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub player_id: String,
    pub player_name: String,
    pub message: String,
    pub sent_at_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub name: String,
    pub host_user_id: String,
    pub created_at_unix: i64,
    pub is_open: bool,
    pub game: GameState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomSummary {
    pub id: String,
    pub name: String,
    pub host_user_id: String,
    pub created_at_unix: i64,
    pub is_open: bool,
    pub player_count: usize,
    pub started: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub session_id: String,
    pub round: u32,
    pub phase: Phase,
    pub cure_progress: u8,
    pub cure_unlocked: bool,
    pub players: HashMap<String, Player>,
    pub eliminated_order: Vec<String>,
    pub winner_summary: Vec<String>,
    pub terror_actions: HashSet<String>,
    pub votes: HashMap<String, String>,
    pub investigator_action: Option<String>,
    pub pairings: Vec<Vec<String>>,
    pub turn_order: Vec<String>,
    pub current_turn_index: usize,
    pub phase_started_at_unix: i64,
    pub chat_history: Vec<ChatMessage>,
    pub chat_last_sent_at: HashMap<String, i64>,
    pub started: bool,
}

impl GameState {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            round: 1,
            phase: Phase::SecretActions,
            cure_progress: 0,
            cure_unlocked: false,
            players: HashMap::new(),
            eliminated_order: Vec::new(),
            winner_summary: Vec::new(),
            terror_actions: HashSet::new(),
            votes: HashMap::new(),
            investigator_action: None,
            pairings: Vec::new(),
            turn_order: Vec::new(),
            current_turn_index: 0,
            phase_started_at_unix: now_unix(),
            chat_history: Vec::new(),
            chat_last_sent_at: HashMap::new(),
            started: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    Join {
        room_id: String,
        user_id: String,
        name: String,
    },
    StartGame,
    TerrorInfect { target_id: String },
    Investigate { target_id: String },
    Vote { target_id: String },
    SendChat { message: String },
    AdvancePhase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    Joined {
        room_id: String,
        player_id: String,
        role: Role,
    },
    PublicState { room_id: String, state: PublicState },
    Error { message: String },
    Info { room_id: Option<String>, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicPlayer {
    pub id: String,
    pub name: String,
    pub alive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicState {
    pub room_id: String,
    pub session_id: String,
    pub round: u32,
    pub phase: Phase,
    pub cure_progress: u8,
    pub cure_unlocked: bool,
    pub players: Vec<PublicPlayer>,
    pub pairings: Vec<Vec<String>>,
    pub current_turn_player_id: Option<String>,
    pub voting_remaining_seconds: u64,
    pub chat_history: Vec<ChatMessage>,
    pub ended: bool,
    pub winners: Vec<String>,
}

impl From<&GameState> for PublicState {
    fn from(value: &GameState) -> Self {
        let mut players: Vec<PublicPlayer> = value
            .players
            .values()
            .map(|p| PublicPlayer {
                id: p.id.clone(),
                name: p.name.clone(),
                alive: p.alive,
            })
            .collect();
        players.sort_by(|a, b| a.name.cmp(&b.name));

        Self {
            room_id: String::new(),
            session_id: value.session_id.clone(),
            round: value.round,
            phase: value.phase.clone(),
            cure_progress: value.cure_progress,
            cure_unlocked: value.cure_unlocked,
            players,
            pairings: value.pairings.clone(),
            current_turn_player_id: value
                .turn_order
                .get(value.current_turn_index)
                .cloned(),
            voting_remaining_seconds: voting_remaining(value),
            chat_history: value.chat_history.clone(),
            ended: value.phase == Phase::Ended,
            winners: value.winner_summary.clone(),
        }
    }
}

impl RoomSummary {
    pub fn from_room(room: &Room) -> Self {
        Self {
            id: room.id.clone(),
            name: room.name.clone(),
            host_user_id: room.host_user_id.clone(),
            created_at_unix: room.created_at_unix,
            is_open: room.is_open,
            player_count: room.game.players.len(),
            started: room.game.started,
        }
    }
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn voting_remaining(state: &GameState) -> u64 {
    if state.phase != Phase::Voting {
        return 0;
    }

    let elapsed = now_unix().saturating_sub(state.phase_started_at_unix);
    if elapsed >= VOTING_DURATION_SECONDS {
        0
    } else {
        (VOTING_DURATION_SECONDS - elapsed) as u64
    }
}
