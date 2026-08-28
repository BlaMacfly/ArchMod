//! Injection du trainer dans le préfixe Wine/Proton du jeu.
//!
//! Trois backends, essayés dans cet ordre en mode automatique :
//!   1. `protontricks -c "wine '<trainer>'" <AppID>` (méthode de référence) ;
//!   2. le script `proton run` de la version Proton du jeu (aucune dépendance
//!      supplémentaire, `protontricks` peut être absent) ;
//!   3. le `wine` système pointé sur `compatdata/<AppID>/pfx` (dernier recours,
//!      susceptible de faire évoluer le préfixe : on prévient dans la console).
//!
//! La sortie du processus est diffusée ligne par ligne au frontend via des
//! évènements Tauri, jamais bufferisée jusqu'à la fin.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;

use crate::error::{Result, TuxError};
use crate::proton::{self, which};
use crate::steam_scanner::SteamGame;
use crate::vault::Backend;

pub const EVENT_LOG: &str = "archmod://log";
pub const EVENT_TRAINER_STATE: &str = "archmod://trainer-state";

const FLATPAK_PROTONTRICKS: &str = "com.github.Matoking.protontricks";

// ---------------------------------------------------------------------------
// Journal
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Command,
    Stdout,
    Stderr,
    Warn,
    Error,
    Success,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub timestamp: i64,
    pub level: LogLevel,
    pub app_id: Option<u32>,
    pub message: String,
}

pub fn log(app: &AppHandle, app_id: Option<u32>, level: LogLevel, message: impl Into<String>) {
    let line = LogLine {
        timestamp: chrono::Utc::now().timestamp_millis(),
        level,
        app_id,
        message: message.into(),
    };
    // Un échec d'émission (fenêtre fermée) ne doit pas interrompre le lancement.
    if let Err(err) = app.emit(EVENT_LOG, &line) {
        eprintln!("[archmod] émission du log impossible : {err}");
    }
}

// ---------------------------------------------------------------------------
// Dépendances système
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependencies {
    pub protontricks: Option<PathBuf>,
    pub protontricks_flatpak: bool,
    pub wine: Option<PathBuf>,
    /// Au moins un backend est utilisable.
    pub ready: bool,
    pub install_command: String,
}

pub async fn check_dependencies() -> Dependencies {
    let protontricks = which("protontricks");
    let wine = which("wine");
    let protontricks_flatpak =
        protontricks.is_none() && flatpak_app_installed(FLATPAK_PROTONTRICKS).await;

    Dependencies {
        ready: protontricks.is_some() || protontricks_flatpak || wine.is_some(),
        protontricks,
        protontricks_flatpak,
        wine,
        install_command: "sudo pacman -S --needed protontricks wine".into(),
    }
}

async fn flatpak_app_installed(app_id: &str) -> bool {
    if which("flatpak").is_none() {
        return false;
    }
    let probe = tokio::process::Command::new("flatpak")
        .args(["info", app_id])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    matches!(
        tokio::time::timeout(std::time::Duration::from_secs(5), probe).await,
        Ok(Ok(status)) if status.success()
    )
}

// ---------------------------------------------------------------------------
// Détection du jeu en cours d'exécution
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRunState {
    pub running: bool,
    pub pids: Vec<u32>,
    /// Heuristique ayant permis la détection (affichée en infobulle).
    pub matched_by: Option<String>,
}

/// Instantané des processus, rafraîchi une seule fois pour toute la bibliothèque.
fn refreshed_system() -> System {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_exe(UpdateKind::Always),
    );
    system
}

/// Détecte si le jeu tourne, via le marqueur `AppId=<id>` posé par le `reaper`
/// de Steam, puis via le chemin d'installation des processus.
fn match_game(system: &System, game: &SteamGame) -> GameRunState {
    let app_marker = format!("AppId={}", game.app_id);
    let install_path = game.install_path.to_string_lossy().to_string();

    let mut pids = Vec::new();
    let mut matched_by = None;

    for (pid, process) in system.processes() {
        let cmd_matches = process.cmd().iter().any(|arg| {
            let arg = arg.to_string_lossy();
            arg.contains(&app_marker) || arg.contains(&install_path)
        });
        let exe_matches = process
            .exe()
            .is_some_and(|exe| exe.starts_with(&game.install_path));

        if cmd_matches || exe_matches {
            pids.push(pid.as_u32());
            if matched_by.is_none() {
                matched_by = Some(if exe_matches {
                    format!("processus dans {install_path}")
                } else {
                    format!("marqueur Steam {app_marker}")
                });
            }
        }
    }

    pids.sort_unstable();
    GameRunState {
        running: !pids.is_empty(),
        pids,
        matched_by,
    }
}

pub fn game_run_state(game: &SteamGame) -> GameRunState {
    match_game(&refreshed_system(), game)
}

/// État d'exécution de toute la bibliothèque en un seul balayage des processus.
pub fn scan_running(games: &[SteamGame]) -> HashMap<u32, GameRunState> {
    let system = refreshed_system();
    games
        .iter()
        .map(|game| (game.app_id, match_game(&system, game)))
        .collect()
}

// ---------------------------------------------------------------------------
// Construction de la commande
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlan {
    pub backend: Backend,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub working_dir: Option<PathBuf>,
    /// Explication lisible, affichée dans la console avant le lancement.
    pub note: String,
}

impl LaunchPlan {
    /// Ligne de commande reconstituée, quotée pour être copiable dans un shell.
    pub fn command_line(&self) -> String {
        let env: String = self
            .env
            .iter()
            .map(|(k, v)| format!("{k}={} ", shell_quote(v)))
            .collect();
        let args: Vec<String> = self.args.iter().map(|a| shell_quote(a)).collect();
        format!("{env}{} {}", shell_quote(&self.program), args.join(" "))
    }
}

/// Quote une valeur pour un shell POSIX (`'` fermé, échappé, rouvert).
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn protontricks_plan(game: &SteamGame, trainer: &Path, flatpak: bool) -> LaunchPlan {
    let inner = format!("wine {}", shell_quote(&trainer.to_string_lossy()));
    let (program, mut args) = if flatpak {
        (
            "flatpak".to_string(),
            vec!["run".to_string(), FLATPAK_PROTONTRICKS.to_string()],
        )
    } else {
        ("protontricks".to_string(), Vec::new())
    };
    args.extend(["-c".to_string(), inner, game.app_id.to_string()]);

    LaunchPlan {
        backend: Backend::Protontricks,
        program,
        args,
        env: vec![("PROTONTRICKS_NO_GUI".into(), "1".into())],
        working_dir: trainer.parent().map(Path::to_path_buf),
        note: if flatpak {
            "protontricks (Flatpak) exécute le trainer dans le préfixe du jeu.".into()
        } else {
            "protontricks exécute le trainer dans le préfixe du jeu.".into()
        },
    }
}

fn proton_plan(game: &SteamGame, trainer: &Path) -> Result<LaunchPlan> {
    let compat_data = game.compat_data_path();
    if !compat_data.join("pfx").is_dir() {
        return Err(TuxError::NoPrefix {
            app_id: game.app_id,
        });
    }
    let runtime = proton::resolve(game)?;

    Ok(LaunchPlan {
        backend: Backend::Proton,
        program: runtime.entry_point.to_string_lossy().into_owned(),
        args: vec!["run".into(), trainer.to_string_lossy().into_owned()],
        env: vec![
            (
                "STEAM_COMPAT_DATA_PATH".into(),
                compat_data.to_string_lossy().into_owned(),
            ),
            (
                "STEAM_COMPAT_CLIENT_INSTALL_PATH".into(),
                game.steam_root.to_string_lossy().into_owned(),
            ),
        ],
        working_dir: trainer.parent().map(Path::to_path_buf),
        note: format!("Repli natif : {} ({}).", runtime.name, runtime.source),
    })
}

fn wine_plan(game: &SteamGame, trainer: &Path) -> Result<LaunchPlan> {
    let prefix = game.compat_data_path().join("pfx");
    if !prefix.is_dir() {
        return Err(TuxError::NoPrefix {
            app_id: game.app_id,
        });
    }
    let wine = which("wine").ok_or_else(|| TuxError::MissingDependency {
        name: "wine".into(),
        hint: "Installe-le avec : sudo pacman -S wine".into(),
    })?;

    Ok(LaunchPlan {
        backend: Backend::Wine,
        program: wine.to_string_lossy().into_owned(),
        args: vec![trainer.to_string_lossy().into_owned()],
        env: vec![
            ("WINEPREFIX".into(), prefix.to_string_lossy().into_owned()),
            ("WINEDEBUG".into(), "fixme-all".into()),
        ],
        working_dir: trainer.parent().map(Path::to_path_buf),
        note: "Dernier recours : wine système sur le préfixe Proton. La version \
               de Wine diffère de celle de Proton et peut modifier le préfixe."
            .into(),
    })
}

/// Choisit et construit le plan de lancement en fonction du backend demandé.
///
/// L'état des dépendances est fourni par l'appelant (il est mis en cache dans
/// l'état de l'application) : construire un plan ne doit pas relancer une
/// détection système à chaque sélection de jeu.
pub fn build_plan(
    game: &SteamGame,
    trainer: &Path,
    preference: Backend,
    deps: &Dependencies,
) -> Result<LaunchPlan> {
    let has_protontricks = deps.protontricks.is_some() || deps.protontricks_flatpak;

    match preference {
        Backend::Protontricks => {
            if !has_protontricks {
                return Err(TuxError::MissingDependency {
                    name: "protontricks".into(),
                    hint: "Installe-le avec : sudo pacman -S protontricks (ou \
                           flatpak install com.github.Matoking.protontricks)"
                        .into(),
                });
            }
            Ok(protontricks_plan(
                game,
                trainer,
                deps.protontricks.is_none(),
            ))
        }
        Backend::Proton => proton_plan(game, trainer),
        Backend::Wine => wine_plan(game, trainer),
        Backend::Auto => {
            if has_protontricks {
                return Ok(protontricks_plan(
                    game,
                    trainer,
                    deps.protontricks.is_none(),
                ));
            }
            match proton_plan(game, trainer) {
                Ok(plan) => Ok(plan),
                Err(proton_error) => wine_plan(game, trainer).map_err(|wine_error| {
                    // On remonte l'erreur la plus informative des deux replis.
                    match wine_error {
                        TuxError::MissingDependency { .. } => proton_error,
                        other => other,
                    }
                }),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Exécution
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningTrainer {
    pub app_id: u32,
    pub pid: u32,
    pub backend: Backend,
    pub started_at: i64,
}

/// Trainers lancés par ArchMod, indexés par AppID.
#[derive(Default)]
pub struct TrainerRegistry {
    inner: Mutex<HashMap<u32, RunningTrainer>>,
}

impl TrainerRegistry {
    pub async fn snapshot(&self) -> Vec<RunningTrainer> {
        let mut running: Vec<_> = self.inner.lock().await.values().cloned().collect();
        running.sort_by_key(|r| r.app_id);
        running
    }

    pub async fn get(&self, app_id: u32) -> Option<RunningTrainer> {
        self.inner.lock().await.get(&app_id).cloned()
    }

    async fn insert(&self, entry: RunningTrainer) {
        self.inner.lock().await.insert(entry.app_id, entry);
    }

    async fn remove(&self, app_id: u32) {
        self.inner.lock().await.remove(&app_id);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOutcome {
    pub started: bool,
    /// `true` si le jeu ne tourne pas : le frontend demande confirmation.
    pub requires_confirmation: bool,
    pub message: String,
    pub plan: Option<LaunchPlan>,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrainerStateEvent {
    app_id: u32,
    running: bool,
    exit_code: Option<i32>,
    message: String,
}

/// Lance le trainer. `force` court-circuite l'avertissement « jeu non détecté ».
#[allow(clippy::too_many_arguments)]
pub async fn launch(
    app: AppHandle,
    registry: Arc<TrainerRegistry>,
    game: SteamGame,
    trainer: PathBuf,
    preference: Backend,
    deps: &Dependencies,
    warn_if_game_not_running: bool,
    force: bool,
) -> Result<LaunchOutcome> {
    if !trainer.is_file() {
        return Err(TuxError::TrainerMissing(trainer));
    }
    if registry.get(game.app_id).await.is_some() {
        return Err(TuxError::AlreadyRunning);
    }

    let run_state = game_run_state(&game);
    if warn_if_game_not_running && !run_state.running && !force {
        return Ok(LaunchOutcome {
            started: false,
            requires_confirmation: true,
            message: format!(
                "« {} » ne semble pas en cours d'exécution. Les trainers doivent \
                 généralement être lancés après le jeu pour pouvoir s'y accrocher.",
                game.name
            ),
            plan: None,
            pid: None,
        });
    }

    let plan = build_plan(&game, &trainer, preference, deps)?;

    log(&app, Some(game.app_id), LogLevel::Info, &plan.note);
    if !run_state.running {
        log(
            &app,
            Some(game.app_id),
            LogLevel::Warn,
            format!("« {} » n'est pas détecté comme lancé.", game.name),
        );
    }
    log(
        &app,
        Some(game.app_id),
        LogLevel::Command,
        plan.command_line(),
    );

    let mut command = tokio::process::Command::new(&plan.program);
    command
        .args(&plan.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    for (key, value) in &plan.env {
        command.env(key, value);
    }
    if let Some(dir) = plan.working_dir.as_ref().filter(|d| d.is_dir()) {
        command.current_dir(dir);
    }
    // Groupe de processus dédié : permet d'arrêter wine et ses enfants d'un bloc.
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command.spawn().map_err(|source| {
        let error = TuxError::Spawn {
            program: plan.program.clone(),
            source,
        };
        log(&app, Some(game.app_id), LogLevel::Error, error.to_string());
        error
    })?;

    let pid = child.id().unwrap_or(0);
    registry
        .insert(RunningTrainer {
            app_id: game.app_id,
            pid,
            backend: plan.backend,
            started_at: chrono::Utc::now().timestamp(),
        })
        .await;

    if let Some(stdout) = child.stdout.take() {
        stream_output(app.clone(), game.app_id, stdout, LogLevel::Stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        stream_output(app.clone(), game.app_id, stderr, LogLevel::Stderr);
    }

    let _ = app.emit(
        EVENT_TRAINER_STATE,
        TrainerStateEvent {
            app_id: game.app_id,
            running: true,
            exit_code: None,
            message: format!("Trainer lancé (PID {pid})."),
        },
    );
    log(
        &app,
        Some(game.app_id),
        LogLevel::Success,
        format!("Trainer lancé (PID {pid}) via {:?}.", plan.backend),
    );

    // Surveillance en tâche de fond : le thread principal reste libre.
    let watcher_app = app.clone();
    let watcher_registry = Arc::clone(&registry);
    let app_id = game.app_id;
    tokio::spawn(async move {
        let status = child.wait().await;
        watcher_registry.remove(app_id).await;

        let (level, exit_code, message) = match status {
            Ok(status) if status.success() => (
                LogLevel::Success,
                status.code(),
                "Le trainer s'est terminé normalement.".to_string(),
            ),
            Ok(status) => (
                LogLevel::Warn,
                status.code(),
                format!(
                    "Le trainer s'est terminé avec le code {}.",
                    status
                        .code()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "inconnu (signal)".into())
                ),
            ),
            Err(err) => (
                LogLevel::Error,
                None,
                format!("Suivi du processus impossible : {err}"),
            ),
        };

        log(&watcher_app, Some(app_id), level, &message);
        let _ = watcher_app.emit(
            EVENT_TRAINER_STATE,
            TrainerStateEvent {
                app_id,
                running: false,
                exit_code,
                message,
            },
        );
    });

    Ok(LaunchOutcome {
        started: true,
        requires_confirmation: false,
        message: format!("Trainer lancé pour « {} ».", game.name),
        plan: Some(plan),
        pid: Some(pid),
    })
}

fn stream_output<R>(app: AppHandle, app_id: u32, reader: R, level: LogLevel)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if !line.trim().is_empty() {
                        log(&app, Some(app_id), level, line);
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    log(
                        &app,
                        Some(app_id),
                        LogLevel::Error,
                        format!("Lecture de la sortie interrompue : {err}"),
                    );
                    break;
                }
            }
        }
    });
}

/// Arrête un trainer lancé par ArchMod (SIGTERM au groupe de processus).
pub async fn stop(app: &AppHandle, registry: &TrainerRegistry, app_id: u32) -> Result<bool> {
    let Some(entry) = registry.get(app_id).await else {
        return Ok(false);
    };
    if entry.pid == 0 {
        return Err(TuxError::Internal(
            "PID du trainer inconnu, arrêt impossible".into(),
        ));
    }

    // Le processus a été lancé dans son propre groupe : on cible le groupe
    // entier pour emporter wine et ses enfants, sans toucher au jeu.
    // SAFETY: appel POSIX sur un PGID que nous avons nous-mêmes créé.
    let result = unsafe { libc::killpg(entry.pid as libc::pid_t, libc::SIGTERM) };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        // Le groupe a pu disparaître entre-temps : ce n'est pas un échec.
        if error.raw_os_error() == Some(libc::ESRCH) {
            registry.remove(app_id).await;
            return Ok(false);
        }
        return Err(TuxError::Internal(format!(
            "arrêt du trainer impossible : {error}"
        )));
    }

    log(
        app,
        Some(app_id),
        LogLevel::Info,
        format!("Signal d'arrêt envoyé au trainer (PID {}).", entry.pid),
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_game() -> SteamGame {
        SteamGame {
            app_id: 292030,
            name: "The Witcher 3".into(),
            steam_root: PathBuf::from("/home/joueur/.local/share/Steam"),
            library_path: PathBuf::from("/mnt/jeux/SteamLibrary"),
            install_path: PathBuf::from("/mnt/jeux/SteamLibrary/steamapps/common/The Witcher 3"),
            size_on_disk: 0,
            last_played: 0,
            prefix_path: None,
        }
    }

    #[test]
    fn quotes_paths_containing_spaces_and_quotes() {
        assert_eq!(
            shell_quote("/tmp/mon trainer.exe"),
            "'/tmp/mon trainer.exe'"
        );
        assert_eq!(shell_quote("l'apostrophe"), r"'l'\''apostrophe'");
    }

    #[test]
    fn protontricks_command_matches_reference_form() {
        let game = dummy_game();
        let plan = protontricks_plan(&game, Path::new("/tmp/FLiNG Trainer.exe"), false);
        assert_eq!(plan.program, "protontricks");
        assert_eq!(
            plan.args,
            vec!["-c", "wine '/tmp/FLiNG Trainer.exe'", "292030"]
        );
        assert_eq!(plan.working_dir, Some(PathBuf::from("/tmp")));
    }

    #[test]
    fn flatpak_variant_wraps_the_same_arguments() {
        let game = dummy_game();
        let plan = protontricks_plan(&game, Path::new("/tmp/t.exe"), true);
        assert_eq!(plan.program, "flatpak");
        assert_eq!(plan.args[0], "run");
        assert_eq!(plan.args[1], FLATPAK_PROTONTRICKS);
        assert_eq!(plan.args[3], "wine '/tmp/t.exe'");
        // La ligne affichée reste copiable telle quelle dans un shell : le
        // sous-shell passé à protontricks est re-quoté une seconde fois.
        assert!(plan.command_line().contains(r"'wine '\''/tmp/t.exe'\'''"));
    }

    #[test]
    fn compat_data_path_uses_the_owning_library() {
        assert_eq!(
            dummy_game().compat_data_path(),
            PathBuf::from("/mnt/jeux/SteamLibrary/steamapps/compatdata/292030")
        );
    }
}
