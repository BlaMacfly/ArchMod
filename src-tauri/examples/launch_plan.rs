//! Montre, pour chaque jeu de la bibliothèque, la commande exacte que ArchMod
//! transmettrait à Steam pour le lancer — sans rien lancer.
//!
//! Utile quand deux clients Steam cohabitent (natif et Flatpak) : c'est là que
//! le choix du mauvais client se verrait, en ouvrant le magasin au lieu du jeu.
fn main() {
    let games = archmod_lib::scan_games_for_diagnostics().expect("scan");
    println!("{} jeu(x)", games.len());
    for game in &games {
        match archmod_lib::launch_plan_for_diagnostics(game) {
            Ok(plan) => println!(
                "  {:<40} {:?}\n      {}",
                game.name,
                plan.client,
                plan.command_line()
            ),
            Err(err) => println!("  {:<40} indisponible : {err}", game.name),
        }
    }
}
