//! Erreurs applicatives de ArchMod.
//!
//! Toutes les erreurs remontées au frontend sont sérialisées sous la forme
//! `{ "kind": "...", "message": "...", "hint": "..." }` afin que l'interface
//! puisse afficher un message clair et, quand c'est possible, une action
//! corrective (installer une dépendance, réparer la configuration, ...).

use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};

pub type Result<T> = std::result::Result<T, TuxError>;

#[derive(Debug, thiserror::Error)]
pub enum TuxError {
    #[error("Impossible de localiser une installation Steam (dossiers testés : {tried})")]
    SteamNotFound { tried: String },

    #[error("Fichier introuvable : {}", .0.display())]
    FileNotFound(PathBuf),

    #[error("Lecture impossible de {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Le fichier VDF/ACF {} est invalide : {detail}", .path.display())]
    VdfParse { path: PathBuf, detail: String },

    #[error("La configuration {} est corrompue : {detail}", .path.display())]
    ConfigCorrupted { path: PathBuf, detail: String },

    #[error("Écriture impossible de la configuration {}: {source}", .path.display())]
    ConfigWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Aucun trainer n'est associé au jeu {app_id}")]
    NoTrainerLinked { app_id: u32 },

    #[error("Le trainer {} n'existe plus sur le disque", .0.display())]
    TrainerMissing(PathBuf),

    #[error("Le fichier {} n'est pas un exécutable Windows (.exe attendu)", .0.display())]
    TrainerNotExe(PathBuf),

    #[error("Jeu inconnu dans la bibliothèque Steam : {app_id}")]
    UnknownGame { app_id: u32 },

    #[error("Dépendance manquante : {name}")]
    MissingDependency { name: String, hint: String },

    #[error("Aucun préfixe Proton pour l'AppID {app_id}. Lance le jeu une fois via Steam pour qu'il soit créé.")]
    NoPrefix { app_id: u32 },

    #[error("Aucune installation Proton exploitable n'a été trouvée pour l'AppID {app_id}")]
    NoProtonRuntime { app_id: u32 },

    #[error("Le trainer est déjà en cours d'exécution pour ce jeu")]
    AlreadyRunning,

    #[error("Échec du lancement de « {program} » : {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Le processus {pid} n'existe plus")]
    ProcessGone { pid: u32 },

    #[error("Module « {name} » introuvable dans le processus {pid}")]
    ModuleNotFound { pid: u32, name: String },

    #[error("Échec de {operation} mémoire à {address:#x} dans le processus {pid} : {source}")]
    MemoryAccess {
        pid: u32,
        address: u64,
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },

    #[error("Motif d'octets invalide « {pattern} » : {detail}")]
    PatternInvalid { pattern: String, detail: String },

    #[error("Détour impossible à {address:#x} : {reason}")]
    HookImpossible { address: u64, reason: String },

    #[error("Symbole « {symbol} » non résolu : il est produit par un script d'auto-assembleur")]
    SymbolUnresolved { symbol: String },

    #[error("« {name} » n'est pas en cours d'exécution")]
    GameNotRunning { name: String },

    #[error("Motif « {pattern} » : {found} correspondance(s) dans {module}, la n°{wanted} était demandée")]
    PatternNotFound {
        pattern: String,
        module: String,
        found: usize,
        wanted: usize,
    },

    #[error("Profil de trainer invalide : {detail}")]
    Profile { detail: String },

    #[error("Table Cheat Engine illisible : {detail}")]
    CheatTable { detail: String },

    #[error("{0}")]
    Internal(String),
}

impl TuxError {
    pub fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        let path = path.as_ref().to_path_buf();
        if source.kind() == std::io::ErrorKind::NotFound {
            TuxError::FileNotFound(path)
        } else {
            TuxError::Io { path, source }
        }
    }

    pub fn vdf(path: impl AsRef<Path>, detail: impl Into<String>) -> Self {
        TuxError::VdfParse {
            path: path.as_ref().to_path_buf(),
            detail: detail.into(),
        }
    }

    /// Identifiant stable, utilisé côté frontend pour choisir l'action corrective.
    pub fn kind(&self) -> &'static str {
        match self {
            TuxError::SteamNotFound { .. } => "steam_not_found",
            TuxError::FileNotFound(_) => "file_not_found",
            TuxError::Io { .. } => "io",
            TuxError::VdfParse { .. } => "vdf_parse",
            TuxError::ConfigCorrupted { .. } => "config_corrupted",
            TuxError::ConfigWrite { .. } => "config_write",
            TuxError::NoTrainerLinked { .. } => "no_trainer_linked",
            TuxError::TrainerMissing(_) => "trainer_missing",
            TuxError::TrainerNotExe(_) => "trainer_not_exe",
            TuxError::UnknownGame { .. } => "unknown_game",
            TuxError::MissingDependency { .. } => "missing_dependency",
            TuxError::NoPrefix { .. } => "no_prefix",
            TuxError::NoProtonRuntime { .. } => "no_proton_runtime",
            TuxError::AlreadyRunning => "already_running",
            TuxError::Spawn { .. } => "spawn",
            TuxError::ProcessGone { .. } => "process_gone",
            TuxError::ModuleNotFound { .. } => "module_not_found",
            TuxError::MemoryAccess { .. } => "memory_access",
            TuxError::PatternInvalid { .. } => "pattern_invalid",
            TuxError::HookImpossible { .. } => "hook_impossible",
            TuxError::SymbolUnresolved { .. } => "symbol_unresolved",
            TuxError::GameNotRunning { .. } => "game_not_running",
            TuxError::PatternNotFound { .. } => "pattern_not_found",
            TuxError::Profile { .. } => "profile",
            TuxError::CheatTable { .. } => "cheat_table",
            TuxError::Internal(_) => "internal",
        }
    }

    /// Conseil affiché sous le message d'erreur dans l'interface.
    pub fn hint(&self) -> Option<String> {
        match self {
            TuxError::SteamNotFound { .. } => Some(
                "Vérifie que Steam est installé (paquet `steam`) et qu'il a été lancé au moins une fois."
                    .into(),
            ),
            TuxError::ConfigCorrupted { .. } => Some(
                "Utilise « Réinitialiser la configuration » : l'ancien fichier sera sauvegardé à côté."
                    .into(),
            ),
            TuxError::MissingDependency { hint, .. } => Some(hint.clone()),
            TuxError::NoPrefix { .. } => Some(
                "Le préfixe Proton n'est créé qu'au premier lancement du jeu depuis Steam.".into(),
            ),
            TuxError::TrainerMissing(_) => {
                Some("Réimporte le trainer depuis son nouvel emplacement.".into())
            }
            TuxError::GameNotRunning { .. } => Some(
                "Lance le jeu depuis Steam : les adresses n'existent que pendant son exécution."
                    .into(),
            ),
            TuxError::PatternNotFound { .. } => Some(
                "Le jeu a probablement été mis à jour depuis l'écriture du profil : \
                 les adresses ont bougé."
                    .into(),
            ),
            TuxError::MemoryAccess { .. } => Some(
                "Si l'accès est refusé, vérifie kernel.yama.ptrace_scope (0 autorise la lecture \
                 entre processus d'un même utilisateur)."
                    .into(),
            ),
            TuxError::ProcessGone { .. } => {
                Some("Le jeu s'est fermé : relance-le avant de réessayer.".into())
            }
            TuxError::NoProtonRuntime { .. } => Some(
                "Installe Proton (ou Proton-GE) pour ce jeu depuis Steam, ou installe `protontricks`."
                    .into(),
            ),
            _ => None,
        }
    }
}

/// Copie sérialisable et clonable d'une erreur, pour les états conservés en
/// mémoire (`TuxError` contient des `io::Error`, non clonables).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorReport {
    pub kind: String,
    pub message: String,
    pub hint: Option<String>,
}

impl From<&TuxError> for ErrorReport {
    fn from(error: &TuxError) -> Self {
        Self {
            kind: error.kind().to_string(),
            message: error.to_string(),
            hint: error.hint(),
        }
    }
}

impl Serialize for TuxError {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("TuxError", 3)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.serialize_field("hint", &self.hint())?;
        state.end()
    }
}
