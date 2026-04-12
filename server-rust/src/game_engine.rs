mod phases;
mod win_conditions;

use crate::models::{
    ChatMessage, ClientEvent, GameState, Phase, Player, PublicState, Role, Room, RoomSummary,
    ServerEvent, CHAT_COOLDOWN_SECONDS, VOTING_DURATION_SECONDS,
};
use crate::repository::ActionRepository;
use phases::{
    all_alive_players_voted, assign_pairings_and_turns, assign_roles, resolve_infections,
    resolve_voting,
};
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;
use win_conditions::evaluate_win_conditions;

#[derive(Clone)]
pub struct GameEngine {
    rooms: Arc<Mutex<HashMap<String, Room>>>,
    repo: ActionRepository,
}

impl GameEngine {
    pub fn new(repo: ActionRepository) -> Self {
        Self {
            rooms: Arc::new(Mutex::new(HashMap::new())),
            repo,
        }
    }

    pub async fn create_room(
        &self,
        name: String,
        host_user_id: String,
    ) -> Result<RoomSummary, String> {
        let clean_name = name.trim().to_string();
        let clean_host = host_user_id.trim().to_string();

        if clean_name.is_empty() {
            return Err("room name es obligatorio".to_string());
        }
        if clean_host.is_empty() {
            return Err("host_user_id es obligatorio".to_string());
        }

        let room_id = Uuid::new_v4().to_string();
        let session_id = Uuid::new_v4().to_string();

        let room = Room {
            id: room_id.clone(),
            name: clean_name,
            host_user_id: clean_host,
            created_at_unix: now_unix(),
            is_open: true,
            game: GameState::new(session_id.clone()),
        };

        let summary = RoomSummary::from_room(&room);

        {
            let mut rooms = self.rooms.lock().await;
            rooms.insert(room_id.clone(), room);
        }

        self.repo
            .log_action(
                &session_id,
                "room_created",
                None,
                json!({"room_id": room_id, "name": summary.name, "host_user_id": summary.host_user_id}),
            )
            .await;

        Ok(summary)
    }

    pub async fn list_open_rooms(&self) -> Vec<RoomSummary> {
        let rooms = self.rooms.lock().await;
        let mut list: Vec<RoomSummary> = rooms
            .values()
            .filter(|r| r.is_open)
            .map(RoomSummary::from_room)
            .collect();

        list.sort_by(|a, b| a.created_at_unix.cmp(&b.created_at_unix));
        list
    }

    pub async fn close_room(
        &self,
        room_id: &str,
        requester_user_id: &str,
    ) -> Result<RoomSummary, String> {
        let mut rooms = self.rooms.lock().await;
        let room = rooms
            .get_mut(room_id)
            .ok_or_else(|| "sala no encontrada".to_string())?;

        if room.host_user_id != requester_user_id {
            return Err("solo el host puede cerrar la sala".to_string());
        }

        if !room.is_open {
            return Err("la sala ya esta cerrada".to_string());
        }

        room.is_open = false;
        let summary = RoomSummary::from_room(room);
        let session_id = room.game.session_id.clone();
        drop(rooms);

        self.repo
            .log_action(
                &session_id,
                "room_closed",
                Some(requester_user_id),
                json!({"room_id": room_id, "requester_user_id": requester_user_id}),
            )
            .await;

        Ok(summary)
    }

    pub async fn reopen_room(
        &self,
        room_id: &str,
        requester_user_id: &str,
    ) -> Result<RoomSummary, String> {
        let mut rooms = self.rooms.lock().await;
        let room = rooms
            .get_mut(room_id)
            .ok_or_else(|| "sala no encontrada".to_string())?;

        if room.host_user_id != requester_user_id {
            return Err("solo el host puede reabrir la sala".to_string());
        }

        if room.game.started {
            return Err("no se puede reabrir una sala con partida iniciada".to_string());
        }

        if room.is_open {
            return Err("la sala ya esta abierta".to_string());
        }

        room.is_open = true;
        let summary = RoomSummary::from_room(room);
        let session_id = room.game.session_id.clone();
        drop(rooms);

        self.repo
            .log_action(
                &session_id,
                "room_reopened",
                Some(requester_user_id),
                json!({"room_id": room_id, "requester_user_id": requester_user_id}),
            )
            .await;

        Ok(summary)
    }

    pub async fn get_public_state(&self, room_id: &str) -> Option<PublicState> {
        let rooms = self.rooms.lock().await;
        rooms.get(room_id).map(|room| {
            let mut public = PublicState::from(&room.game);
            public.room_id = room.id.clone();
            public
        })
    }

    pub async fn get_role(&self, room_id: &str, player_id: &str) -> Option<Role> {
        let rooms = self.rooms.lock().await;
        rooms
            .get(room_id)
            .and_then(|room| room.game.players.get(player_id).map(|p| p.role.clone()))
    }

    pub async fn get_session_id(&self, room_id: &str) -> Option<String> {
        let rooms = self.rooms.lock().await;
        rooms.get(room_id).map(|room| room.game.session_id.clone())
    }

    pub async fn process_event(
        &self,
        current_room_id: Option<String>,
        current_player_id: Option<String>,
        event: ClientEvent,
    ) -> Result<(Option<String>, Option<String>, ServerEvent), String> {
        match event {
            ClientEvent::Join {
                room_id,
                user_id,
                name,
            } => self.join_player(&room_id, user_id, name).await,
            ClientEvent::StartGame => {
                let room_id = current_room_id.ok_or_else(|| "debes unirte a una sala".to_string())?;
                self.start_game(&room_id).await
            }
            ClientEvent::TerrorInfect { target_id } => {
                let room_id = current_room_id.ok_or_else(|| "debes unirte a una sala".to_string())?;
                let pid = current_player_id.ok_or_else(|| "debes hacer join primero".to_string())?;
                self.terror_infect(&room_id, &pid, &target_id).await
            }
            ClientEvent::Investigate { target_id } => {
                let room_id = current_room_id.ok_or_else(|| "debes unirte a una sala".to_string())?;
                let pid = current_player_id.ok_or_else(|| "debes hacer join primero".to_string())?;
                self.investigate(&room_id, &pid, &target_id).await
            }
            ClientEvent::Vote { target_id } => {
                let room_id = current_room_id.ok_or_else(|| "debes unirte a una sala".to_string())?;
                let pid = current_player_id.ok_or_else(|| "debes hacer join primero".to_string())?;
                self.vote(&room_id, &pid, &target_id).await
            }
            ClientEvent::SendChat { message } => {
                let room_id = current_room_id.ok_or_else(|| "debes unirte a una sala".to_string())?;
                let pid = current_player_id.ok_or_else(|| "debes hacer join primero".to_string())?;
                self.send_chat(&room_id, &pid, &message).await
            }
            ClientEvent::AdvancePhase => {
                let room_id = current_room_id.ok_or_else(|| "debes unirte a una sala".to_string())?;
                self.advance_phase(&room_id).await
            }
        }
    }

    async fn join_player(
        &self,
        room_id: &str,
        user_id: String,
        name: String,
    ) -> Result<(Option<String>, Option<String>, ServerEvent), String> {
        let mut rooms = self.rooms.lock().await;
        let room = rooms
            .get_mut(room_id)
            .ok_or_else(|| "sala no encontrada".to_string())?;

        if !room.is_open {
            return Err("la sala esta cerrada".to_string());
        }

        let game = &mut room.game;
        let clean_user_id = user_id.trim().to_string();
        let clean_name = name.trim().to_string();

        if clean_user_id.is_empty() {
            return Err("user_id es obligatorio".to_string());
        }
        if clean_name.is_empty() {
            return Err("name es obligatorio".to_string());
        }

        if let Some(existing) = game.players.get_mut(&clean_user_id) {
            existing.name = clean_name;
            let existing_id = existing.id.clone();
            let existing_role = existing.role.clone();
            let session_id = game.session_id.clone();

            drop(rooms);
            self.repo
                .log_action(
                    &session_id,
                    "join_reconnect",
                    Some(&existing_id),
                    json!({"room_id": room_id, "user_id": existing_id}),
                )
                .await;

            return Ok((
                Some(room_id.to_string()),
                Some(existing_id.clone()),
                ServerEvent::Joined {
                    room_id: room_id.to_string(),
                    player_id: existing_id,
                    role: existing_role,
                },
            ));
        }

        if game.started {
            return Err("el juego ya inicio y este user_id no pertenece a la sesion".to_string());
        }
        if game.players.len() >= 10 {
            return Err("maximo 10 jugadores".to_string());
        }

        let pid = clean_user_id;
        game.players.insert(
            pid.clone(),
            Player {
                id: pid.clone(),
                name: clean_name,
                role: Role::Citizen,
                alive: true,
                infected_counter: None,
            },
        );

        let session_id = game.session_id.clone();
        drop(rooms);

        self.repo
            .log_action(
                &session_id,
                "join",
                Some(&pid),
                json!({"room_id": room_id, "user_id": pid}),
            )
            .await;

        Ok((
            Some(room_id.to_string()),
            Some(pid.clone()),
            ServerEvent::Joined {
                room_id: room_id.to_string(),
                player_id: pid,
                role: Role::Citizen,
            },
        ))
    }

    async fn start_game(
        &self,
        room_id: &str,
    ) -> Result<(Option<String>, Option<String>, ServerEvent), String> {
        let mut rooms = self.rooms.lock().await;
        let room = rooms
            .get_mut(room_id)
            .ok_or_else(|| "sala no encontrada".to_string())?;
        let game = &mut room.game;

        if game.started {
            return Err("el juego ya inicio".to_string());
        }
        if game.players.len() < 8 {
            return Err("se requieren al menos 8 jugadores para esta version".to_string());
        }

        assign_roles(game)?;
        assign_pairings_and_turns(game);
        game.started = true;
        game.phase = Phase::SecretActions;
        game.phase_started_at_unix = now_unix();

        room.is_open = false;
        let session_id = game.session_id.clone();
        let count = game.players.len();
        drop(rooms);

        self.repo
            .log_action(
                &session_id,
                "start_game",
                None,
                json!({"room_id": room_id, "player_count": count}),
            )
            .await;

        Ok((
            Some(room_id.to_string()),
            None,
            ServerEvent::Info {
                room_id: Some(room_id.to_string()),
                message: "juego iniciado".to_string(),
            },
        ))
    }

    async fn terror_infect(
        &self,
        room_id: &str,
        actor_id: &str,
        target_id: &str,
    ) -> Result<(Option<String>, Option<String>, ServerEvent), String> {
        let mut rooms = self.rooms.lock().await;
        let room = rooms
            .get_mut(room_id)
            .ok_or_else(|| "sala no encontrada".to_string())?;
        let game = &mut room.game;

        if game.phase != Phase::SecretActions {
            return Err("la infeccion solo se permite en acciones secretas".to_string());
        }
        let actor = game
            .players
            .get(actor_id)
            .ok_or_else(|| "jugador invalido".to_string())?;
        if !actor.alive || actor.role != Role::Terrorist {
            return Err("solo terroristas vivos pueden infectar".to_string());
        }
        if game.terror_actions.contains(actor_id) {
            return Err("ya realizaste tu accion de ronda".to_string());
        }
        let target = game
            .players
            .get(target_id)
            .ok_or_else(|| "objetivo invalido".to_string())?;
        if !target.alive {
            return Err("no puedes infectar un jugador muerto".to_string());
        }
        if target.role == Role::Terrorist {
            return Err("terroristas no pueden infectarse entre ellos".to_string());
        }

        if let Some(target_mut) = game.players.get_mut(target_id) {
            target_mut.infected_counter = Some(2);
        }
        game.terror_actions.insert(actor_id.to_string());
        let session_id = game.session_id.clone();
        drop(rooms);

        self.repo
            .log_action(
                &session_id,
                "terror_infect",
                Some(actor_id),
                json!({"room_id": room_id, "target_id": target_id}),
            )
            .await;

        Ok((
            Some(room_id.to_string()),
            None,
            ServerEvent::Info {
                room_id: Some(room_id.to_string()),
                message: "accion registrada".to_string(),
            },
        ))
    }

    async fn investigate(
        &self,
        room_id: &str,
        actor_id: &str,
        target_id: &str,
    ) -> Result<(Option<String>, Option<String>, ServerEvent), String> {
        let mut rooms = self.rooms.lock().await;
        let room = rooms
            .get_mut(room_id)
            .ok_or_else(|| "sala no encontrada".to_string())?;
        let game = &mut room.game;

        if game.phase != Phase::SecretActions {
            return Err("la investigacion solo se permite en acciones secretas".to_string());
        }
        let actor = game
            .players
            .get(actor_id)
            .ok_or_else(|| "jugador invalido".to_string())?;
        if !actor.alive || actor.role != Role::Investigator {
            return Err("solo investigador vivo puede investigar".to_string());
        }
        if game.investigator_action.is_some() {
            return Err("investigador ya actuo esta ronda".to_string());
        }
        let target = game
            .players
            .get(target_id)
            .ok_or_else(|| "objetivo invalido".to_string())?;
        if !target.alive {
            return Err("no puedes investigar un jugador muerto".to_string());
        }

        if target.infected_counter.is_some() {
            game.cure_progress = game.cure_progress.saturating_add(1);
        }

        if game.cure_progress >= 3 {
            game.cure_unlocked = true;
            for player in game.players.values_mut() {
                player.infected_counter = None;
            }
            game.phase = Phase::Ended;
            game.winner_summary = vec!["Ciudadanos (cura desarrollada)".to_string()];
            room.is_open = false;
        }

        game.investigator_action = Some(actor_id.to_string());
        let session_id = game.session_id.clone();
        let cure_progress = game.cure_progress;
        let ended = game.phase == Phase::Ended;
        let winners = game.winner_summary.clone();
        drop(rooms);

        self.repo
            .log_action(
                &session_id,
                "investigate",
                Some(actor_id),
                json!({"room_id": room_id, "target_id": target_id, "cure_progress": cure_progress}),
            )
            .await;

        if ended {
            self.repo
                .log_action(
                    &session_id,
                    "game_end",
                    None,
                    json!({"room_id": room_id, "reason": "cure_developed", "winners": winners}),
                )
                .await;
        }

        Ok((
            Some(room_id.to_string()),
            None,
            ServerEvent::Info {
                room_id: Some(room_id.to_string()),
                message: "investigacion registrada".to_string(),
            },
        ))
    }

    async fn vote(
        &self,
        room_id: &str,
        actor_id: &str,
        target_id: &str,
    ) -> Result<(Option<String>, Option<String>, ServerEvent), String> {
        let mut rooms = self.rooms.lock().await;
        let room = rooms
            .get_mut(room_id)
            .ok_or_else(|| "sala no encontrada".to_string())?;
        let game = &mut room.game;

        if game.phase != Phase::Voting {
            return Err("solo puedes votar en fase de votacion".to_string());
        }
        if now_unix().saturating_sub(game.phase_started_at_unix) > VOTING_DURATION_SECONDS {
            return Err("la votacion ya cerro (2 minutos)".to_string());
        }

        let actor = game
            .players
            .get(actor_id)
            .ok_or_else(|| "jugador invalido".to_string())?;
        if !actor.alive {
            return Err("jugador muerto no puede votar".to_string());
        }
        let target = game
            .players
            .get(target_id)
            .ok_or_else(|| "objetivo invalido".to_string())?;
        if !target.alive {
            return Err("no puedes votar por un jugador muerto".to_string());
        }

        game.votes.insert(actor_id.to_string(), target_id.to_string());
        let session_id = game.session_id.clone();
        drop(rooms);

        self.repo
            .log_action(
                &session_id,
                "vote",
                Some(actor_id),
                json!({"room_id": room_id, "target_id": target_id}),
            )
            .await;

        Ok((
            Some(room_id.to_string()),
            None,
            ServerEvent::Info {
                room_id: Some(room_id.to_string()),
                message: "voto registrado".to_string(),
            },
        ))
    }

    async fn send_chat(
        &self,
        room_id: &str,
        actor_id: &str,
        message: &str,
    ) -> Result<(Option<String>, Option<String>, ServerEvent), String> {
        let mut rooms = self.rooms.lock().await;
        let room = rooms
            .get_mut(room_id)
            .ok_or_else(|| "sala no encontrada".to_string())?;
        let game = &mut room.game;

        let now = now_unix();
        let clean = message.trim();
        if clean.is_empty() {
            return Err("mensaje vacio".to_string());
        }
        if clean.len() > 240 {
            return Err("mensaje demasiado largo (max 240 caracteres)".to_string());
        }

        let actor = game
            .players
            .get(actor_id)
            .ok_or_else(|| "jugador invalido".to_string())?;
        if !actor.alive {
            return Err("jugador muerto no puede enviar mensajes".to_string());
        }

        let last_sent = game.chat_last_sent_at.get(actor_id).copied().unwrap_or(0);
        let elapsed = now.saturating_sub(last_sent);
        if elapsed < CHAT_COOLDOWN_SECONDS {
            return Err(format!(
                "debes esperar {} segundos para enviar otro mensaje",
                CHAT_COOLDOWN_SECONDS - elapsed
            ));
        }

        let chat_message = ChatMessage {
            id: Uuid::new_v4().to_string(),
            player_id: actor_id.to_string(),
            player_name: actor.name.clone(),
            message: clean.to_string(),
            sent_at_unix: now,
        };

        game.chat_history.push(chat_message.clone());
        if game.chat_history.len() > 300 {
            let drain_count = game.chat_history.len() - 300;
            game.chat_history.drain(0..drain_count);
        }
        game.chat_last_sent_at.insert(actor_id.to_string(), now);
        let session_id = game.session_id.clone();
        drop(rooms);

        self.repo
            .log_action(
                &session_id,
                "chat_message",
                Some(actor_id),
                json!({
                    "room_id": room_id,
                    "chat_id": chat_message.id,
                    "player_name": chat_message.player_name,
                    "message": chat_message.message,
                    "sent_at_unix": chat_message.sent_at_unix,
                }),
            )
            .await;

        Ok((
            Some(room_id.to_string()),
            None,
            ServerEvent::Info {
                room_id: Some(room_id.to_string()),
                message: "mensaje enviado".to_string(),
            },
        ))
    }

    async fn advance_phase(
        &self,
        room_id: &str,
    ) -> Result<(Option<String>, Option<String>, ServerEvent), String> {
        let mut rooms = self.rooms.lock().await;
        let room = rooms
            .get_mut(room_id)
            .ok_or_else(|| "sala no encontrada".to_string())?;
        let game = &mut room.game;

        if game.phase == Phase::Ended {
            return Err("el juego ya termino".to_string());
        }

        let previous_phase = game.phase.clone();
        let mut voted_out_player: Option<String> = None;
        let mut infection_deaths: Vec<String> = Vec::new();

        game.phase = match game.phase {
            Phase::SecretActions => {
                game.phase_started_at_unix = now_unix();
                Phase::Discussion
            }
            Phase::Discussion => {
                game.phase_started_at_unix = now_unix();
                Phase::Voting
            }
            Phase::Voting => {
                let elapsed = now_unix().saturating_sub(game.phase_started_at_unix);
                if elapsed < VOTING_DURATION_SECONDS && !all_alive_players_voted(game) {
                    return Err(format!(
                        "votacion en curso, faltan {} segundos",
                        VOTING_DURATION_SECONDS - elapsed
                    ));
                }
                voted_out_player = resolve_voting(game);
                game.phase_started_at_unix = now_unix();
                Phase::Resolution
            }
            Phase::Resolution => {
                infection_deaths = resolve_infections(game);
                game.round += 1;
                game.terror_actions.clear();
                game.votes.clear();
                game.investigator_action = None;
                if !game.turn_order.is_empty() {
                    game.current_turn_index = (game.current_turn_index + 1) % game.turn_order.len();
                }
                game.phase_started_at_unix = now_unix();
                Phase::SecretActions
            }
            Phase::Ended => Phase::Ended,
        };

        evaluate_win_conditions(game);
        if game.phase == Phase::Ended {
            room.is_open = false;
        }

        let session_id = game.session_id.clone();
        let round = game.round;
        let phase_now = format!("{:?}", game.phase);
        let winners = game.winner_summary.clone();
        drop(rooms);

        if let Some(player_id) = voted_out_player {
            self.repo
                .log_action(
                    &session_id,
                    "voting_result",
                    None,
                    json!({"room_id": room_id, "round": round, "eliminated_player_id": player_id}),
                )
                .await;
        }

        if !infection_deaths.is_empty() {
            self.repo
                .log_action(
                    &session_id,
                    "infection_resolution",
                    None,
                    json!({"room_id": room_id, "round": round, "deaths": infection_deaths}),
                )
                .await;
        }

        self.repo
            .log_action(
                &session_id,
                "advance_phase",
                None,
                json!({
                    "room_id": room_id,
                    "round": round,
                    "from_phase": format!("{:?}", previous_phase),
                    "to_phase": phase_now
                }),
            )
            .await;

        if phase_now == "Ended" {
            self.repo
                .log_action(
                    &session_id,
                    "game_end",
                    None,
                    json!({"room_id": room_id, "reason": "win_condition", "winners": winners}),
                )
                .await;
        }

        Ok((
            Some(room_id.to_string()),
            None,
            ServerEvent::Info {
                room_id: Some(room_id.to_string()),
                message: format!("fase actual: {}", phase_now),
            },
        ))
    }
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
