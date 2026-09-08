//! Lancement d'un jeu Steam depuis ArchMod.
//!
//! Le point délicat n'est pas la commande — `steam steam://rungameid/<AppID>`
//! suffit — mais le **choix du client** : une machine peut porter à la fois un
//! Steam natif et un Steam Flatpak, chacun avec sa propre bibliothèque. Passer
//! l'AppID au mauvais client ouvre le magasin au lieu de lancer le jeu. On se
//! fie donc à la racine Steam d'où le jeu a été découvert, jamais à ce qui se
//! trouve en premier dans le `PATH`.
//!
//! Une fois la commande émise, Steam rend la main immédiatement : le jeu, lui,
//! peut mettre une minute à apparaître (client à démarrer, shaders à compiler,
//! écrans de lancement). Une tâche de fond surveille donc l'apparition du
//! processus et prévient l'interface, qui peut alors proposer le panneau de
//! trainer sans que l'utilisateur ait à cliquer sur « rafraîchir ».

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::error::{Result, TuxError};
use crate::injector::{self, game_run_state, LogLevel};
use crate::proton::which;
use crate::steam_scanner::SteamGame;

pub const EVENT_GAME_STATE: &str = "archmod://game-state";

const FLATPAK_STEAM: &str = "com.valvesoftware.Steam";

/// Au-delà, on cesse de surveiller : le jeu ne démarrera probablement pas, et
/// une boucle qui balaie tous les processus n'a pas à tourner indéfiniment.
const DETECTION_TIMEOUT: Duration = Duration::from_secs(240);
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// Fenêtre pendant laquelle le code de retour du lanceur est significatif.
/// Au-delà, c'est que le processus *est* la session Steam : il vivra aussi
/// longtemps que le client, et l'attendre bloquerait la surveillance.
const LAUNCHER_GRACE: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Choix du client Steam
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SteamClient {
    /// Paquet système (`/usr/bin/steam`).
    Native,
    /// Application Flatpak `com.valvesoftware.Steam`.
    Flatpak,
    /// Dernier recours : on confie l'URL au gestionnaire du bureau.
    Desktop,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameLaunchPlan {
    pub client: SteamClient,
    pub program: String,
    pub args: Vec<String>,
    /// URL `steam://` transmise au client.
    pub url: String,
    /// Explication lisible, écrite dans la console avant le lancement.
    pub note: String,
}

impl GameLaunchPlan {
    /// Ligne de commande reconstituée, copiable telle quelle dans un terminal.
    pub fn command_line(&self) -> String {
        let args: Vec<String> = self
            .args
            .iter()
            .map(|arg| injector::shell_quote(arg))
            .collect();
        format!(
            "{} {}",
            injector::shell_quote(&self.program),
            args.join(" ")
        )
    }
}

/// Vrai si cette racine Steam appartient à l'installation Flatpak.
///
/// La racine est canonicalisée par le scanner, mais le composant
/// `com.valvesoftware.Steam` du chemin `~/.var/app/...` survit à l'opération.
fn is_flatpak_root(root: &Path) -> bool {
    root.components()
        .any(|component| component.as_os_str() == FLATPAK_STEAM)
}

/// Détermine comment lancer ce jeu précis.
pub fn plan(game: &SteamGame) -> Result<GameLaunchPlan> {
    let url = format!("steam://rungameid/{}", game.app_id);

    if is_flatpak_root(&game.steam_root) {
        let flatpak = which("flatpak").ok_or_else(|| TuxError::MissingDependency {
            name: "flatpak".into(),
            hint: "Ce jeu appartient à la bibliothèque du Steam Flatpak : \
                   `flatpak` doit être installé pour le lancer."
                .into(),
        })?;
        return Ok(GameLaunchPlan {
            client: SteamClient::Flatpak,
            program: flatpak.to_string_lossy().into_owned(),
            args: vec!["run".into(), FLATPAK_STEAM.into(), url.clone()],
            url,
            note: format!(
                "« {} » appartient à la bibliothèque du Steam Flatpak : lancement via `flatpak run`.",
                game.name
            ),
        });
    }

    if let Some(steam) = which("steam") {
        return Ok(GameLaunchPlan {
            client: SteamClient::Native,
            program: steam.to_string_lossy().into_owned(),
            args: vec![url.clone()],
            url,
            note: format!("Lancement de « {} » via le Steam système.", game.name),
        });
    }

    // Le jeu vient d'une racine native mais le binaire `steam` est introuvable :
    // le gestionnaire du bureau sait peut-être encore quoi faire de l'URL.
    if let Some(opener) = which("xdg-open") {
        return Ok(GameLaunchPlan {
            client: SteamClient::Desktop,
            program: opener.to_string_lossy().into_owned(),
            args: vec![url.clone()],
            url,
            note: "Binaire `steam` introuvable : l'URL est confiée au gestionnaire du bureau."
                .into(),
        });
    }

    Err(TuxError::MissingDependency {
        name: "steam".into(),
        hint: "Installe le client Steam (`sudo pacman -S steam`) ou lance le jeu depuis Steam."
            .into(),
    })
}

// ---------------------------------------------------------------------------
// Lancement et surveillance
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LaunchPhase {
    /// Commande transmise à Steam, le jeu n'est pas encore visible.
    Starting,
    /// Le processus du jeu a été détecté.
    Running,
    /// Rien n'est apparu dans le délai imparti.
    Timeout,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStateEvent {
    pub app_id: u32,
    pub phase: LaunchPhase,
    pub message: String,
    pub pids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameLaunchOutcome {
    /// La commande a été transmise au client Steam.
    pub started: bool,
    /// Le jeu tournait déjà : rien n'a été lancé.
    pub already_running: bool,
    pub message: String,
    pub plan: Option<GameLaunchPlan>,
}

fn emit(app: &AppHandle, event: GameStateEvent) {
    if let Err(err) = app.emit(EVENT_GAME_STATE, &event) {
        eprintln!("[archmod] émission de l'état du jeu impossible : {err}");
    }
}

/// Transmet l'URL de lancement au bon client Steam, puis surveille en fond
/// l'apparition du processus du jeu.
pub async fn launch(app: AppHandle, game: SteamGame) -> Result<GameLaunchOutcome> {
    let app_id = game.app_id;

    let state = {
        let game = game.clone();
        tokio::task::spawn_blocking(move || game_run_state(&game))
            .await
            .map_err(|err| TuxError::Internal(format!("détection interrompue : {err}")))?
    };
    if state.running {
        let message = format!("« {} » est déjà en cours d'exécution.", game.name);
        injector::log(&app, Some(app_id), LogLevel::Info, &message);
        return Ok(GameLaunchOutcome {
            started: false,
            already_running: true,
            message,
            plan: None,
        });
    }

    let plan = plan(&game)?;
    injector::log(&app, Some(app_id), LogLevel::Info, &plan.note);
    injector::log(&app, Some(app_id), LogLevel::Command, plan.command_line());

    let mut command = tokio::process::Command::new(&plan.program);
    command
        .args(&plan.args)
        .stdin(std::process::Stdio::null())
        // Steam, lancé à froid, écrit son journal complet sur la sortie standard
        // pendant toute la session : la console de l'interface serait noyée.
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(false);
    // Groupe dédié : fermer ArchMod ne doit pas emporter Steam ni le jeu.
    #[cfg(unix)]
    command.process_group(0);

    let child = command.spawn().map_err(|source| {
        let error = TuxError::Spawn {
            program: plan.program.clone(),
            source,
        };
        injector::log(&app, Some(app_id), LogLevel::Error, error.to_string());
        error
    })?;

    let message = format!(
        "Commande de lancement transmise à Steam pour « {} ». Le jeu peut mettre \
         un moment à apparaître.",
        game.name
    );
    emit(
        &app,
        GameStateEvent {
            app_id,
            phase: LaunchPhase::Starting,
            message: message.clone(),
            pids: Vec::new(),
        },
    );

    tokio::spawn(watch(app, game, child));

    Ok(GameLaunchOutcome {
        started: true,
        already_running: false,
        message,
        plan: Some(plan),
    })
}

/// Attend l'apparition du processus du jeu et en informe l'interface.
async fn watch(app: AppHandle, game: SteamGame, mut child: tokio::process::Child) {
    let app_id = game.app_id;
    let deadline = tokio::time::Instant::now() + DETECTION_TIMEOUT;

    // Steam déjà lancé : le processus transmet l'URL et rend la main aussitôt,
    // son code de retour est alors la seule trace d'un échec. Steam à froid :
    // le même processus devient la session Steam et ne rendra la main qu'à la
    // fermeture du client — on ne l'attend pas, on le laisse vivre.
    match tokio::time::timeout(LAUNCHER_GRACE, child.wait()).await {
        Ok(Ok(status)) if !status.success() => injector::log(
            &app,
            Some(app_id),
            LogLevel::Warn,
            format!("Le client Steam a rendu {status}. Le jeu ne démarrera peut-être pas."),
        ),
        Ok(Err(err)) => injector::log(
            &app,
            Some(app_id),
            LogLevel::Warn,
            format!("Suivi du client Steam impossible : {err}"),
        ),
        _ => {}
    }
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        let probe = game.clone();
        let Ok(state) = tokio::task::spawn_blocking(move || game_run_state(&probe)).await else {
            return;
        };

        if state.running {
            let message = format!(
                "« {} » est lancé ({}).",
                game.name,
                state
                    .pids
                    .iter()
                    .map(|pid| format!("pid {pid}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            injector::log(&app, Some(app_id), LogLevel::Success, &message);
            emit(
                &app,
                GameStateEvent {
                    app_id,
                    phase: LaunchPhase::Running,
                    message,
                    pids: state.pids,
                },
            );
            return;
        }

        if tokio::time::Instant::now() >= deadline {
            let message = format!(
                "« {} » n'a pas été détecté après {} s. Vérifie la fenêtre de Steam.",
                game.name,
                DETECTION_TIMEOUT.as_secs()
            );
            injector::log(&app, Some(app_id), LogLevel::Warn, &message);
            emit(
                &app,
                GameStateEvent {
                    app_id,
                    phase: LaunchPhase::Timeout,
                    message,
                    pids: Vec::new(),
                },
            );
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn game(steam_root: &str) -> SteamGame {
        SteamGame {
            app_id: 1898300,
            name: "ASKA".into(),
            steam_root: PathBuf::from(steam_root),
            library_path: PathBuf::from(steam_root),
            install_path: PathBuf::from(steam_root).join("steamapps/common/ASKA"),
            size_on_disk: 0,
            build_id: None,
            last_played: 0,
            prefix_path: None,
        }
    }

    #[test]
    fn recognises_the_flatpak_root() {
        assert!(is_flatpak_root(Path::new(
            "/home/joueur/.var/app/com.valvesoftware.Steam/.local/share/Steam"
        )));
        assert!(!is_flatpak_root(Path::new(
            "/home/joueur/.local/share/Steam"
        )));
        // Une bibliothèque sur un autre disque n'a rien de Flatpak.
        assert!(!is_flatpak_root(Path::new("/mnt/jeux/SteamLibrary")));
    }

    #[test]
    fn builds_the_flatpak_command_when_the_game_belongs_to_it() {
        let game = game("/home/joueur/.var/app/com.valvesoftware.Steam/.local/share/Steam");
        let Ok(plan) = plan(&game) else {
            // `flatpak` absent de la machine de test : rien à vérifier.
            return;
        };
        assert_eq!(plan.client, SteamClient::Flatpak);
        assert_eq!(
            plan.args,
            vec![
                "run".to_string(),
                FLATPAK_STEAM.to_string(),
                "steam://rungameid/1898300".to_string(),
            ]
        );
    }

    #[test]
    fn native_root_never_goes_through_flatpak() {
        let game = game("/home/joueur/.local/share/Steam");
        let Ok(plan) = plan(&game) else {
            return;
        };
        assert_ne!(plan.client, SteamClient::Flatpak);
        assert_eq!(plan.url, "steam://rungameid/1898300");
    }

    #[test]
    fn quotes_the_command_line() {
        let plan = GameLaunchPlan {
            client: SteamClient::Native,
            program: "/usr/bin/steam".into(),
            args: vec!["steam://rungameid/440".into()],
            url: "steam://rungameid/440".into(),
            note: String::new(),
        };
        assert_eq!(
            plan.command_line(),
            "'/usr/bin/steam' 'steam://rungameid/440'"
        );
    }
}
