use crate::models::{GameState, Role};
use rand::seq::SliceRandom;
use std::collections::HashMap;

const MIN_PLAYERS_TO_ASSIGN_ROLES: usize = 5;

pub fn assign_pairings_and_turns(game: &mut GameState) {
    let mut ids: Vec<String> = game.players.keys().cloned().collect();
    ids.shuffle(&mut rand::thread_rng());

    game.turn_order = ids.clone();
    game.current_turn_index = 0;
    game.pairings = ids.chunks(2).map(|chunk| chunk.to_vec()).collect();
}

pub fn assign_roles(game: &mut GameState) -> Result<(), String> {
    let mut ids: Vec<String> = game.players.keys().cloned().collect();
    ids.shuffle(&mut rand::thread_rng());

    if ids.len() < MIN_PLAYERS_TO_ASSIGN_ROLES {
        return Err("jugadores insuficientes".to_string());
    }

    for (i, id) in ids.iter().enumerate() {
        let role = match i {
            0 | 1 => Role::Terrorist,
            2 => Role::Investigator,
            3 => Role::Fanatic,
            _ => Role::Citizen,
        };

        if let Some(player) = game.players.get_mut(id) {
            player.role = role;
        }
    }

    Ok(())
}

pub fn resolve_voting(game: &mut GameState) -> Option<String> {
    let mut count_by_target: HashMap<String, u32> = HashMap::new();

    for target in game.votes.values() {
        *count_by_target.entry(target.clone()).or_insert(0) += 1;
    }

    let eliminated = count_by_target
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(id, _)| id.clone());

    if let Some(id) = eliminated {
        if let Some(player) = game.players.get_mut(&id) {
            player.alive = false;
            game.eliminated_order.push(id);
            return Some(player.id.clone());
        }
    }

    None
}

pub fn resolve_infections(game: &mut GameState) -> Vec<String> {
    let mut deaths = Vec::new();

    for player in game.players.values_mut() {
        if let Some(counter) = player.infected_counter {
            if counter <= 1 {
                player.alive = false;
                player.infected_counter = None;
                game.eliminated_order.push(player.id.clone());
                deaths.push(player.id.clone());
            } else {
                player.infected_counter = Some(counter - 1);
            }
        }
    }

    deaths
}

pub fn all_alive_players_voted(game: &GameState) -> bool {
    let alive_count = game.players.values().filter(|p| p.alive).count();
    game.votes.len() >= alive_count
}
