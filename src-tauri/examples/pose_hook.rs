//! Pose un détour réel dans un jeu en cours, capture le pointeur que le code
//! manipule, puis restaure immédiatement le code d'origine.
//!
//! ```sh
//! cargo run --release --example pose_hook -- <pid> <module> "<motif>" [secondes]
//! ```
//!
//! Le détour reste posé jusqu'à la capture, sans dépasser le délai demandé :
//! plus tôt le jeu passe dans l'instruction, plus courte est la fenêtre pendant
//! laquelle il exécute du code modifié.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::{thread, time::Duration};

use archmod_lib::hook;
use archmod_lib::memory::{self, Pattern};

/// Écrit en ignorant les protections de page, ce que `process_vm_writev` ne
/// sait pas faire : `/proc/<pid>/mem` accède à la mémoire avec `FOLL_FORCE`.
fn write_force(pid: u32, address: u64, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .open(format!("/proc/{pid}/mem"))?;
    file.seek(SeekFrom::Start(address))?;
    file.write_all(bytes)
}

fn read_bytes(pid: u32, address: u64, length: usize) -> Vec<u8> {
    let mut buffer = vec![0u8; length];
    memory::read(pid, address, &mut buffer).expect("lecture");
    buffer
}

fn rel32(from_end: u64, to: u64) -> i32 {
    let delta = to as i64 - from_end as i64;
    i32::try_from(delta).expect("saut hors de portée")
}

fn main() {
    let pid: u32 = std::env::args().nth(1).unwrap().parse().unwrap();
    let module_name = std::env::args().nth(2).unwrap();
    let source = std::env::args().nth(3).unwrap();
    let timeout = Duration::from_secs(
        std::env::args()
            .nth(4)
            .and_then(|a| a.parse().ok())
            .unwrap_or(30),
    );
    // Un motif peut correspondre à plusieurs endroits : on choisit lequel viser.
    let index: usize = std::env::args()
        .nth(5)
        .and_then(|a| a.parse().ok())
        .unwrap_or(0);
    // Décalage à l'intérieur du motif : les scripts CE écrivent « sym+04: »
    // quand le motif englobe la fin de l'instruction précédente ou un
    // alignement de fonction.
    let hook_offset: u64 = std::env::args()
        .nth(6)
        .and_then(|a| a.parse().ok())
        .unwrap_or(0);

    let module = memory::find_module(pid, &module_name).expect("module");
    let pattern = Pattern::parse(&source).expect("motif");
    let hits = memory::scan_module(pid, &module, &pattern).expect("scan");
    println!("{} correspondance(s) pour ce motif", hits.len());
    let Some(found) = hits.get(index) else {
        println!("pas de correspondance n°{index} (il y en a {})", hits.len());
        return;
    };
    let target = found + hook_offset;

    let plan = match hook::plan(pid, &module, target) {
        Ok(plan) => plan,
        Err(error) => {
            println!("\nanalyse impossible : {error}");
            let mut apercu = [0u8; 16];
            if memory::read(pid, target, &mut apercu).is_ok() {
                println!("octets à {target:#x} : {apercu:02X?}");
                println!(
                    "Si le motif englobe la fin de l'instruction précédente ou un \
                     alignement, indique un décalage en sixième argument (souvent 4)."
                );
            }
            return;
        }
    };
    if !plan.is_feasible() {
        println!("\ndétour non réalisable :");
        for blocker in &plan.blockers {
            println!("  - {blocker}");
        }
        return;
    }

    let cave = plan.cave.as_ref().unwrap().address;
    // On veut une zone franchement vide pour le stockage : 64 octets nuls ont
    // beaucoup moins de chances d'être des données vivantes que 8.
    let storage = hook::find_cave(pid, &module, 64, false, true)
        .expect("recherche")
        .expect("zone inscriptible")
        .address;

    println!(
        "cible      {target:#x}   ({module_name}+{:#x})",
        target - module.base
    );
    println!("trampoline {cave:#x}");
    println!("stockage   {storage:#x}");

    // --- garde-fous ---------------------------------------------------------
    let original = read_bytes(pid, target, plan.stolen_bytes);
    println!("\noctets d'origine : {original:02X?}");
    assert!(
        read_bytes(pid, cave, 32).iter().all(|b| *b == 0x00),
        "la zone de trampoline n'est plus vierge"
    );
    assert!(
        read_bytes(pid, storage, 64).iter().all(|b| *b == 0x00),
        "la zone de stockage n'est plus vierge"
    );

    // --- trampoline ---------------------------------------------------------
    // mov [rip+disp32],rcx : capture du pointeur, adressé relativement à RIP
    // pour rester valable quelle que soit l'adresse de chargement.
    // Compteur de passages, à storage+8 : il distingue « instruction jamais
    // exécutée » de « exécutée mais registre nul ».
    // inc dword [rip+disp32]
    let mut trampoline = vec![0xFF, 0x05];
    trampoline.extend_from_slice(&rel32(cave + 6, storage + 8).to_le_bytes());
    // mov [rip+disp32],rcx — capture du pointeur.
    trampoline.extend_from_slice(&[0x48, 0x89, 0x0D]);
    let position = cave + trampoline.len() as u64 + 4;
    trampoline.extend_from_slice(&rel32(position, storage).to_le_bytes());
    // Instruction volée, rejouée à l'identique (elle ne dépend pas de sa position).
    trampoline.extend_from_slice(&original);
    // Retour juste après la zone patchée.
    let jump_position = cave + trampoline.len() as u64;
    trampoline.push(0xE9);
    trampoline.extend_from_slice(&rel32(jump_position + 5, plan.resume).to_le_bytes());

    println!(
        "trampoline ({} octets) : {trampoline:02X?}",
        trampoline.len()
    );
    write_force(pid, cave, &trampoline).expect("écriture de la trampoline");
    assert_eq!(
        read_bytes(pid, cave, trampoline.len()),
        trampoline,
        "trampoline mal écrite"
    );
    println!("→ trampoline en place et vérifiée");

    // --- détour -------------------------------------------------------------
    let mut patch = vec![0xE9];
    patch.extend_from_slice(&rel32(target + 5, cave).to_le_bytes());
    // Bourrage : le reste de l'instruction volée ne doit jamais être exécuté.
    patch.resize(plan.stolen_bytes, 0x90);

    println!("\npatch : {patch:02X?}");
    // Une seule écriture : la fenêtre pendant laquelle un thread pourrait voir
    // un code à moitié réécrit se réduit à un appel système.
    write_force(pid, target, &patch).expect("écriture du détour");
    println!(
        "→ détour posé. JOUE MAINTENANT : bouge, frappe, ouvre l'inventaire.\n  \
         Surveillance pendant {} s au plus…",
        timeout.as_secs()
    );

    // Sondage rapide plutôt qu'attente fixe : on restaure dès que possible.
    let started = std::time::Instant::now();
    let mut captured = 0u64;
    while started.elapsed() < timeout {
        captured = memory::read_u64(pid, storage).unwrap_or(0);
        if captured != 0 {
            println!("→ capture après {:.1} s", started.elapsed().as_secs_f32());
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    // --- restauration -------------------------------------------------------
    write_force(pid, target, &original).expect("restauration");
    let restored = read_bytes(pid, target, plan.stolen_bytes);
    println!(
        "\ncode restauré : {restored:02X?}  ({})",
        if restored == original {
            "identique"
        } else {
            "DIVERGENT"
        }
    );

    let passages = memory::read_u32(pid, storage + 8).unwrap_or(0);
    println!("passages dans l'instruction : {passages}");

    // Rapport sur disque : la sortie du terminal n'est pas toujours récupérable.
    let mut rapport = String::new();
    let _ = writeln!(rapport, "cible      {target:#x}");
    let _ = writeln!(rapport, "trampoline {cave:#x}");
    let _ = writeln!(rapport, "stockage   {storage:#x}");
    let _ = writeln!(rapport, "passages   {passages}");
    let _ = writeln!(rapport, "capture    {captured:#x}");
    let _ = writeln!(
        rapport,
        "restaure   {}",
        if restored == original { "oui" } else { "NON" }
    );
    if captured != 0 {
        for (nom, offset) in [("Dead", 0x184u64), ("PVP", 0x384)] {
            let mut byte = [0u8; 1];
            let valeur = match memory::read(pid, captured + offset, &mut byte) {
                Ok(()) => byte[0].to_string(),
                Err(_) => "illisible".into(),
            };
            let _ = writeln!(rapport, "{nom:<10} {valeur}");
        }
    }
    let chemin = "/tmp/archmod-hook-rapport.txt";
    if std::fs::write(chemin, &rapport).is_ok() {
        println!("rapport écrit dans {chemin}");
    }

    // --- résultat -----------------------------------------------------------
    if captured == 0 {
        if passages == 0 {
            println!(
                "\nl'instruction n'a JAMAIS été exécutée en {} s : ce chemin de code est \
                 inactif dans cette version du jeu, ou le motif correspond ailleurs qu'à \
                 l'endroit visé par la table.",
                timeout.as_secs()
            );
        } else {
            println!(
                "\ninstruction exécutée {passages} fois, mais le registre rcx valait zéro : \
                 le point d'accroche n'est pas celui qu'attend la table."
            );
        }
        return;
    }
    println!("\nPOINTEUR CAPTURÉ : {captured:#x}   ← c'est le symbole « player »");

    // Deux entrées de la table publiée, résolues grâce à cette capture.
    for (nom, offset) in [("Dead (Yes/No)", 0x184u64), ("PVP Status", 0x384)] {
        let address = captured + offset;
        let mut byte = [0u8; 1];
        match memory::read(pid, address, &mut byte) {
            Ok(()) => println!(
                "  [player]+{offset:X} — {nom:<16} = {} @ {address:#x}",
                byte[0]
            ),
            Err(error) => println!("  [player]+{offset:X} — {nom:<16} illisible : {error}"),
        }
    }
}
