//! Dry-run : que faudrait-il pour détourner l'instruction visée par une table ?
//! Aucune écriture, uniquement de la lecture et du décodage.

use archmod_lib::memory::{self, Pattern};

fn main() {
    let pid: u32 = std::env::args().nth(1).unwrap().parse().unwrap();
    let module_name = std::env::args().nth(2).unwrap();
    let source = std::env::args().nth(3).unwrap();

    let module = memory::find_module(pid, &module_name).expect("module");
    let pattern = Pattern::parse(&source).expect("motif");
    let hits = memory::scan_module(pid, &module, &pattern).expect("scan");
    let Some(target) = hits.first().copied() else {
        println!("motif introuvable");
        return;
    };
    println!(
        "cible {target:#x}  ({}+{:#x})",
        module.name,
        target - module.base
    );

    let plan = archmod_lib::hook::plan(pid, &module, target).expect("plan");

    println!("\ninstructions volées ({} octets) :", plan.stolen_bytes);
    for instruction in &plan.stolen {
        println!(
            "  {:#x}  {:<28} {} octet(s){}",
            instruction.address,
            instruction.text.trim(),
            instruction.length,
            if instruction.position_dependent {
                "   ← dépend de sa position"
            } else {
                ""
            }
        );
    }
    println!("reprise après le détour : {:#x}", plan.resume);

    let decrire = |titre: &str, cave: &Option<archmod_lib::hook::Cave>| match cave {
        Some(cave) => println!(
            "  {titre:<12} {:#x}  {} octets, remplissage {:#04x}{}{}  (distance {:#x})",
            cave.address,
            cave.size,
            cave.filler,
            if cave.executable { ", exécutable" } else { "" },
            if cave.writable { ", inscriptible" } else { "" },
            (cave.address as i64 - target as i64).abs()
        ),
        None => println!("  {titre:<12} aucun"),
    };
    println!("\nemplacements retenus :");
    decrire("trampoline", &plan.cave);
    decrire("stockage", &plan.storage);

    if plan.blockers.is_empty() {
        println!("\nVERDICT : détour réalisable en l'état.");
    } else {
        println!("\nVERDICT : {} obstacle(s)", plan.blockers.len());
        for blocker in &plan.blockers {
            println!("  - {blocker}");
        }
    }
}
