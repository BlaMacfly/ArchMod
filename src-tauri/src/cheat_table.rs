//! Lecture des tables Cheat Engine (`.CT`).
//!
//! Une table est un document XML publié par la communauté : elle décrit, pour
//! une version précise d'un jeu, où trouver les valeurs intéressantes. C'est la
//! source de données qui permet à ArchMod de proposer des options toutes faites
//! plutôt que d'obliger l'utilisateur à chercher lui-même en mémoire.
//!
//! Toutes les entrées ne sont pas exploitables : les tables modernes s'appuient
//! sur des scripts d'auto-assembleur qui créent des symboles à l'exécution. On
//! les analyse quand même — motifs d'octets et symboles déclarés sont extraits —
//! pour pouvoir dire honnêtement à l'utilisateur ce qui marche et ce qui exige
//! encore le moteur d'injection.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::error::{Result, TuxError};

/// Type de la valeur pointée, tel que nommé par Cheat Engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "name")]
pub enum ValueType {
    Byte,
    TwoBytes,
    FourBytes,
    EightBytes,
    Float,
    Double,
    Text,
    Binary,
    /// Entrée qui n'est pas une valeur mais un script d'auto-assembleur.
    Script,
    Other(String),
}

impl ValueType {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "byte" => ValueType::Byte,
            "2 bytes" | "word" => ValueType::TwoBytes,
            "4 bytes" | "dword" => ValueType::FourBytes,
            "8 bytes" | "qword" => ValueType::EightBytes,
            "float" => ValueType::Float,
            "double" => ValueType::Double,
            "string" | "text" => ValueType::Text,
            "binary" | "array of byte" => ValueType::Binary,
            "auto assembler script" => ValueType::Script,
            _ => ValueType::Other(raw.trim().to_string()),
        }
    }

    /// Nombre d'octets à lire, quand il est fixe.
    pub fn size(&self) -> Option<usize> {
        match self {
            ValueType::Byte => Some(1),
            ValueType::TwoBytes => Some(2),
            ValueType::FourBytes | ValueType::Float => Some(4),
            ValueType::EightBytes | ValueType::Double => Some(8),
            _ => None,
        }
    }
}

/// Point de départ d'une adresse, avant application des décalages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AddressBase {
    /// `1A2B3C` — adresse absolue, rare et fragile.
    Absolute { address: u64 },
    /// `GameAssembly.dll+4194FD4` — résolvable immédiatement.
    Module { module: String, offset: i64 },
    /// `player+184` ou `[player]+184` — dépend d'un symbole créé par un script.
    ///
    /// Les crochets changent le sens : `[player]` lit le pointeur *rangé à*
    /// l'adresse du symbole, alors que `player` désigne l'adresse elle-même.
    Symbol {
        symbol: String,
        offset: i64,
        dereference: bool,
    },
}

/// Un scan de motif déclaré par un script : `aobscanmodule(sym,module,motif)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AobScan {
    pub symbol: String,
    pub module: String,
    pub pattern: String,
}

/// Ce qu'ArchMod sait faire d'une entrée, en l'état du moteur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state", content = "detail")]
pub enum Readiness {
    /// Adresse calculable tout de suite.
    Ready,
    /// Nécessite un symbole produit par un script d'auto-assembleur.
    NeedsSymbol(String),
    /// Entrée purement scriptée, ou sans adresse.
    Unsupported(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheatEntry {
    pub id: Option<u32>,
    pub description: String,
    pub value_type: ValueType,
    pub base: Option<AddressBase>,
    /// Décalages dans l'ordre du fichier : le premier est appliqué en dernier.
    pub offsets: Vec<i64>,
    /// Simple titre de section, sans valeur associée.
    pub group_header: bool,
    pub scans: Vec<AobScan>,
    pub registered_symbols: Vec<String>,
    pub children: Vec<CheatEntry>,
}

impl CheatEntry {
    pub fn readiness(&self) -> Readiness {
        if self.group_header {
            return Readiness::Unsupported("titre de section".into());
        }
        if self.value_type == ValueType::Script {
            return Readiness::Unsupported("script d'auto-assembleur".into());
        }
        match &self.base {
            Some(AddressBase::Symbol { symbol, .. }) => Readiness::NeedsSymbol(symbol.clone()),
            Some(_) => Readiness::Ready,
            None => Readiness::Unsupported("aucune adresse".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheatTable {
    pub version: Option<String>,
    pub entries: Vec<CheatEntry>,
}

impl CheatTable {
    pub fn parse(xml: &str) -> Result<Self> {
        let document = roxmltree::Document::parse(xml).map_err(|error| TuxError::CheatTable {
            detail: error.to_string(),
        })?;

        let root = document.root_element();
        if !root.has_tag_name("CheatTable") {
            return Err(TuxError::CheatTable {
                detail: format!(
                    "racine « {} » inattendue, « CheatTable » attendue",
                    root.tag_name().name()
                ),
            });
        }

        let entries = root
            .children()
            .find(|node| node.has_tag_name("CheatEntries"))
            .map(|node| parse_entries(node))
            .unwrap_or_default();

        Ok(Self {
            version: root
                .attribute("CheatEngineTableVersion")
                .map(str::to_string),
            entries,
        })
    }

    /// Toutes les entrées, groupes aplatis.
    pub fn flatten(&self) -> Vec<&CheatEntry> {
        fn walk<'a>(entries: &'a [CheatEntry], out: &mut Vec<&'a CheatEntry>) {
            for entry in entries {
                out.push(entry);
                walk(&entry.children, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.entries, &mut out);
        out
    }

    /// Tous les scans déclarés, quel que soit leur niveau d'imbrication.
    pub fn scans(&self) -> Vec<&AobScan> {
        self.flatten()
            .into_iter()
            .flat_map(|entry| entry.scans.iter())
            .collect()
    }

    /// Symboles dont dépendent les entrées, et qu'un script doit produire.
    pub fn required_symbols(&self) -> BTreeSet<String> {
        self.flatten()
            .into_iter()
            .filter_map(|entry| match entry.readiness() {
                Readiness::NeedsSymbol(symbol) => Some(symbol),
                _ => None,
            })
            .collect()
    }
}

fn parse_entries(node: roxmltree::Node) -> Vec<CheatEntry> {
    node.children()
        .filter(|child| child.has_tag_name("CheatEntry"))
        .map(parse_entry)
        .collect()
}

fn text_of(node: roxmltree::Node, tag: &str) -> Option<String> {
    node.children()
        .find(|child| child.has_tag_name(tag))?
        .text()
        .map(|text| text.trim().to_string())
}

fn parse_entry(node: roxmltree::Node) -> CheatEntry {
    // Cheat Engine entoure les descriptions de guillemets dans le fichier.
    let description = text_of(node, "Description")
        .unwrap_or_default()
        .trim_matches('"')
        .to_string();

    let scripts: Vec<String> = node
        .children()
        .filter(|child| child.has_tag_name("AssemblerScript") || child.has_tag_name("LuaScript"))
        .filter_map(|child| child.text())
        .map(str::to_string)
        .collect();

    let joined = scripts.join("\n");

    CheatEntry {
        id: text_of(node, "ID").and_then(|id| id.parse().ok()),
        description,
        value_type: text_of(node, "VariableType")
            .map(|raw| ValueType::parse(&raw))
            .unwrap_or(ValueType::Other(String::new())),
        base: text_of(node, "Address").and_then(|raw| parse_address(&raw)),
        offsets: node
            .children()
            .find(|child| child.has_tag_name("Offsets"))
            .map(|offsets| {
                offsets
                    .children()
                    .filter(|child| child.has_tag_name("Offset"))
                    .filter_map(|child| child.text())
                    .filter_map(|text| parse_hex(text.trim()))
                    .collect()
            })
            .unwrap_or_default(),
        group_header: text_of(node, "GroupHeader").as_deref() == Some("1"),
        scans: parse_scans(&joined),
        registered_symbols: parse_registered_symbols(&joined),
        children: node
            .children()
            .find(|child| child.has_tag_name("CheatEntries"))
            .map(parse_entries)
            .unwrap_or_default(),
    }
}

/// Entier hexadécimal signé, avec ou sans préfixe.
fn parse_hex(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    let (negative, digits) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let digits = digits.trim_start_matches("0x").trim_start_matches("0X");
    let value = i64::from_str_radix(digits, 16).ok()?;
    Some(if negative { -value } else { value })
}

/// `GameAssembly.dll+4194FD4`, `[player]+184`, `7FF6A1B2C3D4`.
fn parse_address(raw: &str) -> Option<AddressBase> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    // Le décalage est ce qui suit le dernier « + » de premier niveau.
    let (head, offset) = match raw.rsplit_once('+') {
        Some((head, tail)) => (head.trim(), parse_hex(tail).unwrap_or(0)),
        None => (raw, 0),
    };

    if let Some(symbol) = head.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return Some(AddressBase::Symbol {
            symbol: symbol.trim().to_string(),
            offset,
            dereference: true,
        });
    }

    // Un nom de module contient un point (« GameAssembly.dll »), une adresse non.
    if head.contains('.') {
        return Some(AddressBase::Module {
            module: head.to_string(),
            offset,
        });
    }

    match parse_hex(head) {
        Some(address) if offset == 0 => Some(AddressBase::Absolute {
            address: address as u64,
        }),
        Some(address) => Some(AddressBase::Absolute {
            address: (address + offset) as u64,
        }),
        // Ni module, ni adresse : c'est un symbole nu, comme « player+184 ».
        None => Some(AddressBase::Symbol {
            symbol: head.to_string(),
            offset,
            dereference: false,
        }),
    }
}

/// Extrait les arguments d'un appel `nom(a,b,c)` présent dans un script.
fn call_arguments<'a>(script: &'a str, function: &str) -> Vec<Vec<&'a str>> {
    let mut calls = Vec::new();
    for line in script.lines() {
        let line = line.trim();
        // Les lignes commentées ne comptent pas : CE désactive souvent un scan
        // de rechange en le préfixant de « // ».
        if line.starts_with("//") {
            continue;
        }
        let Some(start) = line.find(function) else {
            continue;
        };
        let rest = &line[start + function.len()..];
        let Some(inner) = rest
            .strip_prefix('(')
            .and_then(|inner| inner.split_once(')'))
            .map(|(inner, _)| inner)
        else {
            continue;
        };
        calls.push(inner.split(',').map(str::trim).collect());
    }
    calls
}

fn parse_scans(script: &str) -> Vec<AobScan> {
    call_arguments(script, "aobscanmodule")
        .into_iter()
        .filter_map(|arguments| {
            let [symbol, module, pattern @ ..] = arguments.as_slice() else {
                return None;
            };
            // Le motif contient des espaces mais jamais de virgule : ce qui suit
            // le module est donc le motif complet, recollé.
            let pattern = pattern.join(" ");
            (!pattern.is_empty()).then(|| AobScan {
                symbol: (*symbol).to_string(),
                module: (*module).to_string(),
                pattern,
            })
        })
        .collect()
}

fn parse_registered_symbols(script: &str) -> Vec<String> {
    let mut symbols: Vec<String> = call_arguments(script, "registersymbol")
        .into_iter()
        .filter_map(|arguments| arguments.first().map(|name| name.to_string()))
        .collect();
    symbols.sort();
    symbols.dedup();
    symbols
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extrait fidèle d'une table publiée pour ASKA : un script qui crée des
    /// symboles, une valeur qui en dépend, une valeur directement résolvable,
    /// et un groupe imbriqué.
    const TABLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<CheatTable CheatEngineTableVersion="45">
  <CheatEntries>
    <CheatEntry>
      <ID>52</ID>
      <Description>"Enable"</Description>
      <VariableType>Auto Assembler Script</VariableType>
      <AssemblerScript>[ENABLE]
aobscanmodule(aska,GameAssembly.dll,48 8B 89 40 01 00 00 48 85 C9 74 18)
alloc(newmem,$1000,aska)
registersymbol(player)
registersymbol(aska)
//aobscanmodule(vieux,GameAssembly.dll,90 90 90)
</AssemblerScript>
    </CheatEntry>
    <CheatEntry>
      <ID>8</ID>
      <Description>"Melee: Expertise Multiplier"</Description>
      <VariableType>Float</VariableType>
      <Address>[player]+190</Address>
      <Offsets>
        <Offset>6C</Offset>
      </Offsets>
    </CheatEntry>
    <CheatEntry>
      <ID>40</ID>
      <Description>"[ Time / Weather ]"</Description>
      <GroupHeader>1</GroupHeader>
      <CheatEntries>
        <CheatEntry>
          <ID>41</ID>
          <Description>"Day"</Description>
          <VariableType>4 Bytes</VariableType>
          <Address>GameAssembly.dll+4194FD4</Address>
        </CheatEntry>
      </CheatEntries>
    </CheatEntry>
  </CheatEntries>
</CheatTable>"#;

    #[test]
    fn reads_the_table_structure() {
        let table = CheatTable::parse(TABLE).expect("table valide");
        assert_eq!(table.version.as_deref(), Some("45"));
        assert_eq!(table.entries.len(), 3);
        // Le groupe apporte un enfant : quatre entrées une fois aplati.
        assert_eq!(table.flatten().len(), 4);
    }

    #[test]
    fn strips_quotes_from_descriptions() {
        let table = CheatTable::parse(TABLE).expect("table valide");
        assert_eq!(table.entries[1].description, "Melee: Expertise Multiplier");
    }

    #[test]
    fn classifies_what_is_usable() {
        let table = CheatTable::parse(TABLE).expect("table valide");
        let entries = table.flatten();

        assert_eq!(
            entries[0].readiness(),
            Readiness::Unsupported("script d'auto-assembleur".into())
        );
        assert_eq!(
            entries[1].readiness(),
            Readiness::NeedsSymbol("player".into())
        );
        assert_eq!(
            entries[2].readiness(),
            Readiness::Unsupported("titre de section".into())
        );
        assert_eq!(entries[3].readiness(), Readiness::Ready);
    }

    #[test]
    fn parses_the_three_address_forms() {
        let table = CheatTable::parse(TABLE).expect("table valide");
        assert_eq!(
            table.entries[1].base,
            Some(AddressBase::Symbol {
                symbol: "player".into(),
                offset: 0x190,
                dereference: true
            })
        );
        assert_eq!(
            table.flatten()[3].base,
            Some(AddressBase::Module {
                module: "GameAssembly.dll".into(),
                offset: 0x4194FD4
            })
        );
        assert_eq!(
            parse_address("7FF6A1B2C3D4"),
            Some(AddressBase::Absolute {
                address: 0x7FF6A1B2C3D4
            })
        );
    }

    #[test]
    fn distinguishes_bracketed_symbols_from_bare_ones() {
        assert_eq!(
            parse_address("[player]+184"),
            Some(AddressBase::Symbol {
                symbol: "player".into(),
                offset: 0x184,
                dereference: true
            })
        );
        assert_eq!(
            parse_address("aska+3"),
            Some(AddressBase::Symbol {
                symbol: "aska".into(),
                offset: 3,
                dereference: false
            })
        );
    }

    #[test]
    fn keeps_offsets_in_file_order() {
        let table = CheatTable::parse(TABLE).expect("table valide");
        assert_eq!(table.entries[1].offsets, vec![0x6C]);
    }

    #[test]
    fn extracts_scans_and_ignores_commented_ones() {
        let table = CheatTable::parse(TABLE).expect("table valide");
        let scans = table.scans();
        assert_eq!(scans.len(), 1, "le scan commenté doit être ignoré");
        assert_eq!(scans[0].symbol, "aska");
        assert_eq!(scans[0].module, "GameAssembly.dll");
        assert_eq!(scans[0].pattern, "48 8B 89 40 01 00 00 48 85 C9 74 18");
    }

    #[test]
    fn lists_symbols_that_scripts_must_provide() {
        let table = CheatTable::parse(TABLE).expect("table valide");
        let required = table.required_symbols();
        assert!(required.contains("player"));
        assert_eq!(required.len(), 1);
        assert_eq!(
            table.entries[0].registered_symbols,
            vec!["aska".to_string(), "player".to_string()]
        );
    }

    #[test]
    fn rejects_documents_that_are_not_tables() {
        assert!(CheatTable::parse("<html><body/></html>").is_err());
        assert!(CheatTable::parse("pas du xml").is_err());
    }

    #[test]
    fn maps_value_types_to_sizes() {
        assert_eq!(ValueType::parse("4 Bytes").size(), Some(4));
        assert_eq!(ValueType::parse("Float").size(), Some(4));
        assert_eq!(ValueType::parse("Byte").size(), Some(1));
        assert_eq!(ValueType::parse("Double").size(), Some(8));
        assert_eq!(ValueType::parse("String").size(), None);
    }
}
