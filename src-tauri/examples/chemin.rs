//! Cherche les chemins de pointeurs menant à une adresse.
fn main() {
    let pid: u32 = std::env::args().nth(1).unwrap().parse().unwrap();
    let cible = u64::from_str_radix(
        std::env::args().nth(2).unwrap().trim_start_matches("0x"), 16).unwrap();

    let rapport = archmod_lib::pointer::scan(pid, cible, Default::default()).expect("recherche");
    println!(
        "{} chemin(s) parmi {} pointeurs, en {} ms",
        rapport.paths.len(), rapport.pointers, rapport.elapsed_ms
    );
    for chemin in rapport.paths.iter().take(8) {
        let atteint = archmod_lib::pointer::resolve(pid, chemin);
        println!(
            "  {:<52} {} niveau(x)  {}",
            chemin.display(),
            chemin.offsets.len(),
            match atteint {
                Ok(a) if a == cible => "vérifié ✓".to_string(),
                Ok(a) => format!("mène à {a:#x} ✗"),
                Err(e) => format!("irrésolu : {e}"),
            }
        );
    }
}
