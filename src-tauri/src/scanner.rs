//! Recherche de valeurs en mémoire, à la manière de Cheat Engine.
//!
//! Le principe est celui du jeu du plus chaud : on cherche une valeur connue
//! — 247 unités de bois —, on en ramasse, on cherche 312, et l'intersection des
//! deux résultats isole l'adresse. C'est ce qui permet de créer une option sans
//! connaître le jeu ni disposer d'une table publiée.
//!
//! Une adresse ainsi trouvée vaut **pour la session en cours**. Si elle tombe
//! dans un module, on peut en faire une recette durable ; si elle vit dans le
//! tas, il faudrait une recherche de pointeurs, que ce module ne fait pas
//! encore — et il vaut mieux le dire que de laisser croire l'inverse.

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::cheat_table::ValueType;
use crate::engine::Value;
use crate::error::{Result, TuxError};
use crate::memory;

/// Au-delà, on cesse d'accumuler : une première recherche trop large ne sert à
/// rien et remplirait la mémoire pour rien.
const MAX_CANDIDATES: usize = 2_000_000;

/// Taille des tranches lues dans le processus cible.
const CHUNK: usize = 4 * 1024 * 1024;

/// Critère appliqué à une recherche.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum Filter {
    /// Égale à la valeur donnée.
    Exact(Value),
    /// Strictement supérieure à la valeur donnée.
    Greater(Value),
    /// Strictement inférieure à la valeur donnée.
    Less(Value),
    /// A augmenté depuis la recherche précédente.
    Increased,
    /// A diminué depuis la recherche précédente.
    Decreased,
    /// A changé, dans un sens ou dans l'autre.
    Changed,
    /// N'a pas bougé.
    Unchanged,
}

impl Filter {
    /// Une première recherche n'a rien à quoi se comparer.
    fn needs_previous(self) -> bool {
        matches!(
            self,
            Filter::Increased | Filter::Decreased | Filter::Changed | Filter::Unchanged
        )
    }
}

/// Comparaison numérique de deux valeurs de même type.
fn as_f64(value: Value) -> f64 {
    match value {
        Value::Byte(v) => v as f64,
        Value::TwoBytes(v) => v as f64,
        Value::FourBytes(v) => v as f64,
        Value::EightBytes(v) => v as f64,
        Value::Float(v) => v as f64,
        Value::Double(v) => v,
    }
}

/// Les flottants ne se comparent pas au bit près : un `247.0` affiché par le
/// jeu peut valoir `246.99998`. On tolère un écart relatif minime.
fn nearly_equal(a: Value, b: Value) -> bool {
    match (a, b) {
        (Value::Float(_), _) | (Value::Double(_), _) => {
            let (x, y) = (as_f64(a), as_f64(b));
            let tolerance = x.abs().max(y.abs()).max(1.0) * 1e-6;
            (x - y).abs() <= tolerance
        }
        _ => as_f64(a) == as_f64(b),
    }
}

fn matches(filter: Filter, current: Value, previous: Option<Value>) -> bool {
    match filter {
        Filter::Exact(wanted) => nearly_equal(current, wanted),
        Filter::Greater(bound) => as_f64(current) > as_f64(bound),
        Filter::Less(bound) => as_f64(current) < as_f64(bound),
        Filter::Increased => previous.is_some_and(|p| as_f64(current) > as_f64(p)),
        Filter::Decreased => previous.is_some_and(|p| as_f64(current) < as_f64(p)),
        Filter::Changed => previous.is_some_and(|p| !nearly_equal(current, p)),
        Filter::Unchanged => previous.is_some_and(|p| nearly_equal(current, p)),
    }
}

/// Une adresse retenue, avec sa valeur au moment de la dernière lecture.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub address: u64,
    pub value: Value,
    /// Valeur lors de la recherche précédente, pour afficher l'évolution.
    pub previous: Option<Value>,
}

/// Ce qu'une adresse permet d'espérer en termes de profil durable.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Anchorage {
    /// L'adresse tombe dans un module : elle se convertit en recette stable.
    Module { module: String, offset: i64 },
    /// Mémoire anonyme : l'adresse changera au prochain lancement. Il faudrait
    /// une recherche de pointeurs pour en tirer un profil.
    Volatile,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateView {
    pub address: u64,
    pub value: Value,
    pub previous: Option<Value>,
    pub anchorage: Anchorage,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub matches: usize,
    pub elapsed_ms: u64,
    /// La limite d'accumulation a été atteinte : affine avant de conclure.
    pub truncated: bool,
    /// Extrait des résultats, borné pour ne pas saturer l'interface.
    pub sample: Vec<CandidateView>,
}

/// Une session de recherche attachée à un processus.
pub struct Scanner {
    pid: u32,
    value_type: ValueType,
    candidates: Vec<Candidate>,
    started: bool,
}

impl Scanner {
    pub fn new(pid: u32, value_type: ValueType) -> Self {
        Self {
            pid,
            value_type,
            candidates: Vec::new(),
            started: false,
        }
    }

    pub fn value_type(&self) -> &ValueType {
        &self.value_type
    }

    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    pub fn started(&self) -> bool {
        self.started
    }

    /// Lance une recherche : la première balaye la mémoire, les suivantes
    /// n'examinent que les adresses déjà retenues.
    pub fn scan(&mut self, filter: Filter) -> Result<ScanReport> {
        let started_at = Instant::now();

        if !self.started {
            if filter.needs_previous() {
                return Err(TuxError::Internal(
                    "une première recherche ne peut pas se comparer à la précédente : \
                     indique une valeur"
                        .into(),
                ));
            }
            self.sweep(filter)?;
            self.started = true;
        } else {
            self.refine(filter)?;
        }

        Ok(ScanReport {
            matches: self.candidates.len(),
            elapsed_ms: started_at.elapsed().as_millis() as u64,
            truncated: self.candidates.len() >= MAX_CANDIDATES,
            sample: self.view(200)?,
        })
    }

    /// Premier balayage : toute la mémoire inscriptible du processus.
    ///
    /// On se limite aux régions inscriptibles : une valeur de jeu que l'on veut
    /// modifier y vit forcément, et cela écarte d'emblée le code et les
    /// ressources en lecture seule.
    fn sweep(&mut self, filter: Filter) -> Result<()> {
        let size = self.value_type.size().ok_or_else(|| {
            TuxError::Internal(format!("type {:?} non recherchable", self.value_type))
        })?;
        let regions = memory::regions(self.pid)?;
        let mut buffer = vec![0u8; CHUNK];
        self.candidates.clear();

        for region in regions.iter().filter(|r| r.readable && r.writable) {
            let mut cursor = region.start;
            while cursor < region.end {
                let length = ((region.end - cursor) as usize).min(CHUNK);
                let slice = &mut buffer[..length];
                // Une région peut disparaître en cours de balayage.
                if memory::read(self.pid, cursor, slice).is_err() {
                    break;
                }

                // Les valeurs d'un jeu sont alignées sur leur taille : chercher
                // sans alignement multiplierait le travail par quatre pour un
                // gain négligeable.
                let mut offset = 0;
                while offset + size <= length {
                    if let Some(value) = Value::from_bytes(&self.value_type, &slice[offset..]) {
                        if matches(filter, value, None) {
                            self.candidates.push(Candidate {
                                address: cursor + offset as u64,
                                value,
                                previous: None,
                            });
                            if self.candidates.len() >= MAX_CANDIDATES {
                                return Ok(());
                            }
                        }
                    }
                    offset += size;
                }

                if length < CHUNK {
                    break;
                }
                cursor += length as u64;
            }
        }
        Ok(())
    }

    /// Recherches suivantes : on relit les adresses retenues et on filtre.
    fn refine(&mut self, filter: Filter) -> Result<()> {
        let size = self.value_type.size().unwrap_or(4);
        let mut buffer = vec![0u8; size];
        let value_type = self.value_type.clone();
        let pid = self.pid;

        self.candidates.retain_mut(|candidate| {
            if memory::read(pid, candidate.address, &mut buffer).is_err() {
                // L'adresse n'est plus lisible : la région a été libérée.
                return false;
            }
            let Some(current) = Value::from_bytes(&value_type, &buffer) else {
                return false;
            };
            if matches(filter, current, Some(candidate.value)) {
                candidate.previous = Some(candidate.value);
                candidate.value = current;
                true
            } else {
                false
            }
        });
        Ok(())
    }

    /// Extrait des résultats, enrichi de leur ancrage.
    pub fn view(&self, limit: usize) -> Result<Vec<CandidateView>> {
        let regions = memory::regions(self.pid).unwrap_or_default();

        Ok(self
            .candidates
            .iter()
            .take(limit)
            .map(|candidate| {
                let anchorage = regions
                    .iter()
                    .find(|region| {
                        candidate.address >= region.start && candidate.address < region.end
                    })
                    .and_then(|region| region.file_name().map(|name| (region, name)))
                    .map(|(region, name)| Anchorage::Module {
                        module: name.to_string(),
                        offset: (candidate.address - region.start) as i64,
                    })
                    .unwrap_or(Anchorage::Volatile);

                CandidateView {
                    address: candidate.address,
                    value: candidate.value,
                    previous: candidate.previous,
                    anchorage,
                }
            })
            .collect())
    }

    /// Relit les valeurs affichées sans filtrer : anime la liste de résultats.
    pub fn refresh(&mut self) -> Result<Vec<CandidateView>> {
        let size = self.value_type.size().unwrap_or(4);
        let mut buffer = vec![0u8; size];
        for candidate in self.candidates.iter_mut().take(200) {
            if memory::read(self.pid, candidate.address, &mut buffer).is_ok() {
                if let Some(current) = Value::from_bytes(&self.value_type, &buffer) {
                    candidate.value = current;
                }
            }
        }
        self.view(200)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_integers_exactly() {
        assert!(matches(
            Filter::Exact(Value::FourBytes(247)),
            Value::FourBytes(247),
            None
        ));
        assert!(!matches(
            Filter::Exact(Value::FourBytes(247)),
            Value::FourBytes(248),
            None
        ));
    }

    #[test]
    fn tolerates_float_rounding() {
        // Un jeu affichant « 247 » peut stocker 246.99998.
        assert!(matches(
            Filter::Exact(Value::Float(247.0)),
            Value::Float(246.99998),
            None
        ));
        assert!(!matches(
            Filter::Exact(Value::Float(247.0)),
            Value::Float(248.0),
            None
        ));
    }

    #[test]
    fn evaluates_evolution_against_the_previous_pass() {
        let previous = Some(Value::FourBytes(100));
        assert!(matches(Filter::Increased, Value::FourBytes(120), previous));
        assert!(!matches(Filter::Increased, Value::FourBytes(80), previous));
        assert!(matches(Filter::Decreased, Value::FourBytes(80), previous));
        assert!(matches(Filter::Changed, Value::FourBytes(80), previous));
        assert!(matches(Filter::Unchanged, Value::FourBytes(100), previous));
        assert!(!matches(Filter::Changed, Value::FourBytes(100), previous));
    }

    #[test]
    fn evolution_filters_need_a_previous_pass() {
        assert!(Filter::Increased.needs_previous());
        assert!(Filter::Unchanged.needs_previous());
        assert!(!Filter::Exact(Value::Byte(1)).needs_previous());

        let mut scanner = Scanner::new(0, ValueType::FourBytes);
        assert!(
            scanner.scan(Filter::Increased).is_err(),
            "une première recherche n'a rien à comparer"
        );
    }

    #[test]
    fn bounds_apply_to_any_numeric_type() {
        assert!(matches(
            Filter::Greater(Value::Float(10.0)),
            Value::Float(10.5),
            None
        ));
        assert!(matches(
            Filter::Less(Value::EightBytes(1000)),
            Value::EightBytes(999),
            None
        ));
    }

    #[test]
    fn finds_a_value_in_our_own_memory() {
        // On cherche dans notre propre processus : la valeur est forcément là.
        let cible: Vec<i32> = vec![0x5EED_1234; 64];
        let mut scanner = Scanner::new(std::process::id(), ValueType::FourBytes);
        let report = scanner
            .scan(Filter::Exact(Value::FourBytes(0x5EED_1234)))
            .expect("recherche");

        assert!(report.matches >= 64, "les 64 copies doivent être trouvées");
        assert!(!report.sample.is_empty());
        // On garde la cible vivante jusqu'ici, sinon le compilateur pourrait
        // la libérer avant le balayage.
        assert_eq!(cible[0], 0x5EED_1234);
    }
}
