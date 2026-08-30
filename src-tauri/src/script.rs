//! Exécution des scripts d'auto-assembleur d'une table Cheat Engine.
//!
//! Un script `[ENABLE]` type déclare un motif à retrouver, réserve un bloc de
//! mémoire, y écrit du code qui capture un registre, puis détourne
//! l'instruction visée vers ce code. C'est ce qui donne vie aux symboles dont
//! dépendent toutes les entrées d'une table moderne.
//!
//! ArchMod ne réserve pas de mémoire au sens de Cheat Engine : il ne peut pas
//! appeler `mmap` dans le jeu sans le suspendre. Il cherche à la place une zone
//! de remplissage inutilisée, de préférence inscriptible **et** exécutable,
//! puisque le code injecté y écrira lui-même.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

use serde::Serialize;

use crate::assembler;
use crate::error::{Result, TuxError};
use crate::hook;
use crate::memory::{self, Pattern};

/// Une directive du script, avant exécution.
#[derive(Debug, Clone, PartialEq)]
enum Directive {
    Scan {
        symbol: String,
        module: String,
        pattern: String,
    },
    Alloc {
        name: String,
        size: usize,
        near: Option<String>,
    },
    Register(String),
    /// `label(x)` : purement déclaratif, l'assembleur les déduit du bloc.
    Label(String),
}

/// Un bloc de code introduit par `<étiquette>:` au premier niveau.
#[derive(Debug, Clone, PartialEq)]
struct Section {
    anchor: String,
    body: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Script {
    directives: Vec<Directive>,
    sections: Vec<Section>,
}

/// Découpe `nom(a,b,c)` en ses arguments.
fn call<'a>(line: &'a str, function: &str) -> Option<Vec<&'a str>> {
    let rest = line.trim().strip_prefix(function)?;
    let inner = rest.trim().strip_prefix('(')?.rsplit_once(')')?.0;
    Some(inner.split(',').map(str::trim).collect())
}

/// Sépare les directives des blocs de code.
fn parse(source: &str) -> Script {
    let mut directives = Vec::new();
    let mut sections: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;

    for raw in source.lines() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("[enable]") || lower.starts_with("[disable]") {
            continue;
        }

        if let Some(arguments) = call(line, "aobscanmodule") {
            if let [symbol, module, pattern @ ..] = arguments.as_slice() {
                let pattern = pattern.join(" ");
                if !pattern.is_empty() {
                    directives.push(Directive::Scan {
                        symbol: symbol.to_ascii_lowercase(),
                        module: (*module).to_string(),
                        pattern,
                    });
                }
            }
            continue;
        }
        if let Some(arguments) = call(line, "alloc") {
            if let [name, size, near @ ..] = arguments.as_slice() {
                directives.push(Directive::Alloc {
                    name: name.to_ascii_lowercase(),
                    // `$1000` est hexadécimal chez Cheat Engine.
                    // Chez Cheat Engine, « $ » introduit un hexadécimal :
                    // $1000 vaut 4096, pas mille.
                    size: match size.strip_prefix('$') {
                        Some(hex) => usize::from_str_radix(hex, 16).unwrap_or(0x1000),
                        None => size.parse().unwrap_or(0x1000),
                    },
                    near: near.first().map(|n| n.to_ascii_lowercase()),
                });
            }
            continue;
        }
        if let Some(arguments) = call(line, "registersymbol") {
            if let Some(name) = arguments.first() {
                directives.push(Directive::Register(name.to_ascii_lowercase()));
            }
            continue;
        }
        if let Some(arguments) = call(line, "label") {
            if let Some(name) = arguments.first() {
                directives.push(Directive::Label(name.to_ascii_lowercase()));
            }
            continue;
        }

        // Une étiquette en début de ligne, non indentée, ouvre un bloc.
        if let Some(name) = line.strip_suffix(':') {
            if !raw.starts_with([' ', '\t']) {
                if let Some(section) = current.take() {
                    sections.push(section);
                }
                current = Some(Section {
                    anchor: name.trim().to_ascii_lowercase(),
                    body: String::new(),
                });
                continue;
            }
        }

        if let Some(section) = current.as_mut() {
            section.body.push_str(raw);
            section.body.push('\n');
        }
    }

    if let Some(section) = current {
        sections.push(section);
    }
    Script {
        directives,
        sections,
    }
}

/// Ce qu'une activation a modifié, pour pouvoir revenir en arrière.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub address: u64,
    /// Octets d'origine, à restaurer lors de la désactivation.
    pub original: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnableReport {
    /// Symboles utilisables ensuite par les entrées de la table.
    pub symbols: HashMap<String, u64>,
    pub patches: Vec<Patch>,
    /// Zone retenue pour le code injecté.
    pub allocation: u64,
}

/// Écrit en ignorant les protections de page.
fn write_force(pid: u32, address: u64, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .open(format!("/proc/{pid}/mem"))
        .map_err(|source| TuxError::MemoryAccess {
            pid,
            address,
            operation: "écriture",
            source,
        })?;
    file.seek(SeekFrom::Start(address))
        .and_then(|_| file.write_all(bytes))
        .map_err(|source| TuxError::MemoryAccess {
            pid,
            address,
            operation: "écriture",
            source,
        })
}

/// Exécute la partie `[ENABLE]` d'un script.
pub fn enable(pid: u32, source: &str) -> Result<EnableReport> {
    let script = parse(source);
    let mut symbols: HashMap<String, u64> = HashMap::new();

    // 1. Retrouver les motifs : ils fournissent les adresses d'ancrage.
    for directive in &script.directives {
        if let Directive::Scan {
            symbol,
            module,
            pattern,
        } = directive
        {
            let parsed = Pattern::parse(pattern)?;
            let module = memory::find_module(pid, module)?;
            let hits = memory::scan_module(pid, &module, &parsed)?;
            let address = hits
                .first()
                .copied()
                .ok_or_else(|| TuxError::PatternNotFound {
                    pattern: pattern.clone(),
                    module: module.name.clone(),
                    found: 0,
                    wanted: 0,
                })?;
            symbols.insert(symbol.clone(), address);
        }
    }

    // 2. Trouver une zone pour le code injecté, à portée de saut de l'ancrage.
    let anchor = script
        .directives
        .iter()
        .find_map(|directive| match directive {
            Directive::Alloc { near, .. } => near.as_ref().and_then(|n| symbols.get(n)).copied(),
            _ => None,
        })
        .or_else(|| symbols.values().copied().next())
        .ok_or_else(|| TuxError::Internal("le script ne vise aucune adresse".into()))?;

    let module = memory::regions(pid)?
        .into_iter()
        .find(|region| anchor >= region.start && anchor < region.end)
        .and_then(|region| region.file_name().map(str::to_string))
        .and_then(|name| memory::find_module(pid, &name).ok())
        .ok_or_else(|| TuxError::Internal("ancrage hors de tout module".into()))?;

    // Le code injecté écrit lui-même dans cette zone : elle doit être
    // exécutable *et* inscriptible, faute de quoi le jeu planterait en y
    // rangeant sa capture.
    let cave = hook::find_cave(pid, &module, 256, true, true)?.ok_or_else(|| {
        TuxError::Internal(
            "aucune zone libre à la fois exécutable et inscriptible : ce script demande \
             une allocation, qu'ArchMod ne sait pas encore faire"
                .into(),
        )
    })?;

    for directive in &script.directives {
        if let Directive::Alloc { name, .. } = directive {
            symbols.insert(name.clone(), cave.address);
        }
    }

    // 3. Le point de reprise se situe après les instructions volées.
    let plan = hook::plan(pid, &module, anchor)?;
    symbols.insert("return".into(), plan.resume);

    // 4. Assembler le bloc injecté, puis le détour lui-même.
    let injected = script
        .sections
        .iter()
        .find(|section| !symbols.contains_key(&section.anchor) || section.anchor == "newmem")
        .or_else(|| script.sections.first())
        .ok_or_else(|| TuxError::Internal("le script ne contient aucun code".into()))?;

    let assembly = assembler::assemble(&injected.body, cave.address, &symbols)?;
    if assembly.bytes.len() > cave.size {
        return Err(TuxError::Internal(format!(
            "le code injecté fait {} octets, la zone libre {}",
            assembly.bytes.len(),
            cave.size
        )));
    }

    let mut patches = Vec::new();

    // Le bloc injecté d'abord : le détour ne doit jamais mener à du vide.
    let mut original = vec![0u8; assembly.bytes.len()];
    memory::read(pid, cave.address, &mut original)?;
    write_force(pid, cave.address, &assembly.bytes)?;
    patches.push(Patch {
        address: cave.address,
        original,
    });

    // Puis le détour, en une seule écriture.
    let mut detour = vec![0xE9];
    let relative = i32::try_from(cave.address as i64 - (anchor + 5) as i64)
        .map_err(|_| TuxError::Internal("zone trop éloignée pour un saut relatif".into()))?;
    detour.extend_from_slice(&relative.to_le_bytes());
    detour.resize(plan.stolen_bytes, 0x90);

    let mut original = vec![0u8; plan.stolen_bytes];
    memory::read(pid, anchor, &mut original)?;
    write_force(pid, anchor, &detour)?;
    patches.push(Patch {
        address: anchor,
        original,
    });

    // 5. Ne conserver que les symboles que le script publie.
    let mut published: HashMap<String, u64> = HashMap::new();
    for directive in &script.directives {
        if let Directive::Register(name) = directive {
            if let Some(address) = assembly.labels.get(name).or_else(|| symbols.get(name)) {
                published.insert(name.clone(), *address);
            }
        }
    }

    Ok(EnableReport {
        symbols: published,
        patches,
        allocation: cave.address,
    })
}

/// Remet le jeu dans son état d'origine.
pub fn disable(pid: u32, patches: &[Patch]) -> Result<()> {
    // Le détour est retiré avant le bloc injecté : l'ordre inverse laisserait
    // un saut vers du code effacé.
    for patch in patches.iter().rev() {
        write_force(pid, patch.address, &patch.original)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Script réel, publié pour ASKA.
    const SCRIPT: &str = r#"[ENABLE]
aobscanmodule(aska,GameAssembly.dll,48 8B 89 40 01 00 00 48 85 C9 74 18)
alloc(newmem,$1000,aska)
label(player)
label(code)
label(return)
registersymbol(player)
registersymbol(aska)

newmem:

code:
  mov [player],rcx
  mov rcx,[rcx+00000140]
  jmp return

player:
  dq 0

aska:
  jmp newmem
return:
"#;

    #[test]
    fn separates_directives_from_code() {
        let script = parse(SCRIPT);
        assert_eq!(
            script.directives.len(),
            7,
            "directives lues : {:?}",
            script.directives
        );
        assert!(matches!(
            &script.directives[0],
            Directive::Scan { symbol, module, .. }
                if symbol == "aska" && module == "GameAssembly.dll"
        ));
        assert!(matches!(
            &script.directives[1],
            Directive::Alloc { name, size, near }
                if name == "newmem" && *size == 0x1000 && near.as_deref() == Some("aska")
        ));
    }

    #[test]
    fn collects_the_blocks_in_order() {
        let script = parse(SCRIPT);
        let anchors: Vec<&str> = script
            .sections
            .iter()
            .map(|section| section.anchor.as_str())
            .collect();
        // « return: » en fait partie : c'est l'étiquette du point de reprise.
        assert_eq!(anchors, vec!["newmem", "code", "player", "aska", "return"]);
    }

    #[test]
    fn keeps_only_the_published_symbols() {
        let script = parse(SCRIPT);
        let registered: Vec<&str> = script
            .directives
            .iter()
            .filter_map(|directive| match directive {
                Directive::Register(name) => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(registered, vec!["player", "aska"]);
    }

    #[test]
    fn reads_cheat_engine_sizes_in_hexadecimal() {
        let script = parse("alloc(bloc,$2000,cible)\n");
        assert!(matches!(
            &script.directives[0],
            Directive::Alloc { size, .. } if *size == 0x2000
        ));
    }

    #[test]
    fn ignores_comments_and_markers() {
        let script = parse("[ENABLE]\n// rien\nregistersymbol(x) // fin\n");
        assert_eq!(script.directives.len(), 1);
    }
}
