//! Découverte des installations Steam et des jeux installés.
//!
//! Parcourt les racines Steam connues (natif, `~/.steam/root`, Flatpak), lit
//! `steamapps/libraryfolders.vdf` pour découvrir les bibliothèques sur d'autres
//! disques, puis parse chaque `appmanifest_<AppID>.acf`.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, TuxError};
use crate::vdf;

/// AppIDs d'outils Steam qui ne sont pas des jeux.
const TOOL_APP_IDS: &[u32] = &[
    228980,  // Steamworks Common Redistributables
    1070560, // Steam Linux Runtime 1.0 (scout)
    1391110, // Steam Linux Runtime 2.0 (soldier)
    1493710, // Proton Experimental
    1580130, // Proton 7.0
    1628350, // Steam Linux Runtime 3.0 (sniper)
    1826330, // Proton 8.0 (compat tool)
    2180100, // Proton Hotfix
    2230260, // Proton 9.0
    2348590, // Proton 10.0
];

const TOOL_NAME_PREFIXES: &[&str] = &[
    "Proton",
    "Steam Linux Runtime",
    "Steamworks Common",
    "SteamVR",
    "Steam Controller",
];

/// Un jeu installé, tel que décrit par son `appmanifest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamGame {
    pub app_id: u32,
    pub name: String,
    /// Racine Steam à laquelle appartient la bibliothèque (utile pour Proton).
    pub steam_root: PathBuf,
    /// Bibliothèque contenant le jeu (peut être sur un autre disque).
    pub library_path: PathBuf,
    /// Dossier d'installation absolu (`steamapps/common/<installdir>`).
    pub install_path: PathBuf,
    pub size_on_disk: u64,
    /// Timestamp Unix, `0` si le jeu n'a jamais été lancé.
    pub last_played: u64,
    /// Préfixe Proton (`steamapps/compatdata/<AppID>/pfx`) s'il existe déjà.
    pub prefix_path: Option<PathBuf>,
}

impl SteamGame {
    pub fn compat_data_path(&self) -> PathBuf {
        self.library_path
            .join("steamapps/compatdata")
            .join(self.app_id.to_string())
    }
}

/// Racines Steam candidates, dédupliquées et canonicalisées.
pub fn steam_roots() -> Result<Vec<PathBuf>> {
    let home = dirs::home_dir()
        .ok_or_else(|| TuxError::Internal("dossier personnel introuvable".into()))?;

    let candidates = [
        home.join(".local/share/Steam"),
        home.join(".steam/root"),
        home.join(".steam/steam"),
        home.join(".steam/debian-installation"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
    ];

    let mut seen = HashSet::new();
    let mut roots = Vec::new();
    for candidate in &candidates {
        if !candidate.join("steamapps").is_dir() {
            continue;
        }
        let canonical = candidate
            .canonicalize()
            .unwrap_or_else(|_| candidate.clone());
        if seen.insert(canonical.clone()) {
            roots.push(canonical);
        }
    }

    if roots.is_empty() {
        return Err(TuxError::SteamNotFound {
            tried: candidates
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    Ok(roots)
}

/// Bibliothèques déclarées dans `libraryfolders.vdf`, plus la racine elle-même.
///
/// Un `libraryfolders.vdf` illisible ou corrompu ne doit pas faire échouer le
/// scan complet : on retombe sur la racine seule.
pub fn library_folders(root: &Path) -> Vec<PathBuf> {
    let mut libraries = vec![root.to_path_buf()];

    let manifest = root.join("steamapps/libraryfolders.vdf");
    if let Ok(parsed) = vdf::parse_file(&manifest) {
        let container = parsed
            .get("libraryfolders")
            .or_else(|| parsed.get("LibraryFolders"))
            .unwrap_or(&parsed);

        for (_, entry) in container.entries() {
            // Ancien format : "1" "/mnt/games" — nouveau : "1" { "path" "..." }
            let path = match entry {
                vdf::VdfValue::Str(s) => Some(PathBuf::from(s)),
                vdf::VdfValue::Obj(_) => entry.get_str("path").map(PathBuf::from),
            };
            if let Some(path) = path {
                if path.join("steamapps").is_dir() {
                    libraries.push(path);
                }
            }
        }
    }

    let mut seen = HashSet::new();
    libraries
        .into_iter()
        .map(|p| p.canonicalize().unwrap_or(p))
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

fn is_tool(app_id: u32, name: &str) -> bool {
    TOOL_APP_IDS.contains(&app_id)
        || TOOL_NAME_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

fn parse_manifest(path: &Path, steam_root: &Path, library: &Path) -> Result<Option<SteamGame>> {
    let parsed = vdf::parse_file(path)?;
    let state = parsed
        .get("AppState")
        .ok_or_else(|| TuxError::vdf(path, "section « AppState » absente"))?;

    let app_id = state
        .get_u32("appid")
        .ok_or_else(|| TuxError::vdf(path, "clé « appid » absente ou invalide"))?;
    let name = state
        .get_str("name")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("AppID {app_id}"));

    if is_tool(app_id, &name) {
        return Ok(None);
    }

    let install_dir = state.get_str("installdir").unwrap_or(&name);
    let install_path = library.join("steamapps/common").join(install_dir);

    // Un manifeste peut subsister après désinstallation partielle.
    if !install_path.is_dir() {
        return Ok(None);
    }

    let compat = library
        .join("steamapps/compatdata")
        .join(app_id.to_string())
        .join("pfx");

    Ok(Some(SteamGame {
        app_id,
        name,
        steam_root: steam_root.to_path_buf(),
        library_path: library.to_path_buf(),
        install_path,
        size_on_disk: state.get_u64("SizeOnDisk").unwrap_or(0),
        last_played: state.get_u64("LastPlayed").unwrap_or(0),
        prefix_path: compat.is_dir().then_some(compat),
    }))
}

/// Scanne toutes les racines et bibliothèques et retourne les jeux installés,
/// triés par nom. Les manifestes illisibles sont ignorés silencieusement (ils
/// sont fréquents pendant une installation en cours) mais comptabilisés.
pub fn scan_games() -> Result<Vec<SteamGame>> {
    let roots = steam_roots()?;
    // BTreeMap : dédoublonne par AppID (un jeu peut apparaître via ~/.steam/root
    // et ~/.local/share/Steam) tout en gardant un ordre déterministe.
    let mut games: BTreeMap<u32, SteamGame> = BTreeMap::new();

    for root in &roots {
        for library in library_folders(root) {
            let steamapps = library.join("steamapps");
            let Ok(entries) = std::fs::read_dir(&steamapps) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let is_manifest = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("appmanifest_") && n.ends_with(".acf"));
                if !is_manifest {
                    continue;
                }
                match parse_manifest(&path, root, &library) {
                    Ok(Some(game)) => {
                        games.entry(game.app_id).or_insert(game);
                    }
                    Ok(None) => {}
                    Err(err) => {
                        eprintln!("[archmod] manifeste ignoré ({}) : {err}", path.display());
                    }
                }
            }
        }
    }

    let mut games: Vec<_> = games.into_values().collect();
    games.sort_by_key(|game| game.name.to_lowercase());
    Ok(games)
}

/// Retrouve un jeu précis par AppID.
pub fn find_game(app_id: u32) -> Result<SteamGame> {
    scan_games()?
        .into_iter()
        .find(|g| g.app_id == app_id)
        .ok_or(TuxError::UnknownGame { app_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_out_runtime_tools() {
        assert!(is_tool(228980, "Steamworks Common Redistributables"));
        assert!(is_tool(9999, "Proton 9.0 Beta"));
        assert!(is_tool(1628350, "Steam Linux Runtime 3.0 (sniper)"));
        assert!(!is_tool(292030, "The Witcher 3: Wild Hunt"));
        assert!(!is_tool(105600, "Terraria"));
    }
}
