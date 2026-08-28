//! Localisation du runtime Proton associé à un jeu.
//!
//! Utilisé par le repli natif de l'injecteur, quand `protontricks` n'est pas
//! installé. On privilégie l'information écrite par Proton lui-même dans
//! `compatdata/<AppID>/config_info`, qui pointe exactement vers la version
//! ayant créé le préfixe.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Result, TuxError};
use crate::steam_scanner::SteamGame;
use crate::vdf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtonRuntime {
    /// Racine de la distribution Proton (contient le script `proton`).
    pub dir: PathBuf,
    /// Script `proton` à invoquer (`proton run <exe>`).
    pub entry_point: PathBuf,
    pub name: String,
    /// Comment cette version a été trouvée (affiché dans la console).
    pub source: &'static str,
}

/// Cherche un exécutable dans le `PATH`.
pub fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn runtime_from_dir(dir: PathBuf, source: &'static str) -> Option<ProtonRuntime> {
    let entry_point = dir.join("proton");
    if !entry_point.is_file() {
        return None;
    }
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Proton")
        .to_string();
    Some(ProtonRuntime {
        dir,
        entry_point,
        name,
        source,
    })
}

/// `config_info` contient, ligne par ligne, les chemins de la distribution Wine
/// utilisée pour créer le préfixe. On remonte de `.../files/share/wine/` vers la
/// racine Proton.
fn from_config_info(game: &SteamGame) -> Option<ProtonRuntime> {
    let config_info = game.compat_data_path().join("config_info");
    let content = std::fs::read_to_string(config_info).ok()?;

    for line in content.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Some(index) = line.find("/files/") else {
            continue;
        };
        let dir = PathBuf::from(&line[..index]);
        if let Some(runtime) = runtime_from_dir(dir, "config_info du préfixe") {
            return Some(runtime);
        }
    }
    None
}

/// Nom de l'outil de compatibilité associé au jeu dans `config.vdf`
/// (`proton_experimental`, `GE-Proton9-20`, ...). `None` si Steam utilise le
/// choix par défaut sans le matérialiser.
fn mapped_tool_name(game: &SteamGame) -> Option<String> {
    let config = game.steam_root.join("config/config.vdf");
    let parsed = vdf::parse_file(config).ok()?;
    let mapping = parsed.path(&[
        "InstallConfigStore",
        "Software",
        "Valve",
        "Steam",
        "CompatToolMapping",
    ])?;

    let named = |key: &str| -> Option<String> {
        let name = mapping.get(key)?.get_str("name")?.trim();
        (!name.is_empty()).then(|| name.to_string())
    };

    // Réglage propre au jeu, sinon réglage global (clé "0").
    named(&game.app_id.to_string()).or_else(|| named("0"))
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Tous les dossiers susceptibles de contenir une distribution Proton.
fn tool_directories(game: &SteamGame) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for base in [&game.steam_root, &game.library_path] {
        dirs.push(base.join("compatibilitytools.d"));
        dirs.push(base.join("steamapps/common"));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".steam/root/compatibilitytools.d"));
        dirs.push(home.join(".local/share/Steam/compatibilitytools.d"));
    }
    dirs
}

fn candidate_runtimes(game: &SteamGame) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in tool_directories(game) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join("proton").is_file() {
                found.push(path);
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Résout la version de Proton à utiliser pour ce jeu.
pub fn resolve(game: &SteamGame) -> Result<ProtonRuntime> {
    if let Some(runtime) = from_config_info(game) {
        return Ok(runtime);
    }

    let candidates = candidate_runtimes(game);

    if let Some(tool) = mapped_tool_name(game) {
        let wanted = normalize(&tool);
        let matched = candidates.iter().find(|dir| {
            dir.file_name()
                .and_then(|n| n.to_str())
                .map(normalize)
                .is_some_and(|name| {
                    name == wanted || name.contains(&wanted) || wanted.contains(&name)
                })
        });
        if let Some(dir) = matched {
            if let Some(runtime) = runtime_from_dir(dir.clone(), "outil de compatibilité Steam") {
                return Ok(runtime);
            }
        }
    }

    // Dernier recours : la distribution Proton modifiée le plus récemment.
    let newest = candidates
        .into_iter()
        .filter_map(|dir| {
            let modified = std::fs::metadata(&dir).and_then(|m| m.modified()).ok()?;
            Some((modified, dir))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, dir)| dir);

    newest
        .and_then(|dir| runtime_from_dir(dir, "version Proton la plus récente"))
        .ok_or(TuxError::NoProtonRuntime {
            app_id: game.app_id,
        })
}
