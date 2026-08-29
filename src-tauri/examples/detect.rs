//! Vérifie la détection du processus d'un jeu.
fn main() {
    let app_id: u32 = std::env::args().nth(1).unwrap().parse().unwrap();
    let game = archmod_lib::find_game_for_diagnostics(app_id).expect("jeu");
    println!(
        "jeu        : {} ({})",
        game.name,
        game.install_path.display()
    );
    println!("build      : {:?}", game.build_id);
    let state = archmod_lib::run_state_for_diagnostics(&game);
    println!("en cours   : {} {:?}", state.running, state.matched_by);
    println!(
        "pid du jeu : {:?}",
        archmod_lib::game_process_for_diagnostics(&game)
    );
}
