//! Pose un détour réel dans un jeu en cours, capture le pointeur que le code
//! manipule, puis restaure immédiatement le code d'origine.
//!
//! ```sh
//! cargo run --release --example pose_hook -- <pid> <module> "<motif>"
//! ```

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

    let module = memory::find_module(pid, &module_name).expect("module");
    let pattern = Pattern::parse(&source).expect("motif");
    let target = *memory::scan_module(pid, &module, &pattern)
        .expect("scan")
        .first()
        .expect("motif introuvable");

    let plan = hook::plan(pid, &module, target).expect("plan");
    assert!(
        plan.is_feasible(),
        "détour non réalisable : {:?}",
        plan.blockers
    );

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
    let mut trampoline = vec![0x48, 0x89, 0x0D];
    trampoline.extend_from_slice(&rel32(cave + 7, storage).to_le_bytes());
    // Instruction volée, rejouée à l'identique (elle ne dépend pas de sa position).
    trampoline.extend_from_slice(&original);
    // Retour juste après la zone patchée.
    let jump_position = cave + trampoline.len() as u64;
    trampoline.push(0xE9);
    trampoline.extend_from_slice(&rel32(jump_position + 5, plan.resume).to_le_bytes());

    println!("trampoline ({} octets) : {trampoline:02X?}", trampoline.len());
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
    println!("→ détour posé, attente de 3 s pour que le jeu passe dedans…");

    thread::sleep(Duration::from_secs(3));
    let captured = memory::read_u64(pid, storage).expect("lecture du stockage");

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

    // --- résultat -----------------------------------------------------------
    if captured == 0 {
        println!("\nrien n'a été capturé : le jeu n'est pas passé par cette instruction.");
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
