//! ArchMod — gestionnaire de trainers natif pour les jeux Steam sous Proton.
//!
//! Ce module expose l'API `tauri::command` consommée par le frontend React.
//! Toute la logique métier vit dans les modules dédiés ; on se contente ici de
//! l'orchestration, de l'état partagé et de la conversion des erreurs.

mod banners;
pub mod cheat_table;
pub mod engine;
mod error;
pub mod hook;
mod injector;
/// Exposé pour les outils de diagnostic (`cargo run --example ...`).
pub mod memory;
pub mod pointer;
mod prefix;
pub mod profile;
mod proton;
pub mod scanner;
mod steam_scanner;
pub mod trainer;
mod vault;
mod vdf;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

use banners::BannerKind;
use error::{ErrorReport, Result};
use injector::{Dependencies, LaunchOutcome, LaunchPlan, RunningTrainer, TrainerRegistry};
use pointer::{PointerPath, PointerScanReport, ScanOptions};
use prefix::{Component, PrefixReport};
use profile::{BuildMatch, Profile};
use scanner::{CandidateView, Filter, ScanReport, Scanner};
use steam_scanner::SteamGame;
use trainer::{ActivationReport, OptionStatus, Runtimes};
use vault::{Settings, TrainerEntry, Vault};

/// État partagé entre toutes les commandes.
struct AppState {
    vault: Mutex<Vault>,
    /// Erreur de lecture de `config.json` : l'interface propose une réparation
    /// au lieu d'écraser silencieusement le fichier.
    config_error: Mutex<Option<ErrorReport>>,
    registry: Arc<TrainerRegistry>,
    /// Détection des dépendances système, mise en cache : elle lance des
    /// sous-processus (`flatpak info`) qu'on ne veut pas répéter à chaque clic.
    dependencies: Mutex<Option<Dependencies>>,
    games: Mutex<Vec<SteamGame>>,
    /// Profils de trainer actifs, un par jeu.
    runtimes: Runtimes,
    /// Recherches de valeurs en cours, avec le gel de leurs résultats.
    scanners: Mutex<HashMap<u32, ScanState>>,
    steam_roots: Mutex<Vec<PathBuf>>,
}

impl AppState {
    fn new() -> Self {
        let (vault, config_error) = match vault::load() {
            Ok(vault) => (vault, None),
            Err(err) => {
                eprintln!("[archmod] configuration illisible : {err}");
                (Vault::default(), Some(ErrorReport::from(&err)))
            }
        };

        Self {
            vault: Mutex::new(vault),
            config_error: Mutex::new(config_error),
            registry: Arc::new(TrainerRegistry::default()),
            dependencies: Mutex::new(None),
            games: Mutex::new(Vec::new()),
            runtimes: Runtimes::default(),
            scanners: Mutex::new(HashMap::new()),
            steam_roots: Mutex::new(Vec::new()),
        }
    }

    /// Persiste le coffre ; l'appelant tient déjà le verrou.
    fn persist(vault: &Vault) -> Result<()> {
        vault::save(vault)
    }

    /// Dépendances système, recalculées uniquement sur demande explicite.
    async fn deps(&self, refresh: bool) -> Dependencies {
        let mut cached = self.dependencies.lock().await;
        if refresh || cached.is_none() {
            *cached = Some(injector::check_dependencies().await);
        }
        cached.clone().unwrap_or_else(|| Dependencies {
            protontricks: None,
            protontricks_flatpak: false,
            wine: None,
            ready: false,
            install_command: String::new(),
        })
    }

    async fn game(&self, app_id: u32) -> Result<SteamGame> {
        if let Some(game) = self
            .games
            .lock()
            .await
            .iter()
            .find(|g| g.app_id == app_id)
            .cloned()
        {
            return Ok(game);
        }
        steam_scanner::find_game(app_id)
    }
}

// ---------------------------------------------------------------------------
// Types exposés au frontend
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GameView {
    #[serde(flatten)]
    game: SteamGame,
    trainer: Option<TrainerEntry>,
    /// Le trainer est référencé mais le fichier a disparu du disque.
    trainer_missing: bool,
    running: bool,
    trainer_running: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LibrarySnapshot {
    games: Vec<GameView>,
    steam_roots: Vec<PathBuf>,
    /// Erreur de configuration éventuelle, à afficher en bandeau.
    config_error: Option<ErrorReport>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusUpdate {
    app_id: u32,
    running: bool,
    pids: Vec<u32>,
    matched_by: Option<String>,
    trainer_running: bool,
}

// ---------------------------------------------------------------------------
// Commandes
// ---------------------------------------------------------------------------

#[tauri::command]
async fn scan_library(state: State<'_, AppState>) -> Result<LibrarySnapshot> {
    let started = std::time::Instant::now();
    let games = steam_scanner::scan_games()?;
    let roots = steam_scanner::steam_roots()?;
    eprintln!(
        "[archmod] scan : {} jeu(x) dans {} racine(s) en {} ms",
        games.len(),
        roots.len(),
        started.elapsed().as_millis()
    );
    for root in &roots {
        eprintln!("[archmod]   racine Steam : {}", root.display());
    }
    let running = injector::scan_running(&games);
    let launched = state.registry.snapshot().await;

    let views = {
        let vault = state.vault.lock().await;
        games
            .iter()
            .map(|game| {
                let trainer = vault.trainers.get(&game.app_id).cloned();
                let trainer_missing = trainer.as_ref().is_some_and(|entry| !entry.path.is_file());
                GameView {
                    game: game.clone(),
                    trainer,
                    trainer_missing,
                    running: running.get(&game.app_id).is_some_and(|s| s.running),
                    trainer_running: launched.iter().any(|r| r.app_id == game.app_id),
                }
            })
            .collect()
    };

    *state.games.lock().await = games;
    *state.steam_roots.lock().await = roots.clone();

    Ok(LibrarySnapshot {
        games: views,
        steam_roots: roots,
        config_error: state.config_error.lock().await.clone(),
    })
}

/// Statut d'exécution de toute la bibliothèque (interrogé périodiquement).
#[tauri::command]
async fn refresh_status(state: State<'_, AppState>) -> Result<Vec<StatusUpdate>> {
    let games = state.games.lock().await.clone();
    let games = if games.is_empty() {
        steam_scanner::scan_games()?
    } else {
        games
    };

    let running = injector::scan_running(&games);
    let launched = state.registry.snapshot().await;

    Ok(games
        .iter()
        .map(|game| {
            let state = running.get(&game.app_id);
            StatusUpdate {
                app_id: game.app_id,
                running: state.is_some_and(|s| s.running),
                pids: state.map(|s| s.pids.clone()).unwrap_or_default(),
                matched_by: state.and_then(|s| s.matched_by.clone()),
                trainer_running: launched.iter().any(|r| r.app_id == game.app_id),
            }
        })
        .collect())
}

#[tauri::command]
async fn get_banner(
    state: State<'_, AppState>,
    app_id: u32,
    kind: BannerKind,
) -> Result<Option<PathBuf>> {
    let roots = {
        let cached = state.steam_roots.lock().await.clone();
        if cached.is_empty() {
            steam_scanner::steam_roots()?
        } else {
            cached
        }
    };
    let allow_network = state.vault.lock().await.settings.allow_network_artwork;
    banners::ensure_banner(&roots, app_id, kind, allow_network).await
}

#[tauri::command]
async fn clear_banner_cache() -> Result<u64> {
    banners::clear_cache()
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<Settings> {
    Ok(state.vault.lock().await.settings.clone())
}

#[tauri::command]
async fn update_settings(state: State<'_, AppState>, settings: Settings) -> Result<Settings> {
    let mut vault = state.vault.lock().await;
    vault.settings = settings;
    AppState::persist(&vault)?;
    Ok(vault.settings.clone())
}

#[tauri::command]
async fn set_trainer(
    state: State<'_, AppState>,
    app_id: u32,
    path: PathBuf,
) -> Result<TrainerEntry> {
    let mut vault = state.vault.lock().await;
    let entry = vault.set_trainer(app_id, &path)?;
    AppState::persist(&vault)?;
    eprintln!(
        "[archmod] trainer associé au jeu {app_id} : {}",
        entry.path.display()
    );
    Ok(entry)
}

#[tauri::command]
async fn remove_trainer(state: State<'_, AppState>, app_id: u32) -> Result<()> {
    let mut vault = state.vault.lock().await;
    vault.remove_trainer(app_id);
    AppState::persist(&vault)
}

/// Met de côté une configuration corrompue et repart d'une base saine.
#[tauri::command]
async fn repair_config(state: State<'_, AppState>) -> Result<Option<PathBuf>> {
    let backup = vault::repair()?;
    *state.vault.lock().await = Vault::default();
    *state.config_error.lock().await = None;
    Ok(backup)
}

/// Commande qui serait exécutée, sans rien lancer (affichée dans l'interface).
#[tauri::command]
async fn preview_command(state: State<'_, AppState>, app_id: u32) -> Result<LaunchPlan> {
    let game = state.game(app_id).await?;
    let (trainer, backend) = {
        let vault = state.vault.lock().await;
        (vault.trainer(app_id)?.path.clone(), vault.settings.backend)
    };
    let deps = state.deps(false).await;
    injector::build_plan(&game, &trainer, backend, &deps)
}

#[tauri::command]
async fn launch_trainer(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: u32,
    force: bool,
) -> Result<LaunchOutcome> {
    let game = state.game(app_id).await?;
    let (trainer, backend, warn) = {
        let vault = state.vault.lock().await;
        let entry = vault.trainer(app_id)?;
        (
            entry.path.clone(),
            vault.settings.backend,
            vault.settings.warn_if_game_not_running,
        )
    };

    let deps = state.deps(false).await;
    let outcome = injector::launch(
        app,
        Arc::clone(&state.registry),
        game,
        trainer,
        backend,
        &deps,
        warn,
        force,
    )
    .await?;

    if outcome.started {
        let mut vault = state.vault.lock().await;
        vault.mark_launched(app_id);
        // Un échec d'écriture des statistiques ne doit pas masquer le succès.
        if let Err(err) = AppState::persist(&vault) {
            eprintln!("[archmod] statistiques non enregistrées : {err}");
        }
    }
    Ok(outcome)
}

#[tauri::command]
async fn stop_trainer(app: AppHandle, state: State<'_, AppState>, app_id: u32) -> Result<bool> {
    injector::stop(&app, &state.registry, app_id).await
}

#[tauri::command]
async fn running_trainers(state: State<'_, AppState>) -> Result<Vec<RunningTrainer>> {
    Ok(state.registry.snapshot().await)
}

/// Diagnostic du préfixe : ce qui manque pour qu'un trainer démarre.
#[tauri::command]
async fn inspect_prefix(state: State<'_, AppState>, app_id: u32) -> Result<PrefixReport> {
    let game = state.game(app_id).await?;
    prefix::inspect(&game)
}

/// Installe un composant manquant (le plus souvent .NET) dans le préfixe.
#[tauri::command]
async fn install_component(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: u32,
    component: Component,
) -> Result<bool> {
    let game = state.game(app_id).await?;
    let deps = state.deps(false).await;
    injector::install_component(&app, &game, component, &deps).await
}

#[tauri::command]
async fn check_dependencies(state: State<'_, AppState>) -> Result<Dependencies> {
    let deps = state.deps(true).await;
    eprintln!(
        "[archmod] dépendances : protontricks={} flatpak={} wine={}",
        deps.protontricks
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "absent".into()),
        deps.protontricks_flatpak,
        deps.wine
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "absent".into())
    );
    Ok(deps)
}

/// Une recherche en cours et les valeurs qu'elle maintient figées.
struct ScanState {
    scanner: Scanner,
    freezer: engine::Freezer,
}

/// Démarre une recherche : elle remplace celle qui était éventuellement en cours.
#[tauri::command]
async fn scan_start(
    state: State<'_, AppState>,
    app_id: u32,
    value_type: cheat_table::ValueType,
    filter: Filter,
) -> Result<ScanReport> {
    let game = state.game(app_id).await?;
    let pid = injector::game_process(&game).ok_or(error::TuxError::GameNotRunning {
        name: game.name.clone(),
    })?;

    let mut scanner = Scanner::new(pid, value_type);
    let report = scanner.scan(filter)?;

    let mut scanners = state.scanners.lock().await;
    if let Some(mut previous) = scanners.remove(&app_id) {
        previous.freezer.stop().await;
    }
    scanners.insert(
        app_id,
        ScanState {
            scanner,
            freezer: engine::Freezer::new(pid),
        },
    );
    Ok(report)
}

/// Affine la recherche en cours.
#[tauri::command]
async fn scan_next(state: State<'_, AppState>, app_id: u32, filter: Filter) -> Result<ScanReport> {
    let mut scanners = state.scanners.lock().await;
    let entry = scanners
        .get_mut(&app_id)
        .ok_or_else(|| error::TuxError::Internal("aucune recherche en cours".into()))?;
    entry.scanner.scan(filter)
}

/// Relit les valeurs affichées, sans filtrer.
#[tauri::command]
async fn scan_refresh(state: State<'_, AppState>, app_id: u32) -> Result<Vec<CandidateView>> {
    let mut scanners = state.scanners.lock().await;
    match scanners.get_mut(&app_id) {
        Some(entry) => entry.scanner.refresh(),
        None => Ok(Vec::new()),
    }
}

#[tauri::command]
async fn scan_reset(state: State<'_, AppState>, app_id: u32) -> Result<bool> {
    let mut scanners = state.scanners.lock().await;
    match scanners.remove(&app_id) {
        Some(mut entry) => {
            entry.freezer.stop().await;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Écrit une valeur à une adresse trouvée — l'épreuve décisive : si le jeu
/// réagit, c'est la bonne adresse.
#[tauri::command]
async fn scan_write(
    state: State<'_, AppState>,
    app_id: u32,
    address: u64,
    value: engine::Value,
) -> Result<()> {
    let game = state.game(app_id).await?;
    let pid = injector::game_process(&game).ok_or(error::TuxError::GameNotRunning {
        name: game.name.clone(),
    })?;
    memory::write(pid, address, &value.to_bytes())
}

/// Fige une adresse trouvée, sans passer par un profil.
#[tauri::command]
async fn scan_freeze(
    state: State<'_, AppState>,
    app_id: u32,
    address: u64,
    value: Option<engine::Value>,
) -> Result<bool> {
    let mut scanners = state.scanners.lock().await;
    let entry = scanners
        .get_mut(&app_id)
        .ok_or_else(|| error::TuxError::Internal("aucune recherche en cours".into()))?;

    let key = format!("{address:#x}");
    match value {
        Some(value) => {
            entry.freezer.freeze(key, address, value).await;
            Ok(true)
        }
        None => Ok(entry.freezer.unfreeze(&key).await),
    }
}

/// Cherche par quel chemin de pointeurs une adresse volatile est atteinte.
///
/// C'est l'opération qui transforme une trouvaille éphémère en profil : sans
/// elle, une adresse du tas ne vaut que pour la session en cours.
#[tauri::command]
async fn pointer_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: u32,
    address: u64,
    options: Option<ScanOptions>,
) -> Result<PointerScanReport> {
    let game = state.game(app_id).await?;
    let pid = injector::game_process(&game).ok_or(error::TuxError::GameNotRunning {
        name: game.name.clone(),
    })?;
    let options = options.unwrap_or_default();

    injector::log(
        &app,
        Some(app_id),
        injector::LogLevel::Info,
        format!(
            "Recherche de pointeurs vers {address:#x} (profondeur {}, décalage max {:#x})…",
            options.max_depth, options.max_offset
        ),
    );

    // Le balayage complet de la mémoire dure plusieurs secondes : on le confie
    // à un fil dédié pour ne pas figer l'interface.
    let report = tokio::task::spawn_blocking(move || pointer::scan(pid, address, options))
        .await
        .map_err(|error| error::TuxError::Internal(error.to_string()))??;

    injector::log(
        &app,
        Some(app_id),
        if report.paths.is_empty() {
            injector::LogLevel::Warn
        } else {
            injector::LogLevel::Success
        },
        format!(
            "{} chemin(s) trouvé(s) parmi {} pointeurs, en {} ms.",
            report.paths.len(),
            report.pointers,
            report.elapsed_ms
        ),
    );
    Ok(report)
}

/// Rejoue un chemin et retourne l'adresse atteinte — la vérification qui
/// distingue un chemin fiable d'une coïncidence.
#[tauri::command]
async fn pointer_verify(state: State<'_, AppState>, app_id: u32, path: PointerPath) -> Result<u64> {
    let game = state.game(app_id).await?;
    let pid = injector::game_process(&game).ok_or(error::TuxError::GameNotRunning {
        name: game.name.clone(),
    })?;
    pointer::resolve(pid, &path)
}

/// Un profil disponible pour un jeu, avec sa pertinence pour le build installé.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileEntry {
    profile: Profile,
    build_match: BuildMatch,
}

/// Profils installés localement pour ce jeu, les plus pertinents en premier.
#[tauri::command]
async fn profiles_for_game(state: State<'_, AppState>, app_id: u32) -> Result<Vec<ProfileEntry>> {
    let game = state.game(app_id).await.ok();
    let build = game.as_ref().and_then(|game| game.build_id.clone());

    Ok(profile::for_game(app_id, build.as_deref())?
        .into_iter()
        .map(|profile| ProfileEntry {
            build_match: profile.matches_build(build.as_deref()),
            profile,
        })
        .collect())
}

/// Charge un profil sur le jeu en cours et résout toutes ses adresses.
#[tauri::command]
async fn activate_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: u32,
    profile: Profile,
) -> Result<ActivationReport> {
    let game = state.game(app_id).await?;
    let report = state.runtimes.activate(&game, profile).await?;

    injector::log(
        &app,
        Some(app_id),
        if report.failed == 0 {
            injector::LogLevel::Success
        } else {
            injector::LogLevel::Warn
        },
        format!(
            "Profil chargé sur « {} » (PID {}) : {} option(s) résolue(s), {} en échec.",
            game.name, report.pid, report.resolved, report.failed
        ),
    );
    Ok(report)
}

#[tauri::command]
async fn trainer_report(
    state: State<'_, AppState>,
    app_id: u32,
) -> Result<Option<ActivationReport>> {
    Ok(state.runtimes.report(app_id).await)
}

/// Active une option : écrit sa valeur, et la gèle si son contrôle l'exige.
#[tauri::command]
async fn set_option(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: u32,
    option_id: String,
    value: Option<engine::Value>,
) -> Result<OptionStatus> {
    let status = state.runtimes.set(app_id, &option_id, value).await?;
    injector::log(
        &app,
        Some(app_id),
        injector::LogLevel::Success,
        format!(
            "Option « {option_id} » activée en {:#x}.",
            status.address.unwrap_or_default()
        ),
    );
    Ok(status)
}

#[tauri::command]
async fn clear_option(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: u32,
    option_id: String,
) -> Result<bool> {
    let cleared = state.runtimes.clear(app_id, &option_id).await?;
    injector::log(
        &app,
        Some(app_id),
        injector::LogLevel::Info,
        format!("Option « {option_id} » désactivée."),
    );
    Ok(cleared)
}

/// Relit les valeurs de toutes les options résolues.
#[tauri::command]
async fn refresh_values(state: State<'_, AppState>, app_id: u32) -> Result<ActivationReport> {
    state.runtimes.refresh(app_id).await
}

#[tauri::command]
async fn deactivate_profile(state: State<'_, AppState>, app_id: u32) -> Result<bool> {
    Ok(state.runtimes.deactivate(app_id).await)
}

/// Essai en direct d'une recette d'adresse — l'outil central de l'atelier.
#[tauri::command]
async fn probe_recipe(
    state: State<'_, AppState>,
    app_id: u32,
    recipe: profile::AddressRecipe,
    value_type: cheat_table::ValueType,
) -> Result<OptionStatus> {
    let game = state.game(app_id).await?;
    state.runtimes.probe(&game, &recipe, &value_type).await
}

/// Convertit une table Cheat Engine en profil, sans rien enregistrer.
///
/// L'auteur voit ce qui a été repris et ce qui a été écarté, puis décide.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheatTableImport {
    profile: Profile,
    skipped: Vec<profile::Skipped>,
}

#[tauri::command]
async fn import_cheat_table(
    state: State<'_, AppState>,
    app_id: u32,
    path: PathBuf,
) -> Result<CheatTableImport> {
    let game = state.game(app_id).await?;
    let raw = std::fs::read(&path).map_err(|source| error::TuxError::io(&path, source))?;
    let table = cheat_table::CheatTable::parse(&String::from_utf8_lossy(&raw))?;

    let (profile, skipped) =
        profile::from_cheat_table(&table, app_id, &game.name, game.build_id.clone());
    eprintln!(
        "[archmod] table importée : {} option(s) reprise(s), {} écartée(s)",
        profile.options.len(),
        skipped.len()
    );
    Ok(CheatTableImport { profile, skipped })
}

#[tauri::command]
async fn save_profile(profile: Profile) -> Result<PathBuf> {
    profile::save(&profile)
}

#[tauri::command]
async fn import_profile(path: PathBuf) -> Result<Profile> {
    let imported = profile::load_from(&path)?;
    profile::save(&imported)?;
    Ok(imported)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppPaths {
    config: PathBuf,
    banner_cache: PathBuf,
}

#[tauri::command]
async fn app_paths() -> Result<AppPaths> {
    Ok(AppPaths {
        config: vault::config_path()?,
        banner_cache: banners::cache_dir()?,
    })
}

/// Accès aux détections internes pour les outils de diagnostic.
pub fn find_game_for_diagnostics(app_id: u32) -> Result<SteamGame> {
    steam_scanner::find_game(app_id)
}

pub fn scan_games_for_diagnostics() -> Result<Vec<SteamGame>> {
    steam_scanner::scan_games()
}

pub fn run_state_for_diagnostics(game: &SteamGame) -> injector::GameRunState {
    injector::game_run_state(game)
}

pub fn game_process_for_diagnostics(game: &SteamGame) -> Option<u32> {
    injector::game_process(game)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            eprintln!(
                "[archmod] ArchMod {} — configuration : {}",
                env!("CARGO_PKG_VERSION"),
                vault::config_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|e| e.to_string())
            );
            app.manage(AppState::new());
            // Autorise explicitement le cache de visuels pour le protocole
            // `asset:`, sans dépendre de l'expansion de $HOME dans
            // tauri.conf.json. Un échec ici ne prive que des jaquettes.
            match banners::cache_dir() {
                Ok(dir) => {
                    if let Err(err) = app.asset_protocol_scope().allow_directory(&dir, true) {
                        eprintln!("[archmod] portée asset non étendue : {err}");
                    }
                }
                Err(err) => eprintln!("[archmod] cache de visuels indisponible : {err}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan_library,
            refresh_status,
            get_banner,
            clear_banner_cache,
            get_settings,
            update_settings,
            set_trainer,
            remove_trainer,
            repair_config,
            preview_command,
            launch_trainer,
            stop_trainer,
            running_trainers,
            check_dependencies,
            inspect_prefix,
            install_component,
            profiles_for_game,
            activate_profile,
            trainer_report,
            set_option,
            clear_option,
            refresh_values,
            deactivate_profile,
            probe_recipe,
            pointer_scan,
            pointer_verify,
            scan_start,
            scan_next,
            scan_refresh,
            scan_reset,
            scan_write,
            scan_freeze,
            import_cheat_table,
            save_profile,
            import_profile,
            app_paths,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au démarrage de ArchMod");
}
