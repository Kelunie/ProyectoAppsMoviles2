use crate::models::{GameState, Phase, Role};

pub fn evaluate_win_conditions(game: &mut GameState) {
    if game.phase == Phase::Ended {
        return;
    }

    let mut terrorists_alive = 0;
    let mut citizens_alive = 0;
    let mut fanatic_dead = false;

    for p in game.players.values() {
        if p.role == Role::Fanatic && !p.alive {
            fanatic_dead = true;
        }

        if p.alive {
            match p.role {
                Role::Terrorist => terrorists_alive += 1,
                Role::Citizen | Role::Investigator => citizens_alive += 1,
                Role::Fanatic => {}
            }
        }
    }

    let all_terrorists_dead = game
        .players
        .values()
        .filter(|p| p.role == Role::Terrorist)
        .all(|p| !p.alive);

    if all_terrorists_dead {
        game.phase = Phase::Ended;
        game.winner_summary = vec!["Ciudadanos (terroristas eliminados)".to_string()];
        if fanatic_dead {
            game.winner_summary
                .push("Fanatico (murio durante el juego)".to_string());
        }
        return;
    }

    if terrorists_alive >= citizens_alive && terrorists_alive > 0 {
        game.phase = Phase::Ended;
        game.winner_summary = vec!["Terroristas".to_string()];
        if fanatic_dead {
            game.winner_summary
                .push("Fanatico (murio durante el juego)".to_string());
        }
        return;
    }

    if fanatic_dead && !game.winner_summary.iter().any(|w| w.contains("Fanatico")) {
        game.winner_summary
            .push("Fanatico (objetivo cumplido)".to_string());
    }
}
