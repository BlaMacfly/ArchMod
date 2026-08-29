//! Diagnostic et préparation du préfixe Proton d'un jeu.
//!
//! Un trainer Windows n'est pas une application ordinaire : la plupart sont
//! écrits en .NET et s'attachent à un processus tiers. Trois obstacles
//! reviennent systématiquement sous Proton, tous documentés par la communauté :
//!
//! - **Wine-Mono** remplace .NET par défaut et ne fait pas tourner les trainers
//!   récents ; il faut installer le vrai framework (`dotnet48`, ou `dotnet40`
//!   pour les plus anciens) ;
//! - **GE-Proton** est plus permissif que Proton pour les modifications
//!   mémoire ;
//! - **ESYNC/FSYNC** perturbent l'attachement au processus du jeu.
//!
//! Ce module constate, explique, et propose l'action corrective — il ne
//! modifie jamais un préfixe sans qu'on le lui demande.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::proton;
use crate::steam_scanner::SteamGame;

/// Composants installables dans un préfixe pour faire tourner un trainer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Component {
    /// .NET Framework 4.8 — le choix par défaut pour les trainers récents.
    Dotnet48,
    /// .NET Framework 4.0 — certains trainers anciens n'acceptent que celui-ci.
    Dotnet40,
    /// Polices de base, dont dépendent les interfaces de certains trainers.
    Corefonts,
}

impl Component {
    /// Verbe winetricks correspondant.
    pub fn verb(self) -> &'static str {
        match self {
            Component::Dotnet48 => "dotnet48",
            Component::Dotnet40 => "dotnet40",
            Component::Corefonts => "corefonts",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Component::Dotnet48 => ".NET Framework 4.8",
            Component::Dotnet40 => ".NET Framework 4.0",
            Component::Corefonts => "Polices Microsoft de base",
        }
    }
}

/// État du préfixe au regard de l'exécution d'un trainer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixReport {
    pub exists: bool,
    pub path: PathBuf,
    /// Nom de la distribution Proton utilisée, si elle a pu être déterminée.
    pub proton: Option<String>,
    /// Une variante GE-Proton, plus permissive pour les modifications mémoire.
    pub proton_is_ge: bool,
    /// Wine-Mono est présent : il remplace .NET et fait échouer bien des trainers.
    pub wine_mono: bool,
    /// Verbes déjà installés par winetricks/protontricks dans ce préfixe.
    pub installed: Vec<String>,
    /// Version de Windows annoncée aux applications (« 10.0 », « 5.1 »…).
    pub windows_version: Option<String>,
    /// Constats et actions conseillées, dans l'ordre d'importance.
    pub advice: Vec<Advice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Advice {
    pub message: String,
    /// Composant à installer pour lever le point signalé, s'il y en a un.
    pub install: Option<Component>,
    /// `true` quand le point empêchera vraisemblablement le trainer de démarrer.
    pub blocking: bool,
}

/// Verbes déjà installés, tels que consignés par winetricks dans le préfixe.
fn installed_verbs(prefix: &std::path::Path) -> Vec<String> {
    let log = prefix.join("winetricks.log");
    std::fs::read_to_string(log)
        .map(|content| {
            content
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Version de Windows déclarée dans la base de registre du préfixe.
///
/// Un préfixe réglé sur XP ou Vista fait échouer les trainers .NET récents ;
/// c'est un réglage qu'un correctif de jeu a pu poser sans qu'on s'en souvienne.
fn windows_version(prefix: &std::path::Path) -> Option<String> {
    let registry = std::fs::read_to_string(prefix.join("system.reg")).ok()?;
    let mut in_section = false;

    for line in registry.lines() {
        if line.starts_with('[') {
            // On ne veut que la vraie clé, pas son miroir Wow6432Node.
            in_section = line.starts_with("[Software\\Microsoft\\Windows NT\\CurrentVersion]");
            continue;
        }
        if in_section {
            if let Some(value) = line.strip_prefix("\"CurrentVersion\"=") {
                return Some(value.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// Analyse le préfixe d'un jeu et formule des recommandations.
pub fn inspect(game: &SteamGame) -> Result<PrefixReport> {
    let compat = game.compat_data_path();
    let prefix = compat.join("pfx");
    let exists = prefix.is_dir();

    let runtime = proton::resolve(game).ok();
    let proton_name = runtime.as_ref().map(|runtime| runtime.name.clone());
    let proton_is_ge = proton_name
        .as_deref()
        .is_some_and(|name| name.to_ascii_lowercase().contains("ge-proton"));

    // Wine-Mono s'installe dans le préfixe sous forme de dossier dédié.
    let wine_mono = prefix.join("drive_c/windows/mono").is_dir();
    let installed = installed_verbs(&prefix);
    let has_dotnet = installed.iter().any(|verb| verb.starts_with("dotnet"));
    let version = windows_version(&prefix);

    let mut advice = Vec::new();

    if !exists {
        advice.push(Advice {
            message: "Le préfixe n'existe pas encore : lance le jeu une fois depuis Steam.".into(),
            install: None,
            blocking: true,
        });
        return Ok(PrefixReport {
            exists,
            path: prefix,
            proton: proton_name,
            proton_is_ge,
            wine_mono,
            installed,
            windows_version: None,
            advice,
        });
    }

    if !has_dotnet {
        advice.push(Advice {
            message: if wine_mono {
                "Wine-Mono remplace .NET dans ce préfixe. La plupart des trainers sont \
                 écrits en .NET et ne démarrent pas dessus : installe le vrai framework."
                    .into()
            } else {
                "Aucun .NET Framework installé dans ce préfixe. La plupart des trainers \
                 en ont besoin."
                    .into()
            },
            install: Some(Component::Dotnet48),
            blocking: true,
        });
        advice.push(Advice {
            message: "Si le trainer refuse encore de démarrer, essaie .NET 4.0 : certains \
                      trainers anciens n'acceptent que cette version."
                .into(),
            install: Some(Component::Dotnet40),
            blocking: false,
        });
    }

    // « 10.0 » et « 6.1 » (Windows 7) conviennent ; en deçà, .NET récent refuse.
    if let Some(version) = version.as_deref() {
        let ancien = version.starts_with("5.") || version.starts_with("6.0");
        if ancien {
            advice.push(Advice {
                message: format!(
                    "Le préfixe annonce Windows {version} : les trainers .NET récents \
                     exigent Windows 7 ou plus. Corrige avec « protontricks {} win10 ».",
                    game.app_id
                ),
                install: None,
                blocking: true,
            });
        }
    }

    if !proton_is_ge {
        advice.push(Advice {
            message: format!(
                "Ce jeu utilise {}. GE-Proton est réputé plus permissif pour les \
                 modifications mémoire : à essayer si le trainer démarre mais n'agit pas.",
                proton_name.as_deref().unwrap_or("Proton standard")
            ),
            install: None,
            blocking: false,
        });
    }

    advice.push(Advice {
        message: "Si le trainer ne parvient pas à s'attacher, ajoute \
                  PROTON_NO_ESYNC=1 PROTON_NO_FSYNC=1 %command% aux options de \
                  lancement du jeu dans Steam."
            .into(),
        install: None,
        blocking: false,
    });

    Ok(PrefixReport {
        exists,
        path: prefix,
        proton: proton_name,
        proton_is_ge,
        wine_mono,
        installed,
        windows_version: version,
        advice,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_components_to_winetricks_verbs() {
        assert_eq!(Component::Dotnet48.verb(), "dotnet48");
        assert_eq!(Component::Dotnet40.verb(), "dotnet40");
        assert_eq!(Component::Corefonts.verb(), "corefonts");
    }

    #[test]
    fn reads_installed_verbs_from_the_prefix_log() {
        let dir = std::env::temp_dir().join(format!("archmod-prefix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        std::fs::write(dir.join("winetricks.log"), "dotnet48\ncorefonts\n\n").expect("log");

        let verbs = installed_verbs(&dir);
        assert_eq!(verbs, vec!["dotnet48".to_string(), "corefonts".to_string()]);
        assert!(verbs.iter().any(|verb| verb.starts_with("dotnet")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_the_declared_windows_version() {
        let dir = std::env::temp_dir().join(format!("archmod-reg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        std::fs::write(
            dir.join("system.reg"),
            "[Software\\Wow6432Node\\Microsoft\\Windows NT\\CurrentVersion] 1\n\
             \"CurrentVersion\"=\"5.1\"\n\
             [Software\\Microsoft\\Windows NT\\CurrentVersion] 2\n\
             \"CSDVersion\"=\"\"\n\
             \"CurrentVersion\"=\"10.0\"\n",
        )
        .expect("registre");

        // Le miroir Wow6432Node ne doit pas être confondu avec la vraie clé.
        assert_eq!(windows_version(&dir).as_deref(), Some("10.0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_log_means_nothing_installed() {
        assert!(installed_verbs(std::path::Path::new("/inexistant")).is_empty());
    }
}
