//! Démonstration bout-en-bout : table Cheat Engine → jeu en cours.
//!
//! ```sh
//! cargo run --release --example table_demo -- <pid>
//! ```
//!
//! La table intégrée reprend un motif réellement publié pour ASKA. L'entrée
//! « Sonde » s'appuie sur le symbole que le scan produit : elle démontre la
//! chaîne complète parseur → scan → symbole → adresse → valeur.

use archmod_lib::cheat_table::{CheatTable, Readiness};
use archmod_lib::engine::Session;

const TABLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<CheatTable CheatEngineTableVersion="45">
  <CheatEntries>
    <CheatEntry>
      <ID>1</ID>
      <Description>"Enable"</Description>
      <VariableType>Auto Assembler Script</VariableType>
      <AssemblerScript>[ENABLE]
aobscanmodule(aska,GameAssembly.dll,48 8B 89 40 01 00 00 48 85 C9 74 18)
registersymbol(aska)
</AssemblerScript>
    </CheatEntry>
    <CheatEntry>
      <ID>2</ID>
      <Description>"Sonde : instruction accrochée"</Description>
      <VariableType>4 Bytes</VariableType>
      <Address>aska+3</Address>
    </CheatEntry>
    <CheatEntry>
      <ID>3</ID>
      <Description>"Sonde : en-tête du module"</Description>
      <VariableType>2 Bytes</VariableType>
      <Address>GameAssembly.dll+0</Address>
    </CheatEntry>
    <CheatEntry>
      <ID>4</ID>
      <Description>"Option dépendant d'un script"</Description>
      <VariableType>Float</VariableType>
      <Address>[player]+190</Address>
    </CheatEntry>
  </CheatEntries>
</CheatTable>"#;

fn main() {
    let pid: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .expect("usage : table_demo <pid>");

    let table = CheatTable::parse(TABLE).expect("table valide");
    println!(
        "table version {} — {} entrées",
        table.version.as_deref().unwrap_or("?"),
        table.flatten().len()
    );

    let mut session = Session::new(pid);

    println!("\n— scans de motifs —");
    for outcome in session.run_scans(&table) {
        match (outcome.resolved(), &outcome.error) {
            (Some(address), _) => println!(
                "  {} = {address:#x}  ({} correspondance(s), {} ms)",
                outcome.symbol,
                outcome.matches.len(),
                outcome.elapsed_ms
            ),
            (None, Some(error)) => println!("  {} : {error}", outcome.symbol),
            (None, None) => println!("  {} : aucune correspondance", outcome.symbol),
        }
    }

    println!("\n— options —");
    for entry in table.flatten() {
        match entry.readiness() {
            Readiness::Unsupported(reason) => {
                println!("  [ignorée] {} ({reason})", entry.description)
            }
            _ => match session.read(entry) {
                Ok(value) => {
                    let address = session.resolve(entry).expect("adresse déjà résolue");
                    println!("  [OK]      {} = {value:?} @ {address:#x}", entry.description)
                }
                Err(error) => println!("  [échec]   {} : {error}", entry.description),
            },
        }
    }
}
