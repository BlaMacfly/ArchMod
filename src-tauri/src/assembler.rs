//! Assembleur x86-64 minimal, pour les scripts d'auto-assembleur de Cheat
//! Engine.
//!
//! Le langage de CE est vaste ; on n'en couvre ici que les formes dont un
//! *hook de capture* a besoin — celui qui détourne une instruction, range un
//! registre quelque part, rejoue l'instruction volée et repart. C'est le motif
//! qui débloque les tables modernes, où toutes les entrées dépendent d'un
//! symbole du type `[player]`.
//!
//! Le parti pris est de **refuser ce qu'on ne sait pas encoder** plutôt que de
//! deviner : une instruction mal assemblée ne produit pas un message d'erreur,
//! elle fait planter le jeu.

use std::collections::HashMap;

use serde::Serialize;

use crate::error::{Result, TuxError};

/// Registres 64 bits, dans l'ordre de leur encodage.
const REGISTERS: [&str; 16] = [
    "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13",
    "r14", "r15",
];

fn register(name: &str) -> Option<u8> {
    REGISTERS
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(name))
        .map(|index| index as u8)
}

/// Entier décimal, ou hexadécimal si suffixé/préfixé à la manière de CE.
fn number(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    let (negative, rest) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let value = if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()?
    } else if let Some(hex) = rest.strip_prefix('$') {
        i64::from_str_radix(hex, 16).ok()?
    } else if rest.chars().all(|c| c.is_ascii_hexdigit()) && rest.len() > 1 {
        // Cheat Engine écrit ses constantes en hexadécimal sans préfixe :
        // « mov rcx,[rcx+00000140] » désigne bien 0x140.
        i64::from_str_radix(rest, 16).ok()?
    } else {
        rest.parse().ok()?
    };
    Some(if negative { -value } else { value })
}

/// Ce qu'une ligne de script produit.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    /// Déclaration d'étiquette : `code:`
    Label(String),
    /// `mov [étiquette], reg`
    StoreToLabel {
        label: String,
        source: u8,
    },
    /// `mov reg, [étiquette]`
    LoadFromLabel {
        destination: u8,
        label: String,
    },
    /// `mov reg, [base+déplacement]`
    LoadIndirect {
        destination: u8,
        base: u8,
        displacement: i32,
    },
    /// `mov destination, source`
    MoveRegister {
        destination: u8,
        source: u8,
    },
    /// `jmp étiquette`
    Jump(String),
    Nop,
    /// Données brutes : `db`, `dd`, `dq`
    Bytes(Vec<u8>),
}

impl Item {
    /// Taille encodée, connue avant de résoudre les étiquettes : c'est ce qui
    /// permet la mise en page en deux passes.
    fn size(&self) -> usize {
        match self {
            Item::Label(_) => 0,
            Item::StoreToLabel { .. } | Item::LoadFromLabel { .. } => 7,
            Item::LoadIndirect { .. } => 7,
            Item::MoveRegister { .. } => 3,
            Item::Jump(_) => 5,
            Item::Nop => 1,
            Item::Bytes(bytes) => bytes.len(),
        }
    }
}

fn unsupported(line: &str) -> TuxError {
    TuxError::Assembly {
        line: line.to_string(),
        detail: "forme non prise en charge par l'assembleur d'ArchMod".into(),
    }
}

/// Découpe `[rcx+140]` en base et déplacement.
fn parse_indirect(operand: &str) -> Option<(String, i64)> {
    let inner = operand.trim().strip_prefix('[')?.strip_suffix(']')?.trim();
    match inner.split_once('+') {
        Some((base, offset)) => Some((base.trim().to_string(), number(offset)?)),
        None => match inner.split_once('-') {
            Some((base, offset)) => Some((base.trim().to_string(), -number(offset)?)),
            None => Some((inner.to_string(), 0)),
        },
    }
}

/// Analyse un bloc d'assembleur.
fn parse(source: &str) -> Result<Vec<Item>> {
    let mut items = Vec::new();

    for raw in source.lines() {
        // Les commentaires de CE sont introduits par `//`.
        let line = raw.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        // Les directives de structure sont traitées ailleurs.
        let lower = line.to_ascii_lowercase();
        if lower.starts_with('[') && (lower.contains("enable") || lower.contains("disable")) {
            continue;
        }

        if let Some(label) = line.strip_suffix(':') {
            items.push(Item::Label(label.trim().to_ascii_lowercase()));
            continue;
        }

        let (mnemonic, operands) = match line.split_once(char::is_whitespace) {
            Some((mnemonic, rest)) => (mnemonic.to_ascii_lowercase(), rest.trim()),
            None => (line.to_ascii_lowercase(), ""),
        };

        match mnemonic.as_str() {
            "nop" => items.push(Item::Nop),
            "jmp" => items.push(Item::Jump(operands.trim().to_ascii_lowercase())),
            "db" | "dd" | "dq" => {
                let width = match mnemonic.as_str() {
                    "db" => 1,
                    "dd" => 4,
                    _ => 8,
                };
                let mut bytes = Vec::new();
                for token in operands.split([',', ' ']).filter(|t| !t.is_empty()) {
                    let value = number(token).ok_or_else(|| unsupported(line))?;
                    bytes.extend_from_slice(&value.to_le_bytes()[..width]);
                }
                items.push(Item::Bytes(bytes));
            }
            "mov" => {
                let (left, right) = operands.split_once(',').ok_or_else(|| unsupported(line))?;
                let (left, right) = (left.trim(), right.trim());

                let item = if left.starts_with('[') {
                    // Destination en mémoire : seule la forme « [étiquette], reg »
                    // est couverte — c'est celle de la capture.
                    let source = register(right).ok_or_else(|| unsupported(line))?;
                    let (base, offset) = parse_indirect(left).ok_or_else(|| unsupported(line))?;
                    if register(&base).is_some() || offset != 0 {
                        return Err(unsupported(line));
                    }
                    Item::StoreToLabel {
                        label: base.to_ascii_lowercase(),
                        source,
                    }
                } else if right.starts_with('[') {
                    let destination = register(left).ok_or_else(|| unsupported(line))?;
                    let (base, offset) = parse_indirect(right).ok_or_else(|| unsupported(line))?;
                    match register(&base) {
                        Some(base) => Item::LoadIndirect {
                            destination,
                            base,
                            displacement: i32::try_from(offset).map_err(|_| unsupported(line))?,
                        },
                        None => Item::LoadFromLabel {
                            destination,
                            label: base.to_ascii_lowercase(),
                        },
                    }
                } else {
                    let destination = register(left).ok_or_else(|| unsupported(line))?;
                    let source = register(right).ok_or_else(|| unsupported(line))?;
                    Item::MoveRegister {
                        destination,
                        source,
                    }
                };
                items.push(item);
            }
            _ => return Err(unsupported(line)),
        }
    }
    Ok(items)
}

/// Code assemblé, prêt à être écrit dans le processus.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assembly {
    pub bytes: Vec<u8>,
    /// Adresse absolue de chaque étiquette.
    pub labels: HashMap<String, u64>,
}

/// Préfixe REX pour un accès 64 bits.
fn rex(reg: u8, rm: u8) -> u8 {
    0x48 | ((reg >> 3) << 2) | (rm >> 3)
}

/// Assemble un bloc à l'adresse `base`, en résolvant les étiquettes.
///
/// Les étiquettes externes — celles produites par un `aobscanmodule` ou par le
/// point de retour du détour — sont fournies par `external`.
pub fn assemble(source: &str, base: u64, external: &HashMap<String, u64>) -> Result<Assembly> {
    let items = parse(source)?;

    // Première passe : chaque étiquette reçoit son adresse.
    let mut labels: HashMap<String, u64> = external.clone();
    let mut cursor = base;
    for item in &items {
        if let Item::Label(name) = item {
            labels.insert(name.clone(), cursor);
        }
        cursor += item.size() as u64;
    }

    // Seconde passe : encodage, toutes les cibles étant connues.
    let mut bytes = Vec::new();
    let mut address = base;

    let resolve = |name: &str| -> Result<u64> {
        labels.get(name).copied().ok_or_else(|| TuxError::Assembly {
            line: name.to_string(),
            detail: "étiquette inconnue".into(),
        })
    };

    for item in &items {
        let next = address + item.size() as u64;
        match item {
            Item::Label(_) => {}
            Item::Nop => bytes.push(0x90),
            Item::Bytes(raw) => bytes.extend_from_slice(raw),
            Item::MoveRegister {
                destination,
                source,
            } => {
                bytes.push(rex(*source, *destination));
                bytes.push(0x89);
                bytes.push(0xC0 | ((source & 7) << 3) | (destination & 7));
            }
            Item::StoreToLabel { label, source } => {
                // mov [rip+disp32], reg
                let target = resolve(label)?;
                bytes.push(rex(*source, 0));
                bytes.push(0x89);
                bytes.push(0x05 | ((source & 7) << 3));
                bytes.extend_from_slice(&relative(next, target, label)?.to_le_bytes());
            }
            Item::LoadFromLabel { destination, label } => {
                // mov reg, [rip+disp32]
                let target = resolve(label)?;
                bytes.push(rex(*destination, 0));
                bytes.push(0x8B);
                bytes.push(0x05 | ((destination & 7) << 3));
                bytes.extend_from_slice(&relative(next, target, label)?.to_le_bytes());
            }
            Item::LoadIndirect {
                destination,
                base: source,
                displacement,
            } => {
                // mov reg, [base+disp32] — déplacement toujours sur 32 bits,
                // pour que la taille soit connue dès la première passe.
                bytes.push(rex(*destination, *source));
                bytes.push(0x8B);
                bytes.push(0x80 | ((destination & 7) << 3) | (source & 7));
                if source & 7 == 4 {
                    // RSP et R12 exigent un octet SIB.
                    return Err(TuxError::Assembly {
                        line: format!("mov reg,[{}+…]", REGISTERS[*source as usize]),
                        detail: "base RSP/R12 non prise en charge".into(),
                    });
                }
                bytes.extend_from_slice(&displacement.to_le_bytes());
            }
            Item::Jump(label) => {
                let target = resolve(label)?;
                bytes.push(0xE9);
                bytes.extend_from_slice(&relative(next, target, label)?.to_le_bytes());
            }
        }
        address = next;
    }

    Ok(Assembly { bytes, labels })
}

/// Déplacement relatif, refusé s'il dépasse la portée d'un champ 32 bits.
fn relative(from: u64, to: u64, label: &str) -> Result<i32> {
    i32::try_from(to as i64 - from as i64).map_err(|_| TuxError::Assembly {
        line: label.to_string(),
        detail: "cible hors de portée d'un saut relatif de 32 bits".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn externals(pairs: &[(&str, u64)]) -> HashMap<String, u64> {
        pairs
            .iter()
            .map(|(name, address)| (name.to_string(), *address))
            .collect()
    }

    #[test]
    fn reads_cheat_engine_hexadecimal_constants() {
        // CE écrit ses déplacements en hexadécimal, sans préfixe.
        assert_eq!(number("00000140"), Some(0x140));
        assert_eq!(number("0x1F"), Some(0x1F));
        assert_eq!(number("$1000"), Some(0x1000));
        assert_eq!(number("-8"), Some(-8));
    }

    #[test]
    fn assembles_the_capture_hook_of_a_real_table() {
        // Le bloc exact d'une table publiée pour ASKA.
        let source = "
            newmem:
            code:
              mov [player],rcx
              mov rcx,[rcx+00000140]
              jmp return
            player:
              dq 0
        ";
        let assembly =
            assemble(source, 0x1000, &externals(&[("return", 0x2000)])).expect("assemblage");

        // mov [rip+d],rcx : d vise « player », placé après les trois instructions.
        assert_eq!(&assembly.bytes[0..3], &[0x48, 0x89, 0x0D]);
        let displacement = i32::from_le_bytes(assembly.bytes[3..7].try_into().unwrap());
        assert_eq!(0x1000 + 7 + displacement as u64, assembly.labels["player"]);

        // mov rcx,[rcx+140]
        assert_eq!(&assembly.bytes[7..11], &[0x48, 0x8B, 0x89, 0x40]);
        assert_eq!(
            i32::from_le_bytes(assembly.bytes[10..14].try_into().unwrap()),
            0x140
        );

        // jmp return
        assert_eq!(assembly.bytes[14], 0xE9);
        let jump = i32::from_le_bytes(assembly.bytes[15..19].try_into().unwrap());
        assert_eq!(0x1000 + 19 + jump as u64, 0x2000);

        // Les huit octets de « dq 0 » closent le bloc.
        assert_eq!(assembly.bytes.len(), 19 + 8);
        assert_eq!(assembly.labels["player"], 0x1000 + 19);
    }

    #[test]
    fn refuses_what_it_cannot_encode() {
        let cases = [
            "  xchg rax,rbx",    // mnémonique absent du sous-ensemble
            "  mov rax,[rsp+8]", // base RSP, octet SIB requis
            "  mov [rcx+8],rax", // destination indirecte avec base
            "  add rax,1",       // arithmétique non couverte
        ];
        for case in cases {
            assert!(
                assemble(case, 0x1000, &HashMap::new()).is_err(),
                "« {case} » aurait dû être refusé"
            );
        }
    }

    #[test]
    fn reports_an_unknown_label_instead_of_guessing() {
        let error =
            assemble("  jmp nulle_part", 0x1000, &HashMap::new()).expect_err("doit échouer");
        assert!(error.to_string().contains("étiquette inconnue"));
    }

    #[test]
    fn refuses_a_target_out_of_relative_reach() {
        let far = externals(&[("return", 0x8000_0000_0000)]);
        assert!(assemble("  jmp return", 0x1000, &far).is_err());
    }

    #[test]
    fn ignores_comments_and_section_markers() {
        let assembly = assemble(
            "[ENABLE]\n// un commentaire\n  nop\n  nop // en fin de ligne\n",
            0x1000,
            &HashMap::new(),
        )
        .expect("assemblage");
        assert_eq!(assembly.bytes, vec![0x90, 0x90]);
    }
}
