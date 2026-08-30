fn main() {
    tauri_build::build();

    // Un `cargo build --release` lancé sans le CLI Tauri produit un binaire
    // resté en mode développement : il cherche le serveur Vite et n'affiche
    // qu'une fenêtre vide. Le piège est silencieux, on le rend bruyant.
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    let via_tauri = std::env::var_os("TAURI_ENV_PLATFORM").is_some();
    if release && !via_tauri {
        println!(
            "cargo:warning=Binaire release construit hors du CLI Tauri : il restera en \
             mode développement et n'affichera qu'une fenêtre vide. Pour l'application, \
             utilise « npm run tauri build -- --no-bundle »."
        );
    }
}
