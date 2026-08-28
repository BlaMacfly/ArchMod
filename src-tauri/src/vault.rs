//! Coffre : association persistante « jeu Steam → trainer », plus les réglages.
//!
//! Le fichier vit dans `~/.config/ArchMod/config.json`. Il est écrit de façon
//! atomique (fichier temporaire + `rename`) pour qu'une coupure de courant ne
//! laisse jamais une configuration tronquée derrière elle.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, TuxError};

pub const CONFIG_VERSION: u32 = 1;

/// Backend d'injection choisi par l'utilisateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// protontricks si disponible, sinon Proton natif, sinon Wine système.
    #[default]
    Auto,
    Protontricks,
    Proton,
    Wine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Autorise le téléchargement des jaquettes depuis le CDN Steam.
    pub allow_network_artwork: bool,
    /// Avertit avant de lancer un trainer si le jeu n'est pas détecté.
    pub warn_if_game_not_running: bool,
    pub backend: Backend,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            allow_network_artwork: true,
            warn_if_game_not_running: true,
            backend: Backend::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerEntry {
    pub path: PathBuf,
    /// Nom affiché ; par défaut le nom du fichier.
    pub label: String,
    pub added_at: i64,
    #[serde(default)]
    pub last_launched_at: Option<i64>,
    #[serde(default)]
    pub launch_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vault {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub trainers: BTreeMap<u32, TrainerEntry>,
}

fn default_version() -> u32 {
    CONFIG_VERSION
}

impl Default for Vault {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            settings: Settings::default(),
            trainers: BTreeMap::new(),
        }
    }
}

pub fn config_dir() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .ok_or_else(|| TuxError::Internal("dossier de configuration introuvable".into()))?
        .join("ArchMod"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

/// Charge la configuration depuis `path`.
///
/// - fichier absent → configuration par défaut (cas du premier lancement) ;
/// - fichier illisible ou JSON invalide → erreur explicite, jamais d'écrasement
///   silencieux : c'est à l'utilisateur de décider via `repair`.
pub fn load_from(path: &Path) -> Result<Vault> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vault::default()),
        Err(err) => return Err(TuxError::io(path, err)),
    };

    if raw.trim().is_empty() {
        return Ok(Vault::default());
    }

    let mut vault: Vault = serde_json::from_str(&raw).map_err(|err| TuxError::ConfigCorrupted {
        path: path.to_path_buf(),
        detail: err.to_string(),
    })?;

    if vault.version > CONFIG_VERSION {
        return Err(TuxError::ConfigCorrupted {
            path: path.to_path_buf(),
            detail: format!(
                "version {} écrite par une version plus récente de ArchMod (max supportée : {CONFIG_VERSION})",
                vault.version
            ),
        });
    }
    vault.version = CONFIG_VERSION;
    Ok(vault)
}

pub fn save_to(path: &Path, vault: &Vault) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| TuxError::ConfigWrite {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let serialized = serde_json::to_string_pretty(vault)
        .map_err(|err| TuxError::Internal(format!("sérialisation impossible : {err}")))?;

    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serialized).map_err(|source| TuxError::ConfigWrite {
        path: temporary.clone(),
        source,
    })?;
    std::fs::rename(&temporary, path).map_err(|source| TuxError::ConfigWrite {
        path: path.to_path_buf(),
        source,
    })
}

pub fn load() -> Result<Vault> {
    load_from(&config_path()?)
}

pub fn save(vault: &Vault) -> Result<()> {
    save_to(&config_path()?, vault)
}

/// Met de côté une configuration corrompue et repart d'une base saine.
/// Retourne le chemin de la sauvegarde, s'il y avait quelque chose à sauver.
pub fn repair() -> Result<Option<PathBuf>> {
    let path = config_path()?;
    let mut backup = None;

    if path.is_file() {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let target = path.with_file_name(format!("config.corrupted-{stamp}.json"));
        std::fs::rename(&path, &target).map_err(|source| TuxError::ConfigWrite {
            path: target.clone(),
            source,
        })?;
        backup = Some(target);
    }

    save(&Vault::default())?;
    Ok(backup)
}

/// Valide un chemin de trainer avant de l'enregistrer.
pub fn validate_trainer(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        return Err(TuxError::TrainerMissing(path.to_path_buf()));
    }
    if !path.is_file() {
        return Err(TuxError::TrainerNotExe(path.to_path_buf()));
    }
    let is_exe = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
    if !is_exe {
        return Err(TuxError::TrainerNotExe(path.to_path_buf()));
    }
    Ok(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
}

impl Vault {
    pub fn set_trainer(&mut self, app_id: u32, path: &Path) -> Result<TrainerEntry> {
        let path = validate_trainer(path)?;
        let label = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Trainer")
            .to_string();

        let entry = TrainerEntry {
            path,
            label,
            added_at: chrono::Utc::now().timestamp(),
            last_launched_at: None,
            launch_count: 0,
        };
        // Conserve l'historique si le trainer est simplement remplacé.
        let entry = match self.trainers.get(&app_id) {
            Some(previous) => TrainerEntry {
                launch_count: previous.launch_count,
                last_launched_at: previous.last_launched_at,
                ..entry
            },
            None => entry,
        };
        self.trainers.insert(app_id, entry.clone());
        Ok(entry)
    }

    pub fn remove_trainer(&mut self, app_id: u32) -> Option<TrainerEntry> {
        self.trainers.remove(&app_id)
    }

    pub fn trainer(&self, app_id: u32) -> Result<&TrainerEntry> {
        self.trainers
            .get(&app_id)
            .ok_or(TuxError::NoTrainerLinked { app_id })
    }

    pub fn mark_launched(&mut self, app_id: u32) {
        if let Some(entry) = self.trainers.get_mut(&app_id) {
            entry.last_launched_at = Some(chrono::Utc::now().timestamp());
            entry.launch_count = entry.launch_count.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archmod-tests-{name}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("config.json")
    }

    #[test]
    fn missing_file_yields_defaults() {
        let path = scratch("missing").with_file_name("nope.json");
        let vault = load_from(&path).expect("un fichier absent n'est pas une erreur");
        assert!(vault.trainers.is_empty());
        assert_eq!(vault.settings.backend, Backend::Auto);
    }

    #[test]
    fn corrupted_file_is_reported() {
        let path = scratch("corrupted");
        std::fs::write(&path, "{ ceci n'est pas du json").expect("écriture");
        let err = load_from(&path).expect_err("doit échouer");
        assert_eq!(err.kind(), "config_corrupted");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn round_trip_preserves_entries() {
        let path = scratch("roundtrip");
        let mut vault = Vault::default();
        vault.trainers.insert(
            292030,
            TrainerEntry {
                path: PathBuf::from("/tmp/Witcher3.exe"),
                label: "Witcher3".into(),
                added_at: 42,
                last_launched_at: None,
                launch_count: 3,
            },
        );
        vault.settings.backend = Backend::Protontricks;
        save_to(&path, &vault).expect("écriture");

        let reloaded = load_from(&path).expect("relecture");
        assert_eq!(reloaded.settings.backend, Backend::Protontricks);
        assert_eq!(reloaded.trainer(292030).expect("entrée").launch_count, 3);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_non_exe_trainer() {
        let path = scratch("notexe").with_file_name("trainer.txt");
        std::fs::write(&path, "x").expect("écriture");
        let err = validate_trainer(&path).expect_err("doit refuser un .txt");
        assert_eq!(err.kind(), "trainer_not_exe");
        let _ = std::fs::remove_file(&path);
    }
}
