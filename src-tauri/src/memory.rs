//! Lecture et écriture de la mémoire d'un processus, et recherche de motifs.
//!
//! C'est la brique bas niveau du moteur natif : elle permet de retrouver dans
//! un jeu en cours d'exécution les adresses décrites par une table Cheat
//! Engine, sans passer par Wine.
//!
//! Un jeu lancé sous Proton reste un processus Linux ordinaire : ses modules PE
//! (`GameAssembly.dll`, `<jeu>.exe`) apparaissent dans `/proc/<pid>/maps` comme
//! des projections de fichier, et `process_vm_readv` donne accès à leur
//! contenu. Aucun `ptrace` n'est nécessaire, donc le jeu n'est jamais suspendu.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::Serialize;

use crate::error::{Result, TuxError};

/// Une plage de mémoire telle que décrite par `/proc/<pid>/maps`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub start: u64,
    pub end: u64,
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    /// Fichier projeté, s'il y en a un.
    pub path: Option<PathBuf>,
}

impl Region {
    pub fn size(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// Le nom de fichier projeté, sans son dossier.
    pub fn file_name(&self) -> Option<&str> {
        self.path.as_ref()?.file_name()?.to_str()
    }
}

/// Un module PE chargé par Wine dans l'espace d'adressage du jeu.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub name: String,
    pub base: u64,
    /// Taille de l'image, lue dans l'en-tête PE en mémoire.
    pub size: u64,
    pub path: PathBuf,
}

impl Module {
    pub fn contains(&self, address: u64) -> bool {
        address >= self.base && address < self.base.saturating_add(self.size)
    }
}

/// Analyse `/proc/<pid>/maps`.
pub fn regions(pid: u32) -> Result<Vec<Region>> {
    let path = format!("/proc/{pid}/maps");
    let content = fs::read_to_string(&path).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            TuxError::ProcessGone { pid }
        } else {
            TuxError::io(&path, source)
        }
    })?;

    let mut regions = Vec::new();
    for line in content.lines() {
        // Format : 7f3c-7f3d rw-p 00000000 08:32 22020205  /chemin/fichier
        let mut fields = line
            .splitn(6, char::is_whitespace)
            .filter(|f| !f.is_empty());
        let Some(range) = fields.next() else { continue };
        let Some(perms) = fields.next() else { continue };
        let Some((start, end)) = range.split_once('-') else {
            continue;
        };
        let (Ok(start), Ok(end)) = (u64::from_str_radix(start, 16), u64::from_str_radix(end, 16))
        else {
            continue;
        };

        let path = fields
            .nth(3)
            .map(str::trim)
            .filter(|p| p.starts_with('/'))
            .map(PathBuf::from);

        let perms = perms.as_bytes();
        regions.push(Region {
            start,
            end,
            readable: perms.first() == Some(&b'r'),
            writable: perms.get(1) == Some(&b'w'),
            executable: perms.get(2) == Some(&b'x'),
            path,
        });
    }
    Ok(regions)
}

/// Lit `buffer.len()` octets à `address` dans le processus `pid`.
///
/// Une lecture partielle est une erreur : elle signifie qu'on a franchi la fin
/// d'une région projetée, et poursuivre donnerait des données tronquées.
pub fn read(pid: u32, address: u64, buffer: &mut [u8]) -> Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }
    let local = libc::iovec {
        iov_base: buffer.as_mut_ptr().cast(),
        iov_len: buffer.len(),
    };
    let remote = libc::iovec {
        iov_base: address as *mut libc::c_void,
        iov_len: buffer.len(),
    };

    // SAFETY: les deux iovec décrivent des tampons valides ; l'appel ne fait que
    // copier des octets et signale toute erreur par un retour négatif.
    let read = unsafe { libc::process_vm_readv(pid as libc::pid_t, &local, 1, &remote, 1, 0) };

    if read < 0 {
        return Err(TuxError::MemoryAccess {
            pid,
            address,
            operation: "lecture",
            source: io::Error::last_os_error(),
        });
    }
    if read as usize != buffer.len() {
        return Err(TuxError::MemoryAccess {
            pid,
            address,
            operation: "lecture",
            source: io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("{read} octets lus sur {} demandés", buffer.len()),
            ),
        });
    }
    Ok(())
}

/// Écrit des octets dans le processus cible.
pub fn write(pid: u32, address: u64, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    let local = libc::iovec {
        iov_base: bytes.as_ptr() as *mut libc::c_void,
        iov_len: bytes.len(),
    };
    let remote = libc::iovec {
        iov_base: address as *mut libc::c_void,
        iov_len: bytes.len(),
    };

    // SAFETY: mêmes garanties que pour la lecture. L'écriture reste soumise aux
    // permissions du noyau : un refus remonte en erreur, pas en corruption.
    let written = unsafe { libc::process_vm_writev(pid as libc::pid_t, &local, 1, &remote, 1, 0) };

    if written < 0 || written as usize != bytes.len() {
        return Err(TuxError::MemoryAccess {
            pid,
            address,
            operation: "écriture",
            source: io::Error::last_os_error(),
        });
    }
    Ok(())
}

pub fn read_u32(pid: u32, address: u64) -> Result<u32> {
    let mut buffer = [0u8; 4];
    read(pid, address, &mut buffer)?;
    Ok(u32::from_le_bytes(buffer))
}

pub fn read_u64(pid: u32, address: u64) -> Result<u64> {
    let mut buffer = [0u8; 8];
    read(pid, address, &mut buffer)?;
    Ok(u64::from_le_bytes(buffer))
}

/// Suit une chaîne de pointeurs façon Cheat Engine : `[[base]+18]+C0`.
pub fn resolve_chain(pid: u32, base: u64, offsets: &[u64]) -> Result<u64> {
    let mut address = base;
    for (index, offset) in offsets.iter().enumerate() {
        // Le dernier décalage désigne la valeur elle-même : on ne déréférence pas.
        if index + 1 == offsets.len() {
            return Ok(address.wrapping_add(*offset));
        }
        address = read_u64(pid, address)?.wrapping_add(*offset);
    }
    Ok(address)
}

/// Localise un module PE et déduit sa taille de l'en-tête chargé en mémoire.
pub fn find_module(pid: u32, name: &str) -> Result<Module> {
    let regions = regions(pid)?;
    let matching: Vec<&Region> = regions
        .iter()
        .filter(|region| {
            region
                .file_name()
                .is_some_and(|file| file.eq_ignore_ascii_case(name))
        })
        .collect();

    let first = matching.first().ok_or_else(|| TuxError::ModuleNotFound {
        pid,
        name: name.to_string(),
    })?;
    let base = first.start;

    // Taille officielle : OptionalHeader.SizeOfImage. On retombe sur l'étendue
    // des projections si l'en-tête n'est pas lisible.
    let size = pe_image_size(pid, base).unwrap_or_else(|| {
        matching
            .iter()
            .map(|region| region.end)
            .max()
            .unwrap_or(base)
            .saturating_sub(base)
    });

    Ok(Module {
        name: name.to_string(),
        base,
        size,
        path: first.path.clone().unwrap_or_default(),
    })
}

/// Lit `SizeOfImage` dans l'en-tête PE projeté à `base`.
fn pe_image_size(pid: u32, base: u64) -> Option<u64> {
    let mut dos = [0u8; 2];
    read(pid, base, &mut dos).ok()?;
    if &dos != b"MZ" {
        return None;
    }
    // e_lfanew : décalage de la signature PE, à 0x3C de l'en-tête DOS.
    let pe_offset = read_u32(pid, base + 0x3C).ok()? as u64;
    let mut signature = [0u8; 4];
    read(pid, base + pe_offset, &mut signature).ok()?;
    if &signature != b"PE\0\0" {
        return None;
    }
    // SizeOfImage : 0x50 après la signature, pour PE32 comme PE32+.
    let size = read_u32(pid, base + pe_offset + 0x50).ok()?;
    (size > 0).then_some(size as u64)
}

// ---------------------------------------------------------------------------
// Recherche de motifs (AOB)
// ---------------------------------------------------------------------------

/// Motif d'octets avec jokers, au format Cheat Engine : `48 8B ?? 40 01`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    bytes: Vec<u8>,
    /// `false` pour un joker.
    mask: Vec<bool>,
    /// Deux positions connues, testées avant la comparaison complète.
    anchors: (usize, usize),
}

impl Pattern {
    pub fn parse(input: &str) -> Result<Self> {
        let mut bytes = Vec::new();
        let mut mask = Vec::new();

        for token in input.split_whitespace() {
            // CE accepte « ?? », « ? » et les demi-jokers façon « 4? ».
            if token.chars().all(|c| c == '?') {
                bytes.push(0);
                mask.push(false);
                continue;
            }
            let value = u8::from_str_radix(token, 16).map_err(|_| TuxError::PatternInvalid {
                pattern: input.to_string(),
                detail: format!("« {token} » n'est pas un octet hexadécimal"),
            })?;
            bytes.push(value);
            mask.push(true);
        }

        if bytes.is_empty() {
            return Err(TuxError::PatternInvalid {
                pattern: input.to_string(),
                detail: "motif vide".into(),
            });
        }
        if !mask.iter().any(|known| *known) {
            return Err(TuxError::PatternInvalid {
                pattern: input.to_string(),
                detail: "un motif entièrement composé de jokers correspondrait partout".into(),
            });
        }
        let anchors = choose_anchors(&bytes, &mask);
        Ok(Self {
            bytes,
            mask,
            anchors,
        })
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    fn matches_at(&self, haystack: &[u8], index: usize) -> bool {
        self.bytes
            .iter()
            .zip(&self.mask)
            .enumerate()
            .all(|(offset, (byte, known))| !known || haystack.get(index + offset) == Some(byte))
    }

    /// Tous les décalages où le motif apparaît dans `haystack`.
    pub fn find_all(&self, haystack: &[u8]) -> Vec<usize> {
        if haystack.len() < self.bytes.len() {
            return Vec::new();
        }
        let mut hits = Vec::new();
        let limit = haystack.len() - self.bytes.len();

        // Deux octets rares filtrent les positions avant la comparaison
        // complète : s'ancrer sur le seul premier octet est ruineux quand il
        // vaut 0x48, le préfixe REX omniprésent en x86-64.
        let (first, second) = self.anchors;
        let (first_byte, second_byte) = (self.bytes[first], self.bytes[second]);

        for index in 0..=limit {
            if haystack[index + first] == first_byte
                && haystack[index + second] == second_byte
                && self.matches_at(haystack, index)
            {
                hits.push(index);
            }
        }
        hits
    }
}

/// Fréquence approximative des octets dans du code x86-64 : plus la valeur est
/// élevée, plus l'octet est mauvais comme filtre. Les préfixes REX (0x40-0x4F),
/// les octets nuls et le remplissage 0xCC saturent n'importe quel binaire.
fn byte_cost(byte: u8) -> u32 {
    match byte {
        0x00 => 100,
        0x48 => 90,
        0xCC | 0xFF => 60,
        0x40..=0x4F => 50,
        0x8B | 0x89 => 40,
        _ => 1,
    }
}

/// Choisit les deux positions connues les plus discriminantes.
fn choose_anchors(bytes: &[u8], mask: &[bool]) -> (usize, usize) {
    let mut known: Vec<usize> = mask
        .iter()
        .enumerate()
        .filter(|(_, is_known)| **is_known)
        .map(|(index, _)| index)
        .collect();
    known.sort_by_key(|index| byte_cost(bytes[*index]));

    let first = known.first().copied().unwrap_or(0);
    let second = known.get(1).copied().unwrap_or(first);
    (first, second)
}

/// Taille des tranches lues lors d'un balayage.
const CHUNK: usize = 4 * 1024 * 1024;

/// Cherche un motif dans l'étendue d'un module, comme `aobscanmodule`.
pub fn scan_module(pid: u32, module: &Module, pattern: &Pattern) -> Result<Vec<u64>> {
    let regions = regions(pid)?;
    let mut hits = Vec::new();

    for region in regions.iter().filter(|region| {
        region.readable && region.start >= module.base && region.end <= module.base + module.size
    }) {
        hits.extend(scan_region(pid, region, pattern));
    }
    hits.sort_unstable();
    hits.dedup();
    Ok(hits)
}

/// Balaye toutes les régions lisibles du processus (scan « mémoire entière »).
pub fn scan_all(pid: u32, pattern: &Pattern) -> Result<Vec<u64>> {
    let regions = regions(pid)?;
    let mut hits = Vec::new();
    for region in regions.iter().filter(|region| region.readable) {
        hits.extend(scan_region(pid, region, pattern));
    }
    hits.sort_unstable();
    hits.dedup();
    Ok(hits)
}

/// Une région peut disparaître pendant le balayage : une lecture qui échoue
/// interrompt cette région seulement, jamais le scan complet.
fn scan_region(pid: u32, region: &Region, pattern: &Pattern) -> Vec<u64> {
    let overlap = pattern.len().saturating_sub(1);
    let mut hits = Vec::new();
    let mut cursor = region.start;
    let mut buffer = vec![0u8; CHUNK];

    while cursor < region.end {
        let length = ((region.end - cursor) as usize).min(CHUNK);
        let slice = &mut buffer[..length];
        if read(pid, cursor, slice).is_err() {
            break;
        }
        hits.extend(
            pattern
                .find_all(slice)
                .into_iter()
                .map(|offset| cursor + offset as u64),
        );

        if length < CHUNK {
            break;
        }
        // Chevauchement : un motif à cheval sur deux tranches doit être vu.
        cursor += (CHUNK - overlap) as u64;
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cheat_engine_syntax() {
        let pattern = Pattern::parse("48 8B ?? 40 01 00 00").expect("motif valide");
        assert_eq!(pattern.len(), 7);
        assert!(!pattern.mask[2], "le troisième octet est un joker");
    }

    #[test]
    fn rejects_degenerate_patterns() {
        assert!(Pattern::parse("").is_err());
        assert!(Pattern::parse("?? ?? ??").is_err());
        assert!(Pattern::parse("48 ZZ").is_err());
    }

    #[test]
    fn anchors_on_the_rarest_bytes() {
        // « 48 » et « 00 » sont omniprésents : l'ancre doit les éviter.
        let pattern = Pattern::parse("48 8B 89 40 01 00 00 74 18").expect("motif valide");
        let (first, second) = pattern.anchors;
        assert_ne!(pattern.bytes[first], 0x48);
        assert_ne!(pattern.bytes[first], 0x00);
        assert_ne!(first, second, "deux ancres distinctes");
    }

    #[test]
    fn finds_every_occurrence_with_wildcards() {
        let haystack = [
            0x00, 0x48, 0x8B, 0x89, 0x40, 0x01, 0xFF, 0x48, 0x8B, 0x7A, 0x40, 0x01,
        ];
        let pattern = Pattern::parse("48 8B ?? 40 01").expect("motif valide");
        assert_eq!(pattern.find_all(&haystack), vec![1, 7]);
    }

    #[test]
    fn ignores_patterns_longer_than_the_haystack() {
        let pattern = Pattern::parse("48 8B 89 40 01 00 00").expect("motif valide");
        assert!(pattern.find_all(&[0x48, 0x8B]).is_empty());
    }

    #[test]
    fn resolves_a_pointer_chain_on_our_own_process() {
        // Une chaîne d'un seul décalage ne déréférence rien : elle doit rendre
        // base + offset, ce qui se vérifie sans processus tiers.
        let pid = std::process::id();
        assert_eq!(resolve_chain(pid, 0x1000, &[0x18]).expect("chaîne"), 0x1018);
    }

    #[test]
    fn reads_our_own_memory() {
        let pid = std::process::id();
        let source = [0xDEu8, 0xAD, 0xBE, 0xEF];
        let mut destination = [0u8; 4];
        read(pid, source.as_ptr() as u64, &mut destination).expect("lecture de soi-même");
        assert_eq!(destination, source);
    }

    #[test]
    fn lists_our_own_regions() {
        let regions = regions(std::process::id()).expect("maps lisible");
        assert!(regions.iter().any(|region| region.readable));
        assert!(regions.iter().any(|region| region.path.is_some()));
    }
}
