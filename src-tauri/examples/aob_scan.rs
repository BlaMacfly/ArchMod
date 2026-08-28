//! Diagnostic : retrouve dans un jeu en cours les motifs d'octets d'une table
//! Cheat Engine, comme le ferait `aobscanmodule`.
//!
//! ```sh
//! cargo run --release --example aob_scan -- <pid> <module> "48 8B ?? 40 01" [motif...]
//! ```

use std::time::Instant;

use archmod_lib::memory::{find_module, scan_module, Pattern};

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [pid, module_name, patterns @ ..] = arguments.as_slice() else {
        eprintln!("usage : aob_scan <pid> <module> \"<motif>\" [motif...]");
        std::process::exit(2);
    };
    if patterns.is_empty() {
        eprintln!("aucun motif fourni");
        std::process::exit(2);
    }

    let pid: u32 = pid.parse().unwrap_or_else(|_| {
        eprintln!("PID invalide : {pid}");
        std::process::exit(2);
    });

    let module = match find_module(pid, module_name) {
        Ok(module) => module,
        Err(error) => {
            eprintln!("module introuvable : {error}");
            std::process::exit(1);
        }
    };
    println!(
        "module {} : base {:#x}, taille {:.1} Mo",
        module.name,
        module.base,
        module.size as f64 / 1_048_576.0
    );

    let mut failures = 0;
    for source in patterns {
        let pattern = match Pattern::parse(source) {
            Ok(pattern) => pattern,
            Err(error) => {
                eprintln!("  motif rejeté : {error}");
                failures += 1;
                continue;
            }
        };

        let started = Instant::now();
        match scan_module(pid, &module, &pattern) {
            Ok(hits) => {
                println!(
                    "\n  motif « {source} »\n    {} correspondance(s) en {} ms",
                    hits.len(),
                    started.elapsed().as_millis()
                );
                for hit in hits.iter().take(5) {
                    println!("    {hit:#x}  =  {}+{:#x}", module.name, hit - module.base);
                }
                if hits.is_empty() {
                    failures += 1;
                }
            }
            Err(error) => {
                eprintln!("    balayage impossible : {error}");
                failures += 1;
            }
        }
    }
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
