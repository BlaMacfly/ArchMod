//! Profils de trainer : le format d'échange de la communauté.
//!
//! Un profil décrit, pour **une version précise d'un jeu**, la liste des
//! options qu'ArchMod doit afficher et comment atteindre chaque valeur en
//! mémoire. C'est ce que produit quelqu'un qui a cherché les adresses avec
//! GameConqueror ou Cheat Engine, et ce que consomme le joueur qui veut
//! seulement cliquer sur un interrupteur.
//!
//! Le format est volontairement lisible et modifiable à la main : il a vocation
//! à vivre dans un dépôt git, à être relu en pull request et validé par la CI.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cheat_table::{AddressBase, CheatTable, Readiness, ValueType};
use crate::engine::Value;
use crate::error::{Result, TuxError};

/// Version du format. Toute évolution incompatible l'incrémente.
pub const FORMAT_VERSION: u32 = 1;

/// Comment atteindre la valeur en mémoire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Anchor {
    /// `GameAssembly.dll+0x4194FD4` — simple, mais casse à chaque mise à jour.
    Module { module: String, offset: i64 },
    /// Motif d'octets recherché à l'exécution : survit souvent aux mises à jour
    /// mineures, c'est la forme à privilégier.
    Aob {
        module: String,
        pattern: String,
        /// Décalage à l'intérieur du motif.
        #[serde(default)]
        offset: i64,
        /// Correspondance à retenir quand le motif en a plusieurs.
        #[serde(default)]
        occurrence: usize,
    },
}

/// Recette complète d'une adresse : ancrage puis chaîne de pointeurs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressRecipe {
    pub anchor: Anchor,
    /// Lire le pointeur rangé à l'adresse d'ancrage avant d'appliquer les
    /// décalages — l'équivalent des crochets de Cheat Engine.
    #[serde(default)]
    pub dereference: bool,
    /// Décalages successifs, du plus externe au plus interne, comme dans un
    /// fichier `.CT`.
    #[serde(default)]
    pub offsets: Vec<i64>,
}

/// Nature de l'option, qui détermine le contrôle affiché.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "control")]
pub enum Control {
    /// Interrupteur : gèle la valeur tant qu'il est actif.
    Toggle { frozen: Value },
    /// Curseur ou champ numérique, écrit puis gelé.
    Number {
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default)]
        default: Option<f64>,
        /// Geler la valeur après l'avoir écrite.
        #[serde(default = "default_true")]
        freeze: bool,
    },
    /// Bouton : écrit une fois, sans gel.
    Action { value: Value },
    /// Simple affichage : la valeur est lue et rafraîchie, jamais écrite.
    ///
    /// Indispensable pour les statistiques, et pour toute adresse qui pointe
    /// vers une section de code — lisible, mais que le jeu n'autorise pas à
    /// modifier.
    Display,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerOption {
    /// Identifiant stable, utilisé pour l'état et les raccourcis.
    pub id: String,
    /// Section de l'interface : « Joueur », « Inventaire », « Monde »…
    pub category: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub value_type: ValueType,
    pub control: Control,
    pub address: AddressRecipe,
    /// Raccourci global souhaité, par exemple « Numpad1 ».
    #[serde(default)]
    pub hotkey: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(default = "default_format")]
    pub format: u32,
    pub app_id: u32,
    pub game: String,
    /// Build Steam pour lequel les adresses ont été relevées.
    #[serde(default)]
    pub build_id: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    pub options: Vec<TrainerOption>,
}

fn default_format() -> u32 {
    FORMAT_VERSION
}

/// Correspondance entre un profil et le jeu réellement installé.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state", content = "detail")]
pub enum BuildMatch {
    /// Même build : les adresses sont valides.
    Exact,
    /// Le jeu a été mis à jour depuis l'écriture du profil.
    Outdated { profile: String, installed: String },
    /// Impossible de comparer, faute d'information de part ou d'autre.
    Unknown,
}

impl Profile {
    /// Vérifie la cohérence interne avant enregistrement ou publication.
    pub fn validate(&self) -> Result<()> {
        let invalid = |detail: String| TuxError::Profile { detail };

        if self.format > FORMAT_VERSION {
            return Err(invalid(format!(
                "format {} écrit par une version plus récente d'ArchMod",
                self.format
            )));
        }
        if self.game.trim().is_empty() {
            return Err(invalid("le nom du jeu est vide".into()));
        }
        if self.options.is_empty() {
            return Err(invalid("le profil ne contient aucune option".into()));
        }

        let mut seen = BTreeSet::new();
        for option in &self.options {
            if option.id.trim().is_empty() {
                return Err(invalid(format!(
                    "l'option « {} » n'a pas d'identifiant",
                    option.name
                )));
            }
            if !seen.insert(option.id.as_str()) {
                return Err(invalid(format!(
                    "identifiant « {} » utilisé deux fois",
                    option.id
                )));
            }
            if option.name.trim().is_empty() {
                return Err(invalid(format!(
                    "l'option « {} » n'a pas de nom",
                    option.id
                )));
            }
            // Un motif invalide ne se découvrirait qu'au lancement, trop tard.
            if let Anchor::Aob { pattern, .. } = &option.address.anchor {
                crate::memory::Pattern::parse(pattern)
                    .map_err(|error| invalid(format!("option « {} » : {error}", option.id)))?;
            }
        }
        Ok(())
    }

    /// Confronte le profil au build réellement installé.
    pub fn matches_build(&self, installed: Option<&str>) -> BuildMatch {
        match (self.build_id.as_deref(), installed) {
            (Some(profile), Some(installed)) if profile == installed => BuildMatch::Exact,
            (Some(profile), Some(installed)) => BuildMatch::Outdated {
                profile: profile.to_string(),
                installed: installed.to_string(),
            },
            _ => BuildMatch::Unknown,
        }
    }

    /// Catégories dans l'ordre de première apparition, pour l'affichage.
    pub fn categories(&self) -> Vec<String> {
        let mut categories: Vec<String> = Vec::new();
        for option in &self.options {
            if !categories.iter().any(|known| known == &option.category) {
                categories.push(option.category.clone());
            }
        }
        categories
    }

    pub fn parse(json: &str) -> Result<Self> {
        let profile: Profile = serde_json::from_str(json).map_err(|error| TuxError::Profile {
            detail: error.to_string(),
        })?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|error| TuxError::Profile {
            detail: error.to_string(),
        })
    }

    /// Nom de fichier canonique, tel qu'attendu dans le dépôt de profils.
    pub fn file_name(&self) -> String {
        format!("{}.json", self.build_id.as_deref().unwrap_or("sans-build"))
    }

    /// Dossier canonique du jeu dans le dépôt : `<appid>-<nom-simplifié>`.
    pub fn directory_name(&self) -> String {
        let slug: String = self
            .game
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        let slug = slug.trim_matches('-').replace("--", "-");
        format!("{}-{}", self.app_id, slug)
    }
}

/// Une entrée de table qu'ArchMod n'a pas su convertir, et pourquoi.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub description: String,
    pub reason: String,
}

/// Identifiant stable dérivé d'un libellé.
fn slugify(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    let mut compact = String::with_capacity(slug.len());
    let mut previous_dash = false;
    for c in slug.chars() {
        if c == '-' && previous_dash {
            continue;
        }
        previous_dash = c == '-';
        compact.push(c);
    }
    if compact.is_empty() {
        "option".into()
    } else {
        compact
    }
}

/// Convertit une table Cheat Engine en profil ArchMod.
///
/// Deux différences de convention doivent être franchies, et c'est là que se
/// joue la justesse des adresses :
///
/// 1. **L'ordre des décalages.** Dans un `.CT`, le premier décalage listé est
///    appliqué en dernier ; dans un profil, ils sont appliqués dans l'ordre de
///    lecture. La liste est donc renversée, et le décalage porté par l'adresse
///    de base vient en tête.
/// 2. **Les symboles.** Une entrée qui vise `[player]+184` est inexploitable en
///    l'état, mais si un script de la table définit `player` par un
///    `aobscanmodule`, on peut remplacer le symbole par ce motif — et l'entrée
///    devient utilisable sans moteur d'auto-assembleur.
pub fn from_cheat_table(
    table: &CheatTable,
    app_id: u32,
    game: &str,
    build_id: Option<String>,
) -> (Profile, Vec<Skipped>) {
    let scans = table.scans();
    let mut options = Vec::new();
    let mut skipped = Vec::new();
    let mut used_ids: BTreeSet<String> = BTreeSet::new();

    for entry in table.flatten() {
        let refuse = |reason: &str, skipped: &mut Vec<Skipped>| {
            skipped.push(Skipped {
                description: entry.description.clone(),
                reason: reason.to_string(),
            });
        };

        if let Readiness::Unsupported(reason) = entry.readiness() {
            // Les titres de section n'ont pas à être signalés comme des échecs.
            if !entry.group_header {
                refuse(&reason, &mut skipped);
            }
            continue;
        }
        if entry.value_type.size().is_none() {
            refuse("type de valeur non pris en charge", &mut skipped);
            continue;
        }

        let (anchor, dereference, base_offset) = match entry.base.as_ref() {
            Some(AddressBase::Module { module, offset }) => (
                Anchor::Module {
                    module: module.clone(),
                    offset: 0,
                },
                false,
                *offset,
            ),
            Some(AddressBase::Symbol {
                symbol,
                offset,
                dereference,
            }) => {
                // Le symbole n'a de sens que si un scan de la table le produit.
                let Some(scan) = scans.iter().find(|scan| &scan.symbol == symbol) else {
                    refuse(
                        &format!(
                            "dépend du symbole « {symbol} », produit par un script d'auto-assembleur"
                        ),
                        &mut skipped,
                    );
                    continue;
                };
                (
                    Anchor::Aob {
                        module: scan.module.clone(),
                        pattern: scan.pattern.clone(),
                        offset: 0,
                        occurrence: 0,
                    },
                    *dereference,
                    *offset,
                )
            }
            Some(AddressBase::Absolute { .. }) => {
                refuse(
                    "adresse absolue : elle ne survivrait pas au prochain lancement",
                    &mut skipped,
                );
                continue;
            }
            None => {
                refuse("aucune adresse", &mut skipped);
                continue;
            }
        };

        // Renversement des décalages, décalage de base en tête.
        let mut offsets = vec![base_offset];
        offsets.extend(entry.offsets.iter().rev().copied());

        let mut id = slugify(&entry.description);
        let mut suffix = 2;
        while !used_ids.insert(id.clone()) {
            id = format!("{}-{suffix}", slugify(&entry.description));
            suffix += 1;
        }

        options.push(TrainerOption {
            id,
            category: "Importé".into(),
            name: entry.description.clone(),
            description: None,
            value_type: entry.value_type.clone(),
            control: Control::Number {
                min: None,
                max: None,
                default: None,
                freeze: true,
            },
            address: AddressRecipe {
                anchor,
                dereference,
                offsets,
            },
            hotkey: None,
        });
    }

    (
        Profile {
            format: FORMAT_VERSION,
            app_id,
            game: game.to_string(),
            build_id,
            author: None,
            notes: Some("Importé d'une table Cheat Engine.".into()),
            options,
        },
        skipped,
    )
}

/// Dossier des profils installés localement.
pub fn profiles_dir() -> Result<PathBuf> {
    let dir = crate::vault::config_dir()?.join("profiles");
    std::fs::create_dir_all(&dir).map_err(|source| TuxError::ConfigWrite {
        path: dir.clone(),
        source,
    })?;
    Ok(dir)
}

pub fn load_from(path: &Path) -> Result<Profile> {
    let raw = std::fs::read_to_string(path).map_err(|source| TuxError::io(path, source))?;
    Profile::parse(&raw)
}

pub fn save(profile: &Profile) -> Result<PathBuf> {
    profile.validate()?;
    let dir = profiles_dir()?.join(profile.directory_name());
    std::fs::create_dir_all(&dir).map_err(|source| TuxError::ConfigWrite {
        path: dir.clone(),
        source,
    })?;

    let path = dir.join(profile.file_name());
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, profile.to_json()?).map_err(|source| TuxError::ConfigWrite {
        path: temporary.clone(),
        source,
    })?;
    std::fs::rename(&temporary, &path).map_err(|source| TuxError::ConfigWrite {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Profils installés pour un jeu, du plus pertinent au moins pertinent.
pub fn for_game(app_id: u32, build_id: Option<&str>) -> Result<Vec<Profile>> {
    let dir = profiles_dir()?;
    let mut profiles = Vec::new();

    for entry in std::fs::read_dir(&dir)
        .map_err(|e| TuxError::io(&dir, e))?
        .flatten()
    {
        if !entry.path().is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(entry.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match load_from(&path) {
                Ok(profile) if profile.app_id == app_id => profiles.push(profile),
                Ok(_) => {}
                Err(error) => eprintln!("[archmod] profil ignoré ({}) : {error}", path.display()),
            }
        }
    }

    // Le profil du build installé passe devant les autres.
    profiles.sort_by_key(|profile| match profile.matches_build(build_id) {
        BuildMatch::Exact => 0,
        BuildMatch::Unknown => 1,
        BuildMatch::Outdated { .. } => 2,
    });
    Ok(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Profile {
        Profile {
            format: FORMAT_VERSION,
            app_id: 3527290,
            game: "PEAK".into(),
            build_id: Some("24720181".into()),
            author: Some("commu".into()),
            notes: None,
            options: vec![
                TrainerOption {
                    id: "unlimited-stamina".into(),
                    category: "Joueur".into(),
                    name: "Endurance infinie".into(),
                    description: None,
                    value_type: ValueType::Float,
                    control: Control::Toggle {
                        frozen: Value::Float(100.0),
                    },
                    address: AddressRecipe {
                        anchor: Anchor::Aob {
                            module: "GameAssembly.dll".into(),
                            pattern: "F3 0F 11 ?? 28 48 85 C0".into(),
                            offset: 4,
                            occurrence: 0,
                        },
                        dereference: true,
                        offsets: vec![0x1C],
                    },
                    hotkey: Some("Numpad1".into()),
                },
                TrainerOption {
                    id: "lantern-fuel".into(),
                    category: "Inventaire".into(),
                    name: "Carburant de lanterne illimité".into(),
                    description: None,
                    value_type: ValueType::FourBytes,
                    control: Control::Number {
                        min: Some(0.0),
                        max: Some(999.0),
                        default: Some(100.0),
                        freeze: true,
                    },
                    address: AddressRecipe {
                        anchor: Anchor::Module {
                            module: "GameAssembly.dll".into(),
                            offset: 0x4194FD4,
                        },
                        dereference: false,
                        offsets: vec![],
                    },
                    hotkey: None,
                },
            ],
        }
    }

    #[test]
    fn converts_a_cheat_table_and_reverses_offset_order() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<CheatTable CheatEngineTableVersion="45">
  <CheatEntries>
    <CheatEntry>
      <Description>"Enable"</Description>
      <VariableType>Auto Assembler Script</VariableType>
      <AssemblerScript>aobscanmodule(player,GameAssembly.dll,48 8B 89 40 01)
registersymbol(player)</AssemblerScript>
    </CheatEntry>
    <CheatEntry>
      <Description>"Vie"</Description>
      <VariableType>Float</VariableType>
      <Address>[player]+190</Address>
      <Offsets><Offset>C0</Offset><Offset>18</Offset></Offsets>
    </CheatEntry>
    <CheatEntry>
      <Description>"Or"</Description>
      <VariableType>4 Bytes</VariableType>
      <Address>GameAssembly.dll+4194FD4</Address>
    </CheatEntry>
    <CheatEntry>
      <Description>"Orphelin"</Description>
      <VariableType>Float</VariableType>
      <Address>[inconnu]+10</Address>
    </CheatEntry>
  </CheatEntries>
</CheatTable>"#;

        let table = CheatTable::parse(xml).expect("table");
        let (profile, skipped) = from_cheat_table(&table, 1, "Jeu", Some("42".into()));

        assert_eq!(profile.options.len(), 2, "deux entrées convertibles");

        // Le symbole est remplacé par le motif qui le produit.
        let vie = &profile.options[0];
        assert_eq!(vie.name, "Vie");
        assert!(
            vie.address.dereference,
            "les crochets imposent un déréférencement"
        );
        assert_eq!(
            vie.address.anchor,
            Anchor::Aob {
                module: "GameAssembly.dll".into(),
                pattern: "48 8B 89 40 01".into(),
                offset: 0,
                occurrence: 0
            }
        );
        // [player]+190 avec les décalages CE [C0, 18] devient, dans notre ordre,
        // 190 puis 18 puis C0.
        assert_eq!(vie.address.offsets, vec![0x190, 0x18, 0xC0]);

        assert_eq!(
            profile.options[1].address.anchor,
            Anchor::Module {
                module: "GameAssembly.dll".into(),
                offset: 0
            }
        );
        assert_eq!(profile.options[1].address.offsets, vec![0x4194FD4]);

        // Le script et l'entrée au symbole inconnu sont écartés, avec la raison.
        assert_eq!(skipped.len(), 2);
        assert!(skipped.iter().any(|s| s.reason.contains("inconnu")));
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn round_trips_through_json() {
        let profile = sample();
        let json = profile.to_json().expect("sérialisation");
        assert_eq!(Profile::parse(&json).expect("relecture"), profile);
    }

    #[test]
    fn rejects_duplicate_option_ids() {
        let mut profile = sample();
        profile.options[1].id = profile.options[0].id.clone();
        let error = profile.validate().expect_err("doit refuser");
        assert!(error.to_string().contains("deux fois"));
    }

    #[test]
    fn rejects_invalid_byte_patterns() {
        let mut profile = sample();
        profile.options[0].address.anchor = Anchor::Aob {
            module: "GameAssembly.dll".into(),
            pattern: "?? ??".into(),
            offset: 0,
            occurrence: 0,
        };
        assert!(
            profile.validate().is_err(),
            "un motif tout en jokers est refusé"
        );
    }

    #[test]
    fn rejects_empty_profiles() {
        let mut profile = sample();
        profile.options.clear();
        assert!(profile.validate().is_err());
    }

    #[test]
    fn compares_build_identifiers() {
        let profile = sample();
        assert_eq!(profile.matches_build(Some("24720181")), BuildMatch::Exact);
        assert_eq!(
            profile.matches_build(Some("24999999")),
            BuildMatch::Outdated {
                profile: "24720181".into(),
                installed: "24999999".into()
            }
        );
        assert_eq!(profile.matches_build(None), BuildMatch::Unknown);
    }

    #[test]
    fn groups_options_by_category_in_order() {
        assert_eq!(sample().categories(), vec!["Joueur", "Inventaire"]);
    }

    #[test]
    fn builds_canonical_repository_paths() {
        let profile = sample();
        assert_eq!(profile.directory_name(), "3527290-peak");
        assert_eq!(profile.file_name(), "24720181.json");
    }

    /// Valide tous les profils versionnés dans le dépôt.
    ///
    /// Ce test est le portier des contributions : la CI le joue à chaque pull
    /// request, donc un profil mal formé ne peut pas être fusionné.
    #[test]
    fn every_profile_in_the_repository_is_valid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles");
        if !root.is_dir() {
            return; // aucun profil publié pour l'instant
        }

        let mut checked = 0;
        let mut walk = vec![root];
        while let Some(dir) = walk.pop() {
            for entry in std::fs::read_dir(&dir)
                .expect("lecture du dossier")
                .flatten()
            {
                let path = entry.path();
                if path.is_dir() {
                    walk.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let profile =
                    load_from(&path).unwrap_or_else(|error| panic!("{} : {error}", path.display()));

                // Le chemin doit correspondre au contenu, sinon l'application
                // ne retrouvera pas le profil du build installé.
                let dossier = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str());
                assert_eq!(
                    dossier,
                    Some(profile.directory_name().as_str()),
                    "{} devrait être dans {}",
                    path.display(),
                    profile.directory_name()
                );
                assert_eq!(
                    path.file_name().and_then(|n| n.to_str()),
                    Some(profile.file_name().as_str()),
                    "{} devrait s'appeler {}",
                    path.display(),
                    profile.file_name()
                );
                checked += 1;
            }
        }
        println!("{checked} profil(s) validé(s)");
    }

    #[test]
    fn tolerates_minimal_json_from_a_contributor() {
        // Ce qu'une personne écrirait à la main : pas de champs facultatifs.
        let json = r#"{
          "appId": 1898300,
          "game": "ASKA",
          "options": [{
            "id": "wood",
            "category": "Ressources",
            "name": "Bois",
            "valueType": { "kind": "fourBytes" },
            "control": { "control": "number" },
            "address": {
              "anchor": { "kind": "module", "module": "GameAssembly.dll", "offset": 123 }
            }
          }]
        }"#;
        let profile = Profile::parse(json).expect("profil minimal accepté");
        assert_eq!(profile.format, FORMAT_VERSION);
        assert_eq!(profile.options[0].address.offsets.len(), 0);
        assert!(matches!(
            profile.options[0].control,
            Control::Number { freeze: true, .. }
        ));
    }
}
