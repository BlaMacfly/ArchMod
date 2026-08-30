//! Recherche de chemins de pointeurs.
//!
//! Une valeur de jeu vit presque toujours dans le tas, à une adresse qui change
//! à chaque lancement. Elle est donc inutilisable telle quelle dans un profil.
//! Mais le jeu, lui, la retrouve : il part d'une variable globale — dans un
//! module, donc à une adresse stable — et suit une suite de pointeurs.
//!
//! Ce module refait ce chemin à l'envers : partant de l'adresse trouvée, il
//! cherche qui pointe dessus, puis qui pointe sur ce qui pointe dessus, jusqu'à
//! retomber dans un module. Le résultat est directement convertible en recette
//! de profil.

use std::collections::HashMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::error::{Result, TuxError};
use crate::memory::{self, Module, Region};

/// Taille des tranches lues lors de la collecte.
const CHUNK: usize = 4 * 1024 * 1024;

/// Au-delà, la collecte s'arrête : un jeu très gros saturerait la mémoire.
const MAX_POINTERS: usize = 12_000_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOptions {
    /// Nombre maximal de déréférencements dans un chemin.
    pub max_depth: usize,
    /// Écart maximal entre l'adresse pointée et la cible.
    pub max_offset: u64,
    /// Nombre de chemins au-delà duquel on s'arrête.
    pub max_results: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            // Quatre niveaux couvrent l'écrasante majorité des cas ; au-delà,
            // le nombre de chemins explose sans gagner en fiabilité.
            max_depth: 4,
            max_offset: 0x800,
            max_results: 100,
        }
    }
}

/// Un chemin depuis une adresse statique jusqu'à la valeur.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerPath {
    pub module: String,
    /// Décalage de l'ancrage dans le module.
    pub base_offset: i64,
    /// Décalages successifs, dans l'ordre d'application.
    pub offsets: Vec<i64>,
    /// Adresse finale atteinte lors de la découverte, pour vérification.
    pub resolved: u64,
}

impl PointerPath {
    /// Écriture façon Cheat Engine, pour comparer avec une table publiée.
    pub fn display(&self) -> String {
        let mut text = format!("[{}+{:X}]", self.module, self.base_offset);
        for (index, offset) in self.offsets.iter().enumerate() {
            text = if index + 1 == self.offsets.len() {
                format!("{text}+{offset:X}")
            } else {
                format!("[{text}+{offset:X}]")
            };
        }
        text
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerScanReport {
    pub target: u64,
    pub paths: Vec<PointerPath>,
    /// Nombre de pointeurs examinés.
    pub pointers: usize,
    pub elapsed_ms: u64,
    /// La collecte ou la recherche a été écourtée.
    pub truncated: bool,
}

/// Table des pointeurs présents en mémoire, triée par valeur pointée.
struct PointerMap {
    /// (adresse pointée, emplacement du pointeur), trié par adresse pointée.
    entries: Vec<(u64, u64)>,
    truncated: bool,
}

impl PointerMap {
    /// Emplacements dont la valeur tombe dans `[low, high]`.
    fn pointing_into(&self, low: u64, high: u64) -> &[(u64, u64)] {
        let start = self.entries.partition_point(|(value, _)| *value < low);
        let end = self.entries.partition_point(|(value, _)| *value <= high);
        &self.entries[start..end]
    }
}

/// Vrai si l'adresse tombe dans une région lisible : un pointeur valide pointe
/// forcément vers de la mémoire réellement projetée.
fn is_mapped(sorted_regions: &[(u64, u64)], address: u64) -> bool {
    let index = sorted_regions.partition_point(|(start, _)| *start <= address);
    index > 0 && address < sorted_regions[index - 1].1
}

/// Parcourt la mémoire et relève tout ce qui ressemble à un pointeur.
fn collect_pointers(pid: u32, regions: &[Region]) -> Result<PointerMap> {
    let mut sorted: Vec<(u64, u64)> = regions
        .iter()
        .filter(|region| region.readable)
        .map(|region| (region.start, region.end))
        .collect();
    sorted.sort_unstable();

    let mut entries = Vec::new();
    let mut buffer = vec![0u8; CHUNK];
    let mut truncated = false;

    // Un pointeur est rangé dans une zone inscriptible — tas, pile, sections de
    // données — jamais dans du code en lecture seule.
    for region in regions.iter().filter(|r| r.readable && r.writable) {
        let mut cursor = region.start;
        while cursor < region.end {
            let length = ((region.end - cursor) as usize).min(CHUNK);
            let slice = &mut buffer[..length];
            if memory::read(pid, cursor, slice).is_err() {
                break;
            }

            let mut offset = 0;
            while offset + 8 <= length {
                let value =
                    u64::from_le_bytes(slice[offset..offset + 8].try_into().unwrap_or([0; 8]));
                // Filtre grossier avant la vérification coûteuse : une adresse
                // valide est alignée et n'est jamais minuscule.
                if value >= 0x1000 && value % 4 == 0 && is_mapped(&sorted, value) {
                    entries.push((value, cursor + offset as u64));
                    if entries.len() >= MAX_POINTERS {
                        truncated = true;
                        break;
                    }
                }
                offset += 8;
            }
            if truncated || length < CHUNK {
                break;
            }
            cursor += length as u64;
        }
        if truncated {
            break;
        }
    }

    entries.sort_unstable();
    Ok(PointerMap { entries, truncated })
}

/// Modules chargés, avec leur étendue, pour reconnaître une adresse statique.
fn modules(pid: u32, regions: &[Region]) -> Vec<Module> {
    // On retient les modules du programme lui-même. Les bibliothèques système
    // ne portent jamais les variables globales d'un jeu, et les écarter divise
    // le travail par dix.
    let mut names: Vec<String> = regions
        .iter()
        .filter(|region| {
            let Some(path) = region.path.as_ref().and_then(|p| p.to_str()) else {
                return false;
            };
            let lower = path.to_ascii_lowercase();
            lower.ends_with(".exe")
                || lower.ends_with(".dll")
                || !(path.starts_with("/usr/")
                    || path.starts_with("/lib")
                    || path.starts_with("/opt/")
                    || path.contains("/pressure-vessel/"))
        })
        .filter_map(|region| region.file_name().map(str::to_string))
        .collect();
    names.sort();
    names.dedup();

    names
        .into_iter()
        .filter_map(|name| memory::find_module(pid, &name).ok())
        .collect()
}

/// Cherche les chemins menant à `target`.
pub fn scan(pid: u32, target: u64, options: ScanOptions) -> Result<PointerScanReport> {
    let started = Instant::now();
    let regions = memory::regions(pid)?;
    let modules = modules(pid, &regions);
    if modules.is_empty() {
        return Err(TuxError::Internal(
            "aucun module identifiable dans ce processus".into(),
        ));
    }

    let map = collect_pointers(pid, &regions)?;
    let mut paths: Vec<PointerPath> = Vec::new();
    let mut truncated = map.truncated;

    // Parcours en largeur, de la valeur vers les adresses statiques. Chaque
    // niveau remonte d'un déréférencement.
    let mut level: Vec<(u64, Vec<i64>)> = vec![(target, Vec::new())];
    let mut seen: HashMap<u64, usize> = HashMap::new();

    for depth in 1..=options.max_depth {
        let mut next = Vec::new();

        for (address, suffix) in &level {
            let low = address.saturating_sub(options.max_offset);
            for (pointed, location) in map.pointing_into(low, *address) {
                let offset = (address - pointed) as i64;
                let mut offsets = vec![offset];
                offsets.extend(suffix.iter().copied());

                // Emplacement statique : le chemin est complet.
                if let Some(module) = modules.iter().find(|m| m.contains(*location)) {
                    paths.push(PointerPath {
                        module: module.name.clone(),
                        base_offset: (*location - module.base) as i64,
                        offsets,
                        resolved: target,
                    });
                    if paths.len() >= options.max_results {
                        truncated = true;
                        break;
                    }
                    continue;
                }

                if depth == options.max_depth {
                    continue;
                }
                // Un même emplacement atteint par un chemin plus court est
                // toujours préférable : on ne le réexplore pas.
                match seen.get(location) {
                    Some(known) if *known <= depth => continue,
                    _ => {
                        seen.insert(*location, depth);
                        next.push((*location, offsets));
                    }
                }
            }
            if paths.len() >= options.max_results {
                break;
            }
        }

        if paths.len() >= options.max_results || next.is_empty() {
            break;
        }
        level = next;
    }

    // Les chemins courts sont plus robustes : ils traversent moins de
    // structures susceptibles de changer d'une version à l'autre.
    paths.sort_by_key(|path| path.offsets.len());

    Ok(PointerScanReport {
        target,
        pointers: map.entries.len(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        truncated,
        paths,
    })
}

/// Rejoue un chemin pour vérifier qu'il mène toujours à une valeur lisible.
pub fn resolve(pid: u32, path: &PointerPath) -> Result<u64> {
    let module = memory::find_module(pid, &path.module)?;
    let mut address = memory::read_u64(pid, module.base.wrapping_add(path.base_offset as u64))?;

    let last = path.offsets.len().saturating_sub(1);
    for (index, offset) in path.offsets.iter().enumerate() {
        address = address.wrapping_add(*offset as u64);
        if index != last {
            address = memory::read_u64(pid, address)?;
        }
    }
    Ok(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    /// Variable globale de notre propre binaire : elle vit dans le module, donc
    /// à une adresse statique, et pointe vers le tas. C'est exactement la
    /// structure qu'on cherche dans un jeu.
    ///
    /// La double indirection est le sujet même du test : elle fabrique un
    /// chemin à deux déréférencements, comme celui qui mène à la vie du joueur.
    #[allow(clippy::redundant_allocation)]
    static ANCRAGE: OnceLock<Box<Box<u64>>> = OnceLock::new();

    #[test]
    fn describes_a_path_the_cheat_engine_way() {
        let path = PointerPath {
            module: "jeu.exe".into(),
            base_offset: 0x1234,
            offsets: vec![0x18, 0xC0],
            resolved: 0,
        };
        assert_eq!(path.display(), "[[jeu.exe+1234]+18]+C0");
    }

    #[test]
    fn locates_addresses_inside_mapped_regions() {
        let regions = [(0x1000u64, 0x2000u64), (0x5000, 0x6000)];
        assert!(is_mapped(&regions, 0x1500));
        assert!(!is_mapped(&regions, 0x2000), "la borne haute est exclue");
        assert!(!is_mapped(&regions, 0x3000));
        assert!(is_mapped(&regions, 0x5FFF));
        assert!(!is_mapped(&regions, 0x500));
    }

    #[test]
    fn finds_the_path_from_a_static_anchor_to_a_heap_value() {
        // On construit la chaîne : globale → tas → tas → valeur.
        ANCRAGE.get_or_init(|| Box::new(Box::new(0xFEED_BEEF)));
        let cible = ANCRAGE.get().expect("ancrage") as *const _ as u64;
        // Adresse réelle de la valeur, au bout des deux déréférencements.
        #[allow(clippy::borrowed_box)]
        let inner: &Box<u64> = ANCRAGE.get().expect("ancrage");
        let valeur = &**inner as *const u64 as u64;
        assert_ne!(valeur, cible);

        let rapport = scan(
            std::process::id(),
            valeur,
            ScanOptions {
                max_depth: 4,
                max_offset: 0x100,
                max_results: 20,
            },
        )
        .expect("recherche");

        assert!(rapport.pointers > 0, "des pointeurs doivent être relevés");
        assert!(
            !rapport.paths.is_empty(),
            "un chemin statique doit mener à la valeur"
        );

        // Le chemin trouvé doit effectivement ramener à la même adresse.
        let chemin = &rapport.paths[0];
        assert_eq!(
            resolve(std::process::id(), chemin).expect("résolution"),
            valeur,
            "le chemin {} doit se rejouer à l'identique",
            chemin.display()
        );
    }
}
