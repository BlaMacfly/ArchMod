//! Reproduit exactement ce que l'interface interroge : l'état de toute la
//! bibliothèque, en un seul balayage des processus.
fn main() {
    let games = archmod_lib::scan_games_for_diagnostics().expect("scan");
    println!("{} jeu(x)", games.len());
    let mut aucun = true;
    for game in &games {
        let state = archmod_lib::run_state_for_diagnostics(game);
        if state.running {
            aucun = false;
            println!(
                "  EN COURS  {} (AppID {}) — pids {:?}\n            {:?}\n            {}",
                game.name,
                game.app_id,
                state.pids,
                state.matched_by,
                game.install_path.display()
            );
        }
    }
    if aucun {
        println!("  aucun jeu détecté comme lancé");
    }
}
