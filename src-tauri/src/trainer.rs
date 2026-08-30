//! Exécution d'un profil sur un jeu en cours.
//!
//! C'est le socle commun aux deux publics d'ArchMod : le joueur qui bascule un
//! interrupteur, et l'auteur qui met au point une adresse dans l'atelier. Les
//! deux passent par les mêmes opérations — résoudre, lire, écrire, geler — la
//! seule différence étant que l'atelier travaille sur une recette encore
//! provisoire.

use std::collections::HashMap;

use serde::Serialize;
use tokio::sync::Mutex;

use crate::cheat_table::ValueType;
use crate::engine::{Freezer, Session, Value};
use crate::error::{Result, TuxError};
use crate::injector;
use crate::profile::{AddressRecipe, Control, Profile, TrainerOption};
use crate::steam_scanner::SteamGame;

/// État d'une option après résolution ou action.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionStatus {
    pub id: String,
    /// Adresse résolue, absente si la recette a échoué.
    pub address: Option<u64>,
    /// Valeur lue à l'instant de la résolution.
    pub value: Option<Value>,
    /// L'option est active : valeur écrite, et gelée le cas échéant.
    pub active: bool,
    /// Raison de l'échec, formulée pour l'utilisateur.
    pub error: Option<String>,
}

/// Résultat de l'activation d'un profil sur un jeu.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationReport {
    pub app_id: u32,
    pub pid: u32,
    pub options: Vec<OptionStatus>,
    /// Options dont l'adresse a été trouvée.
    pub resolved: usize,
    /// Options en échec — un profil périmé les accumule.
    pub failed: usize,
}

/// Un profil chargé et rattaché à un processus.
struct GameRuntime {
    pid: u32,
    profile: Profile,
    session: Session,
    freezer: Freezer,
    /// Adresses résolues, par identifiant d'option.
    addresses: HashMap<String, u64>,
    active: HashMap<String, Value>,
}

impl GameRuntime {
    fn option(&self, id: &str) -> Result<&TrainerOption> {
        self.profile
            .options
            .iter()
            .find(|option| option.id == id)
            .ok_or_else(|| TuxError::Internal(format!("option « {id} » absente du profil")))
    }

    fn status_of(&self, option: &TrainerOption) -> OptionStatus {
        OptionStatus {
            id: option.id.clone(),
            address: self.addresses.get(&option.id).copied(),
            value: self.active.get(&option.id).copied(),
            active: self.active.contains_key(&option.id),
            error: None,
        }
    }

    fn report(&self) -> ActivationReport {
        let options: Vec<OptionStatus> = self
            .profile
            .options
            .iter()
            .map(|option| self.status_of(option))
            .collect();
        let resolved = options.iter().filter(|o| o.address.is_some()).count();
        ActivationReport {
            app_id: self.profile.app_id,
            pid: self.pid,
            failed: options.len() - resolved,
            resolved,
            options,
        }
    }
}

/// Profils actifs, un par jeu.
#[derive(Default)]
pub struct Runtimes {
    inner: Mutex<HashMap<u32, GameRuntime>>,
}

/// Valeur à écrire pour une option, selon son contrôle et la saisie éventuelle.
fn value_for(option: &TrainerOption, requested: Option<Value>) -> Result<(Value, bool)> {
    match &option.control {
        Control::Display => Err(TuxError::Internal(format!(
            "« {} » est une valeur affichée, pas modifiable",
            option.name
        ))),
        Control::Toggle { frozen } => Ok((requested.unwrap_or(*frozen), true)),
        Control::Action { value } => Ok((requested.unwrap_or(*value), false)),
        Control::Number {
            default, freeze, ..
        } => {
            let value = match requested {
                Some(value) => value,
                None => {
                    let number = default.ok_or_else(|| {
                        TuxError::Internal(format!(
                            "l'option « {} » attend une valeur",
                            option.name
                        ))
                    })?;
                    number_as(&option.value_type, number)?
                }
            };
            Ok((value, *freeze))
        }
    }
}

/// Convertit un nombre du profil vers le type déclaré de l'option.
fn number_as(value_type: &ValueType, number: f64) -> Result<Value> {
    Ok(match value_type {
        ValueType::Byte => Value::Byte(number as u8),
        ValueType::TwoBytes => Value::TwoBytes(number as u16),
        ValueType::FourBytes => Value::FourBytes(number as i32),
        ValueType::EightBytes => Value::EightBytes(number as i64),
        ValueType::Float => Value::Float(number as f32),
        ValueType::Double => Value::Double(number),
        other => return Err(TuxError::Internal(format!("type {other:?} non modifiable"))),
    })
}

impl Runtimes {
    /// Charge un profil et résout toutes ses adresses.
    ///
    /// Une option qui échoue n'empêche pas les autres : sur un profil écrit
    /// pour un build antérieur, une partie des adresses reste souvent valable.
    pub async fn activate(&self, game: &SteamGame, profile: Profile) -> Result<ActivationReport> {
        profile.validate()?;
        let pid = injector::game_process(game).ok_or(TuxError::GameNotRunning {
            name: game.name.clone(),
        })?;

        // Résoudre un profil déclenche autant de balayages de motifs qu'il y a
        // d'options : plusieurs secondes sur un gros module. Le travail part sur
        // un fil dédié pour que l'interface reste vivante.
        let entries = profile.options.clone();
        let (session, addresses, options) = tokio::task::spawn_blocking(move || {
            let mut session = Session::new(pid);
            let mut addresses = HashMap::new();
            let mut options = Vec::new();

            for option in &entries {
                let mut status = OptionStatus {
                    id: option.id.clone(),
                    address: None,
                    value: None,
                    active: false,
                    error: None,
                };
                match session.read_recipe(&option.address, &option.value_type) {
                    Ok((address, value)) => {
                        addresses.insert(option.id.clone(), address);
                        status.address = Some(address);
                        status.value = Some(value);
                    }
                    Err(error) => status.error = Some(error.to_string()),
                }
                options.push(status);
            }
            (session, addresses, options)
        })
        .await
        .map_err(|error| TuxError::Internal(error.to_string()))?;

        let resolved = options.iter().filter(|o| o.address.is_some()).count();
        let report = ActivationReport {
            app_id: profile.app_id,
            pid,
            failed: options.len() - resolved,
            resolved,
            options,
        };

        let mut runtimes = self.inner.lock().await;
        // Un profil remplacé libère les gels du précédent.
        if let Some(mut previous) = runtimes.remove(&profile.app_id) {
            previous.freezer.stop().await;
        }
        runtimes.insert(
            profile.app_id,
            GameRuntime {
                pid,
                profile,
                session,
                freezer: Freezer::new(pid),
                addresses,
                active: HashMap::new(),
            },
        );
        Ok(report)
    }

    pub async fn report(&self, app_id: u32) -> Option<ActivationReport> {
        self.inner.lock().await.get(&app_id).map(|r| r.report())
    }

    pub async fn profile(&self, app_id: u32) -> Option<Profile> {
        self.inner
            .lock()
            .await
            .get(&app_id)
            .map(|runtime| runtime.profile.clone())
    }

    /// Active une option : écrit la valeur, et la gèle si le contrôle l'exige.
    pub async fn set(
        &self,
        app_id: u32,
        option_id: &str,
        requested: Option<Value>,
    ) -> Result<OptionStatus> {
        let mut runtimes = self.inner.lock().await;
        let runtime = runtimes
            .get_mut(&app_id)
            .ok_or_else(|| TuxError::Internal("aucun profil actif pour ce jeu".into()))?;

        let option = runtime.option(option_id)?.clone();
        let (value, freeze) = value_for(&option, requested)?;

        // On recalcule l'adresse : une chaîne de pointeurs peut avoir bougé
        // depuis l'activation, par exemple après un changement de niveau.
        let address = runtime.session.resolve_recipe(&option.address)?;
        runtime.addresses.insert(option.id.clone(), address);

        crate::memory::write(runtime.pid, address, &value.to_bytes())?;
        if freeze {
            runtime
                .freezer
                .freeze(option.id.clone(), address, value)
                .await;
        }
        runtime.active.insert(option.id.clone(), value);

        Ok(OptionStatus {
            id: option.id,
            address: Some(address),
            value: Some(value),
            active: true,
            error: None,
        })
    }

    /// Désactive une option : le gel cesse, la valeur reste telle quelle.
    pub async fn clear(&self, app_id: u32, option_id: &str) -> Result<bool> {
        let mut runtimes = self.inner.lock().await;
        let runtime = runtimes
            .get_mut(&app_id)
            .ok_or_else(|| TuxError::Internal("aucun profil actif pour ce jeu".into()))?;
        runtime.active.remove(option_id);
        Ok(runtime.freezer.unfreeze(option_id).await)
    }

    /// Relit toutes les options résolues : c'est ce qui anime les valeurs
    /// affichées, et ce qui révèle qu'une adresse est devenue caduque.
    pub async fn refresh(&self, app_id: u32) -> Result<ActivationReport> {
        let mut runtimes = self.inner.lock().await;
        let runtime = runtimes
            .get_mut(&app_id)
            .ok_or_else(|| TuxError::Internal("aucun profil actif pour ce jeu".into()))?;

        let options = runtime.profile.options.clone();
        let mut statuses = Vec::new();
        for option in &options {
            let mut status = runtime.status_of(option);
            match runtime
                .session
                .read_recipe(&option.address, &option.value_type)
            {
                Ok((address, value)) => {
                    runtime.addresses.insert(option.id.clone(), address);
                    status.address = Some(address);
                    status.value = Some(value);
                    status.error = None;
                }
                Err(error) => {
                    status.error = Some(error.to_string());
                    status.address = None;
                }
            }
            statuses.push(status);
        }

        let resolved = statuses.iter().filter(|s| s.address.is_some()).count();
        Ok(ActivationReport {
            app_id,
            pid: runtime.pid,
            failed: statuses.len() - resolved,
            resolved,
            options: statuses,
        })
    }

    pub async fn deactivate(&self, app_id: u32) -> bool {
        let mut runtimes = self.inner.lock().await;
        match runtimes.remove(&app_id) {
            Some(mut runtime) => {
                runtime.freezer.stop().await;
                true
            }
            None => false,
        }
    }

    /// Essai en direct d'une recette encore provisoire : le cœur de l'atelier.
    ///
    /// Ne nécessite aucun profil actif — on travaille sur un jeu, pas sur une
    /// configuration enregistrée.
    pub async fn probe(
        &self,
        game: &SteamGame,
        recipe: &AddressRecipe,
        value_type: &ValueType,
    ) -> Result<OptionStatus> {
        let pid = injector::game_process(game).ok_or(TuxError::GameNotRunning {
            name: game.name.clone(),
        })?;

        // Une session éphémère : l'atelier ne doit pas perturber un profil actif.
        let mut session = Session::new(pid);
        let (address, value) = session.read_recipe(recipe, value_type)?;
        Ok(OptionStatus {
            id: "essai".into(),
            address: Some(address),
            value: Some(value),
            active: false,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{AddressRecipe, Anchor};

    fn option(control: Control, value_type: ValueType) -> TrainerOption {
        TrainerOption {
            id: "essai".into(),
            category: "Test".into(),
            name: "Essai".into(),
            description: None,
            value_type,
            control,
            address: AddressRecipe {
                anchor: Anchor::Module {
                    module: "jeu.exe".into(),
                    offset: 0,
                },
                dereference: false,
                offsets: vec![],
            },
            hotkey: None,
        }
    }

    #[test]
    fn a_toggle_freezes_its_declared_value() {
        let option = option(
            Control::Toggle {
                frozen: Value::Float(100.0),
            },
            ValueType::Float,
        );
        let (value, freeze) = value_for(&option, None).expect("valeur");
        assert_eq!(value, Value::Float(100.0));
        assert!(freeze, "un interrupteur gèle");
    }

    #[test]
    fn an_action_writes_once_without_freezing() {
        let option = option(
            Control::Action {
                value: Value::FourBytes(999),
            },
            ValueType::FourBytes,
        );
        let (value, freeze) = value_for(&option, None).expect("valeur");
        assert_eq!(value, Value::FourBytes(999));
        assert!(!freeze, "un bouton n'impose rien dans la durée");
    }

    #[test]
    fn a_number_uses_the_users_input_over_its_default() {
        let option = option(
            Control::Number {
                min: None,
                max: None,
                default: Some(50.0),
                freeze: true,
            },
            ValueType::FourBytes,
        );
        let (value, freeze) = value_for(&option, Some(Value::FourBytes(7))).expect("valeur");
        assert_eq!(value, Value::FourBytes(7));
        assert!(freeze);

        // Sans saisie, la valeur par défaut du profil est convertie au bon type.
        let (value, _) = value_for(&option, None).expect("défaut");
        assert_eq!(value, Value::FourBytes(50));
    }

    #[test]
    fn a_display_option_is_never_written() {
        let option = option(Control::Display, ValueType::FourBytes);
        assert!(
            value_for(&option, Some(Value::FourBytes(1))).is_err(),
            "même avec une valeur fournie, on n'écrit pas"
        );
    }

    #[test]
    fn a_number_without_default_or_input_is_refused() {
        let option = option(
            Control::Number {
                min: None,
                max: None,
                default: None,
                freeze: false,
            },
            ValueType::FourBytes,
        );
        assert!(value_for(&option, None).is_err());
    }

    #[test]
    fn converts_profile_numbers_to_the_declared_type() {
        assert_eq!(
            number_as(&ValueType::Float, 12.5).expect("float"),
            Value::Float(12.5)
        );
        assert_eq!(
            number_as(&ValueType::Byte, 255.0).expect("octet"),
            Value::Byte(255)
        );
        assert!(number_as(&ValueType::Text, 1.0).is_err());
    }
}
