//! Moteur d'options : relie une table Cheat Engine à un jeu en cours.
//!
//! Une session résout les symboles que la table réclame (scans de motifs),
//! calcule les adresses réelles des entrées, lit et écrit leurs valeurs, et
//! maintient un geleur pour les options du type « vie infinie » — qui ne sont
//! rien d'autre qu'une réécriture périodique de la même valeur.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::cheat_table::{AddressBase, CheatEntry, CheatTable, Readiness, ValueType};
use crate::error::{Result, TuxError};
use crate::memory::{self, Module, Pattern};

/// Cadence de réécriture des valeurs gelées.
const FREEZE_INTERVAL: Duration = Duration::from_millis(100);

/// Une valeur lue ou à écrire, typée comme dans la table.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "value")]
pub enum Value {
    Byte(u8),
    TwoBytes(u16),
    FourBytes(i32),
    EightBytes(i64),
    Float(f32),
    Double(f64),
}

impl Value {
    pub fn to_bytes(self) -> Vec<u8> {
        match self {
            Value::Byte(v) => v.to_le_bytes().to_vec(),
            Value::TwoBytes(v) => v.to_le_bytes().to_vec(),
            Value::FourBytes(v) => v.to_le_bytes().to_vec(),
            Value::EightBytes(v) => v.to_le_bytes().to_vec(),
            Value::Float(v) => v.to_le_bytes().to_vec(),
            Value::Double(v) => v.to_le_bytes().to_vec(),
        }
    }

    /// Décode selon le type déclaré par la table.
    pub fn from_bytes(value_type: &ValueType, bytes: &[u8]) -> Option<Self> {
        Some(match value_type {
            ValueType::Byte => Value::Byte(*bytes.first()?),
            ValueType::TwoBytes => {
                Value::TwoBytes(u16::from_le_bytes(bytes.get(..2)?.try_into().ok()?))
            }
            ValueType::FourBytes => {
                Value::FourBytes(i32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
            }
            ValueType::EightBytes => {
                Value::EightBytes(i64::from_le_bytes(bytes.get(..8)?.try_into().ok()?))
            }
            ValueType::Float => Value::Float(f32::from_le_bytes(bytes.get(..4)?.try_into().ok()?)),
            ValueType::Double => {
                Value::Double(f64::from_le_bytes(bytes.get(..8)?.try_into().ok()?))
            }
            _ => return None,
        })
    }

    /// Convertit une saisie utilisateur vers le type attendu.
    pub fn parse(value_type: &ValueType, input: &str) -> Result<Self> {
        let input = input.trim();
        let invalid = || TuxError::Internal(format!("valeur « {input} » invalide pour ce type"));
        Ok(match value_type {
            ValueType::Byte => Value::Byte(input.parse().map_err(|_| invalid())?),
            ValueType::TwoBytes => Value::TwoBytes(input.parse().map_err(|_| invalid())?),
            ValueType::FourBytes => Value::FourBytes(input.parse().map_err(|_| invalid())?),
            ValueType::EightBytes => Value::EightBytes(input.parse().map_err(|_| invalid())?),
            ValueType::Float => Value::Float(input.parse().map_err(|_| invalid())?),
            ValueType::Double => Value::Double(input.parse().map_err(|_| invalid())?),
            other => return Err(TuxError::Internal(format!("type {other:?} non modifiable"))),
        })
    }
}

/// Résultat d'un scan de motif déclaré par la table.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    pub symbol: String,
    pub module: String,
    pub pattern: String,
    /// Adresses trouvées ; le symbole prend la première, comme dans CE.
    pub matches: Vec<u64>,
    pub elapsed_ms: u64,
    /// Renseigné quand le scan n'a pas pu aboutir.
    pub error: Option<String>,
}

impl ScanOutcome {
    pub fn resolved(&self) -> Option<u64> {
        self.matches.first().copied()
    }
}

/// Session attachée à un processus de jeu.
pub struct Session {
    pid: u32,
    modules: HashMap<String, Module>,
    symbols: HashMap<String, u64>,
}

impl Session {
    pub fn new(pid: u32) -> Self {
        Self {
            pid,
            modules: HashMap::new(),
            symbols: HashMap::new(),
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn symbols(&self) -> &HashMap<String, u64> {
        &self.symbols
    }

    /// Déclare un symbole résolu autrement (par le futur auto-assembleur).
    pub fn define_symbol(&mut self, name: impl Into<String>, address: u64) {
        self.symbols.insert(name.into(), address);
    }

    fn module(&mut self, name: &str) -> Result<Module> {
        if let Some(module) = self.modules.get(name) {
            return Ok(module.clone());
        }
        let module = memory::find_module(self.pid, name)?;
        self.modules.insert(name.to_string(), module.clone());
        Ok(module)
    }

    /// Exécute tous les `aobscanmodule` de la table et enregistre les symboles.
    ///
    /// Un scan qui échoue n'interrompt pas les autres : une table écrite pour
    /// une autre version du jeu retrouve souvent une partie de ses motifs, et
    /// l'utilisateur mérite de savoir lesquels.
    pub fn run_scans(&mut self, table: &CheatTable) -> Vec<ScanOutcome> {
        let mut outcomes = Vec::new();

        for scan in table.scans() {
            let started = std::time::Instant::now();
            let mut outcome = ScanOutcome {
                symbol: scan.symbol.clone(),
                module: scan.module.clone(),
                pattern: scan.pattern.clone(),
                matches: Vec::new(),
                elapsed_ms: 0,
                error: None,
            };

            match Pattern::parse(&scan.pattern) {
                Ok(pattern) => match self.module(&scan.module) {
                    Ok(module) => match memory::scan_module(self.pid, &module, &pattern) {
                        Ok(matches) => outcome.matches = matches,
                        Err(error) => outcome.error = Some(error.to_string()),
                    },
                    Err(error) => outcome.error = Some(error.to_string()),
                },
                Err(error) => outcome.error = Some(error.to_string()),
            }

            outcome.elapsed_ms = started.elapsed().as_millis() as u64;
            if let Some(address) = outcome.resolved() {
                self.symbols.insert(outcome.symbol.clone(), address);
            }
            outcomes.push(outcome);
        }
        outcomes
    }

    /// Adresse de base d'une entrée, avant application des décalages.
    fn base_address(&mut self, entry: &CheatEntry) -> Result<u64> {
        match entry.base.as_ref() {
            Some(AddressBase::Absolute { address }) => Ok(*address),
            Some(AddressBase::Module { module, offset }) => {
                let module = self.module(module)?;
                Ok(module.base.wrapping_add(*offset as u64))
            }
            Some(AddressBase::Symbol {
                symbol,
                offset,
                dereference,
            }) => {
                let address = self.symbols.get(symbol).copied().ok_or_else(|| {
                    TuxError::SymbolUnresolved {
                        symbol: symbol.clone(),
                    }
                })?;
                // `[player]` lit le pointeur rangé à cette adresse ; `player`
                // désigne l'adresse elle-même.
                let address = if *dereference {
                    memory::read_u64(self.pid, address)?
                } else {
                    address
                };
                Ok(address.wrapping_add(*offset as u64))
            }
            None => Err(TuxError::Internal(format!(
                "l'entrée « {} » n'a pas d'adresse",
                entry.description
            ))),
        }
    }

    /// Adresse effective de la valeur, chaîne de pointeurs comprise.
    pub fn resolve(&mut self, entry: &CheatEntry) -> Result<u64> {
        if let Readiness::Unsupported(reason) = entry.readiness() {
            return Err(TuxError::Internal(format!(
                "entrée « {} » non exploitable : {reason}",
                entry.description
            )));
        }
        let base = self.base_address(entry)?;
        memory::resolve_pointer_in(self.pid, base, &entry.offsets)
    }

    pub fn read(&mut self, entry: &CheatEntry) -> Result<Value> {
        let size = entry.value_type.size().ok_or_else(|| {
            TuxError::Internal(format!(
                "type {:?} non lisible directement",
                entry.value_type
            ))
        })?;
        let address = self.resolve(entry)?;
        let mut buffer = vec![0u8; size];
        memory::read(self.pid, address, &mut buffer)?;

        Value::from_bytes(&entry.value_type, &buffer).ok_or_else(|| {
            TuxError::Internal(format!("décodage impossible de « {} »", entry.description))
        })
    }

    pub fn write(&mut self, entry: &CheatEntry, value: Value) -> Result<u64> {
        let address = self.resolve(entry)?;
        memory::write(self.pid, address, &value.to_bytes())?;
        Ok(address)
    }
}

// ---------------------------------------------------------------------------
// Gel de valeurs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrozenValue {
    /// Identifiant de l'option, tel qu'affiché dans l'interface.
    pub key: String,
    pub address: u64,
    pub value: Value,
}

/// Réécrit périodiquement les valeurs gelées dans le processus cible.
///
/// C'est ce qui transforme une simple écriture en option « infinie » : le jeu
/// remet la valeur à sa guise, on la réimpose dix fois par seconde.
pub struct Freezer {
    pid: u32,
    entries: Arc<Mutex<HashMap<String, FrozenValue>>>,
    task: Option<JoinHandle<()>>,
}

impl Freezer {
    pub fn new(pid: u32) -> Self {
        Self {
            pid,
            entries: Arc::new(Mutex::new(HashMap::new())),
            task: None,
        }
    }

    /// Démarre la boucle si elle ne tourne pas déjà.
    pub fn start(&mut self) {
        if self.task.is_some() {
            return;
        }
        let pid = self.pid;
        let entries = Arc::clone(&self.entries);

        self.task = Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(FREEZE_INTERVAL);
            loop {
                ticker.tick().await;
                let snapshot: Vec<FrozenValue> = entries.lock().await.values().cloned().collect();
                if snapshot.is_empty() {
                    continue;
                }
                for frozen in snapshot {
                    // Le jeu peut se fermer entre deux battements : on ignore
                    // l'échec, la disparition sera signalée par ailleurs.
                    let _ = memory::write(pid, frozen.address, &frozen.value.to_bytes());
                }
            }
        }));
    }

    pub async fn freeze(&mut self, key: impl Into<String>, address: u64, value: Value) {
        let key = key.into();
        self.entries.lock().await.insert(
            key.clone(),
            FrozenValue {
                key,
                address,
                value,
            },
        );
        self.start();
    }

    pub async fn unfreeze(&self, key: &str) -> bool {
        self.entries.lock().await.remove(key).is_some()
    }

    pub async fn frozen(&self) -> Vec<FrozenValue> {
        let mut values: Vec<_> = self.entries.lock().await.values().cloned().collect();
        values.sort_by(|a, b| a.key.cmp(&b.key));
        values
    }

    /// Arrête la boucle et oublie toutes les valeurs.
    pub async fn stop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.entries.lock().await.clear();
    }
}

impl Drop for Freezer {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_and_decodes_every_supported_type() {
        let cases = [
            (ValueType::Byte, Value::Byte(200)),
            (ValueType::TwoBytes, Value::TwoBytes(65000)),
            (ValueType::FourBytes, Value::FourBytes(-123456)),
            (ValueType::EightBytes, Value::EightBytes(-9876543210)),
            (ValueType::Float, Value::Float(13.37)),
            (ValueType::Double, Value::Double(-0.5)),
        ];
        for (value_type, value) in cases {
            let bytes = value.to_bytes();
            assert_eq!(bytes.len(), value_type.size().expect("taille connue"));
            assert_eq!(
                Value::from_bytes(&value_type, &bytes),
                Some(value),
                "aller-retour pour {value_type:?}"
            );
        }
    }

    #[test]
    fn refuses_truncated_buffers() {
        assert_eq!(Value::from_bytes(&ValueType::FourBytes, &[1, 2]), None);
        assert_eq!(Value::from_bytes(&ValueType::Text, &[1, 2, 3, 4]), None);
    }

    #[test]
    fn parses_user_input_according_to_type() {
        assert_eq!(
            Value::parse(&ValueType::Float, " 12.5 ").expect("float"),
            Value::Float(12.5)
        );
        assert_eq!(
            Value::parse(&ValueType::FourBytes, "-7").expect("entier"),
            Value::FourBytes(-7)
        );
        assert!(Value::parse(&ValueType::Byte, "300").is_err());
        assert!(Value::parse(&ValueType::Text, "abc").is_err());
    }

    #[tokio::test]
    async fn tracks_frozen_values_without_a_target() {
        // Le PID 0 n'existe pas : les écritures échouent en silence, ce qui
        // vérifie que la boucle survit à un processus absent.
        let mut freezer = Freezer::new(0);
        freezer.freeze("vie", 0x1000, Value::FourBytes(999)).await;
        freezer.freeze("faim", 0x2000, Value::Float(1.0)).await;

        let frozen = freezer.frozen().await;
        assert_eq!(frozen.len(), 2);
        assert_eq!(frozen[0].key, "faim");

        assert!(freezer.unfreeze("vie").await);
        assert!(!freezer.unfreeze("vie").await);
        assert_eq!(freezer.frozen().await.len(), 1);

        freezer.stop().await;
        assert!(freezer.frozen().await.is_empty());
    }
}
