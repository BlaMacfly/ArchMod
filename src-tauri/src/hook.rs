//! Analyse préalable à la pose d'un détour (« hook ») dans le code d'un jeu.
//!
//! Détourner une instruction consiste à la remplacer par un saut vers du code à
//! nous. Deux conditions doivent être réunies avant d'écrire quoi que ce soit :
//!
//! - un saut relatif occupe 5 octets, or on ne peut pas couper une instruction
//!   en deux : il faut donc « voler » un nombre entier d'instructions d'au moins
//!   cette taille, et les rejouer ensuite ;
//! - il faut un emplacement libre pour y loger notre code, à portée de saut.
//!
//! Ce module ne modifie rien : il produit un plan vérifiable. L'écriture
//! viendra ensuite, une fois le plan validé.

use serde::Serialize;

use crate::error::{Result, TuxError};
use crate::memory::{self, Module};

/// Taille d'un `jmp rel32`.
pub const JUMP_SIZE: usize = 5;

/// Une instruction décodée au point de détour.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedInstruction {
    pub address: u64,
    pub length: usize,
    pub text: String,
    /// Une instruction dont le codage dépend de sa position ne peut pas être
    /// déplacée telle quelle dans la trampoline.
    pub position_dependent: bool,
}

/// Ce qu'il faudrait écrire pour poser le détour.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookPlan {
    pub target: u64,
    /// Instructions remplacées, à rejouer dans la trampoline.
    pub stolen: Vec<DecodedInstruction>,
    /// Nombre d'octets écrasés : saut + remplissage.
    pub stolen_bytes: usize,
    /// Adresse de retour, juste après les instructions volées.
    pub resume: u64,
    /// Emplacement exécutable retenu pour la trampoline.
    pub cave: Option<Cave>,
    /// Emplacement inscriptible où le code injecté rangera ses captures.
    pub storage: Option<Cave>,
    /// Raisons pour lesquelles le détour est risqué ou impossible.
    pub blockers: Vec<String>,
}

impl HookPlan {
    pub fn is_feasible(&self) -> bool {
        self.blockers.is_empty() && self.cave.is_some()
    }
}

/// Une plage d'octets de remplissage, utilisable pour y loger du code.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cave {
    pub address: u64,
    pub size: usize,
    pub writable: bool,
    pub executable: bool,
    /// Octet de remplissage rencontré (0xCC ou 0x00).
    pub filler: u8,
}

/// Décode les instructions à `address` jusqu'à couvrir au moins `JUMP_SIZE`.
pub fn decode_stolen(pid: u32, address: u64) -> Result<Vec<DecodedInstruction>> {
    // 16 octets suffisent pour la plus longue instruction x86, 32 pour en
    // couvrir plusieurs sans relire.
    let mut buffer = [0u8; 32];
    memory::read(pid, address, &mut buffer)?;

    let mut decoder =
        iced_x86::Decoder::with_ip(64, &buffer, address, iced_x86::DecoderOptions::NONE);
    let mut formatter = iced_x86::GasFormatter::new();
    let mut instructions = Vec::new();
    let mut covered = 0usize;

    while covered < JUMP_SIZE && decoder.can_decode() {
        let instruction = decoder.decode();
        if instruction.is_invalid() {
            return Err(TuxError::HookImpossible {
                address,
                reason: "instruction indécodable au point de détour".into(),
            });
        }

        let mut text = String::new();
        iced_x86::Formatter::format(&mut formatter, &instruction, &mut text);

        // Sauts, appels et adressages relatifs à RIP changent de sens s'ils
        // sont recopiés ailleurs : ils demandent une réécriture, pas une copie.
        let position_dependent = instruction.is_ip_rel_memory_operand()
            || matches!(
                instruction.flow_control(),
                iced_x86::FlowControl::UnconditionalBranch
                    | iced_x86::FlowControl::ConditionalBranch
                    | iced_x86::FlowControl::Call
                    | iced_x86::FlowControl::IndirectBranch
                    | iced_x86::FlowControl::IndirectCall
                    | iced_x86::FlowControl::Return
            );

        covered += instruction.len();
        instructions.push(DecodedInstruction {
            address: instruction.ip(),
            length: instruction.len(),
            text,
            position_dependent,
        });
    }

    if covered < JUMP_SIZE {
        return Err(TuxError::HookImpossible {
            address,
            reason: format!("{covered} octets décodés, {JUMP_SIZE} nécessaires"),
        });
    }
    Ok(instructions)
}

/// Cherche une suite d'octets de remplissage assez longue dans une région.
///
/// Les compilateurs alignent les fonctions en insérant de l'`int3` (0xCC) ou
/// des zéros : ces zones ne sont jamais exécutées et peuvent accueillir du code.
pub fn find_cave(
    pid: u32,
    module: &Module,
    size: usize,
    require_executable: bool,
    require_writable: bool,
) -> Result<Option<Cave>> {
    let regions = memory::regions(pid)?;
    let mut buffer = vec![0u8; 1024 * 1024];

    for region in regions.iter().filter(|region| {
        region.readable
            && region.start >= module.base
            && region.end <= module.base + module.size
            && (!require_writable || region.writable)
            && (!require_executable || region.executable)
    }) {
        let mut cursor = region.start;
        while cursor < region.end {
            let length = ((region.end - cursor) as usize).min(buffer.len());
            let slice = &mut buffer[..length];
            if memory::read(pid, cursor, slice).is_err() {
                break;
            }

            if let Some((offset, filler)) = longest_filler_run(slice, size) {
                return Ok(Some(Cave {
                    address: cursor + offset as u64,
                    size,
                    writable: region.writable,
                    executable: region.executable,
                    filler,
                }));
            }
            cursor += length as u64;
        }
    }
    Ok(None)
}

/// Premier emplacement d'au moins `size` octets de remplissage identique.
fn longest_filler_run(haystack: &[u8], size: usize) -> Option<(usize, u8)> {
    if size == 0 {
        return None;
    }
    let mut run_start = 0usize;
    let mut run_byte = None::<u8>;

    for (index, byte) in haystack.iter().enumerate() {
        let is_filler = *byte == 0xCC || *byte == 0x00;
        match (is_filler, run_byte) {
            (true, Some(current)) if current == *byte => {
                if index + 1 - run_start >= size {
                    return Some((run_start, current));
                }
            }
            (true, _) => {
                run_start = index;
                run_byte = Some(*byte);
                if size == 1 {
                    return Some((run_start, *byte));
                }
            }
            (false, _) => run_byte = None,
        }
    }
    None
}

/// Construit le plan de détour d'une adresse, sans rien modifier.
pub fn plan(pid: u32, module: &Module, target: u64) -> Result<HookPlan> {
    let stolen = decode_stolen(pid, target)?;
    let stolen_bytes: usize = stolen.iter().map(|instruction| instruction.length).sum();

    let mut blockers = Vec::new();
    for instruction in &stolen {
        if instruction.position_dependent {
            blockers.push(format!(
                "l'instruction « {} » à {:#x} dépend de sa position et devra être réécrite",
                instruction.text.trim(),
                instruction.address
            ));
        }
    }

    // La trampoline doit tenir : instructions volées + saut de retour, plus une
    // marge pour le code propre au script.
    let needed = stolen_bytes + JUMP_SIZE + 32;
    let cave = find_cave(pid, module, needed, true, false)?;
    // Le code injecté s'exécute dans le jeu : pour y ranger une capture, la page
    // doit être réellement inscriptible — contourner les protections depuis
    // l'extérieur ne suffit pas, c'est le jeu lui-même qui écrira.
    let storage = find_cave(pid, module, 16, false, true)?;

    match &cave {
        Some(cave) => {
            let distance = (cave.address as i64 - target as i64).abs();
            if distance > i32::MAX as i64 {
                blockers.push(format!(
                    "trampoline trop éloignée ({distance:#x}) pour un saut relatif"
                ));
            }
        }
        None => blockers.push(format!(
            "aucun emplacement exécutable de {needed} octets trouvé"
        )),
    }

    if storage.is_none() {
        blockers.push("aucun emplacement inscriptible pour les captures".into());
    }

    Ok(HookPlan {
        target,
        stolen_bytes,
        resume: target + stolen_bytes as u64,
        stolen,
        cave,
        storage,
        blockers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_run_of_padding() {
        let mut haystack = vec![0x90u8; 64];
        haystack[20..40].fill(0xCC);
        let (offset, filler) = longest_filler_run(&haystack, 16).expect("zone trouvée");
        assert_eq!(offset, 20);
        assert_eq!(filler, 0xCC);
    }

    #[test]
    fn ignores_runs_that_are_too_short() {
        let mut haystack = vec![0x90u8; 64];
        haystack[10..18].fill(0xCC);
        assert!(longest_filler_run(&haystack, 32).is_none());
    }

    #[test]
    fn does_not_mix_two_kinds_of_filler() {
        // 8 octets nuls suivis de 8 int3 ne font pas une zone de 16.
        let mut haystack = vec![0x90u8; 64];
        haystack[10..18].fill(0x00);
        haystack[18..26].fill(0xCC);
        assert!(longest_filler_run(&haystack, 16).is_none());
    }

    #[test]
    fn decodes_and_flags_position_dependent_instructions() {
        // mov rcx,[rcx+140] puis test rcx,rcx puis je +18
        let code = [
            0x48, 0x8B, 0x89, 0x40, 0x01, 0x00, 0x00, 0x48, 0x85, 0xC9, 0x74, 0x18,
        ];
        let mut decoder =
            iced_x86::Decoder::with_ip(64, &code, 0x140000000, iced_x86::DecoderOptions::NONE);
        let first = decoder.decode();
        assert_eq!(first.len(), 7, "mov rcx,[rcx+140] fait 7 octets");
        assert_eq!(
            first.flow_control(),
            iced_x86::FlowControl::Next,
            "un mov est déplaçable"
        );
        let second = decoder.decode();
        assert_eq!(second.len(), 3);
        let third = decoder.decode();
        assert_eq!(
            third.flow_control(),
            iced_x86::FlowControl::ConditionalBranch,
            "un saut conditionnel doit être signalé"
        );
    }
}
