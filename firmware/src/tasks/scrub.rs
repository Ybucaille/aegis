//! Tâche 7 `scrub` (non critique) : vote TMR et correction SECDED en tourniquet (SPEC §4.1).
//!
//! Deux curseurs (cellules TMR `SCRUB_TMR_CUR`, `SCRUB_ECC_CUR`, lus modulo leur nombre
//! d'unités : un curseur corrompu ne sort jamais des bornes) :
//! - TMR : une unité = un octet de bloc (ses trois copies) ; `TMR_PER_CYCLE` unités par cycle ;
//! - SECDED : une unité = un groupe entrelacé (8 voies) ; `ECC_PER_CYCLE` unités par cycle.
//!
//! L'image protégée complète est balayée en `SWEEP_S` = 30 cycles (exigence : <= 60 s).
//! Réparations comptées dans `CNT_TMR_FIX` / `CNT_ECC_CORR` ; absence de majorité signalée
//! dans `TMR_ALARM` (par `tmr`). Sans protection (feature `unprotected`), `tmr` et `ecc` rendent
//! le scrub inopérant ; rien ici ne dépend de la feature.

use crate::ecc;
use crate::hal::Hal;
use crate::ram::{self, Ram, ECC_AREAS, ECC_UNITS, TMR_AREAS, TMR_UNITS};
use crate::sched;
use crate::tmr;

/// Durée d'un balayage complet (cycles).
pub const SWEEP_S: usize = 30;
pub const TMR_PER_CYCLE: usize = TMR_UNITS.div_ceil(SWEEP_S);
pub const ECC_PER_CYCLE: usize = ECC_UNITS.div_ceil(SWEEP_S);
const _: () = assert!(TMR_PER_CYCLE * SWEEP_S >= TMR_UNITS && SWEEP_S <= 60);
const _: () = assert!(ECC_PER_CYCLE * SWEEP_S >= ECC_UNITS);

fn scrub_tmr_unit(ram: &mut Ram, mut u: usize) {
    for a in &TMR_AREAS {
        if u < a.block {
            tmr::scrub_byte(ram, a, u);
            return;
        }
        u -= a.block;
    }
}

fn scrub_ecc_unit(ram: &mut Ram, mut u: usize) {
    for a in &ECC_AREAS {
        if u < a.groups {
            ecc::scrub_group(ram, a, u);
            return;
        }
        u -= a.groups;
    }
}

pub fn run<H: Hal>(ram: &mut Ram, _hal: &mut H) {
    let mut c = ram::SCRUB_TMR_CUR.get(ram) as usize % TMR_UNITS;
    for _ in 0..TMR_PER_CYCLE {
        scrub_tmr_unit(ram, c);
        c = (c + 1) % TMR_UNITS;
    }
    ram::SCRUB_TMR_CUR.set(ram, c as u16);

    let mut c = ram::SCRUB_ECC_CUR.get(ram) as usize % ECC_UNITS;
    for _ in 0..ECC_PER_CYCLE {
        scrub_ecc_unit(ram, c);
        c = (c + 1) % ECC_UNITS;
    }
    ram::SCRUB_ECC_CUR.set(ram, c as u16);

    sched::set_flag(ram, sched::FLAG_SCRUB);
}

/// Vrai si toute l'image protégée est cohérente (copies TMR identiques, voies SECDED propres).
pub fn image_clean(ram: &Ram) -> bool {
    TMR_AREAS.iter().all(|a| tmr::area_consistent(ram, a))
        && ECC_AREAS
            .iter()
            .all(|a| (0..a.groups).all(|g| ecc::group_clean(ram, a, g)))
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::hal::RAM_SIZE;
    use crate::testhal::MockHal;

    #[test]
    fn sweep_parameters() {
        std::println!(
            "scrub : {TMR_UNITS} octets TMR ({TMR_PER_CYCLE}/cycle), {ECC_UNITS} groupes SECDED ({ECC_PER_CYCLE}/cycle), balayage {SWEEP_S} s"
        );
        assert!(TMR_UNITS.div_ceil(TMR_PER_CYCLE) <= 60);
        assert!(ECC_UNITS.div_ceil(ECC_PER_CYCLE) <= 60);
    }

    #[test]
    fn corrupted_cursors_stay_in_bounds() {
        let mut ram = [0u8; RAM_SIZE];
        let mut hal = MockHal::nominal();
        ram::SCRUB_TMR_CUR.set(&mut ram, u16::MAX);
        ram::SCRUB_ECC_CUR.set(&mut ram, u16::MAX);
        run(&mut ram, &mut hal);
        assert!((ram::SCRUB_TMR_CUR.get(&mut ram) as usize) < TMR_UNITS);
        assert!((ram::SCRUB_ECC_CUR.get(&mut ram) as usize) < ECC_UNITS);
    }

    /// Bit basculé jamais relu par ailleurs (seul le scrub tourne) : corrigé en <= 60 cycles,
    /// pour TOUT bit de toute zone TMR et SECDED, quel que soit le curseur de départ.
    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn every_protected_bit_is_scrubbed_within_60_cycles() {
        let mut hal = MockHal::nominal();
        let mut base = [0u8; RAM_SIZE];
        // Contenu non trivial : chaque cellule à une valeur distincte.
        for (i, s) in ram::SLOTS.iter().enumerate() {
            let v = 0x11u8.wrapping_mul(i as u8 + 1);
            match s.prot {
                ram::Prot::Tmr(t) => {
                    for k in 0..s.len {
                        tmr::write(&mut base, s.start + k, t.block, &[v ^ k as u8]);
                    }
                }
                ram::Prot::Ecc(e) => {
                    for k in 0..s.len {
                        ecc::write_byte(&mut base, &e, s.start + k, v ^ k as u8);
                    }
                }
                ram::Prot::Plain => {}
            }
        }
        let ranges: std::vec::Vec<(usize, usize)> = TMR_AREAS
            .iter()
            .map(|a| (a.base, a.end()))
            .chain(ECC_AREAS.iter().map(|a| (a.base, a.end())))
            .collect();
        let mut n = 0;
        for (lo, hi) in ranges {
            for bit in lo * 8..hi * 8 {
                let mut ram = base;
                // Curseurs de départ variés.
                ram::SCRUB_TMR_CUR.set(&mut ram, (bit % TMR_UNITS) as u16);
                ram::SCRUB_ECC_CUR.set(&mut ram, (bit % ECC_UNITS) as u16);
                let mut witness = ram;
                ram[bit / 8] ^= 1 << (bit % 8);
                let mut cycles = 0;
                while !image_clean(&ram) {
                    run(&mut ram, &mut hal);
                    run(&mut witness, &mut hal);
                    cycles += 1;
                    assert!(cycles <= 60, "bit {bit} non corrigé en 60 cycles");
                }
                // Valeur juste : identique au témoin sans basculement, compteurs exceptés.
                for c in [&ram::CNT_TMR_FIX, &ram::CNT_ECC_CORR] {
                    c.set(&mut ram, 0);
                    c.set(&mut witness, 0);
                }
                assert_eq!(ram[..], witness[..], "bit {bit}");
                n += 1;
            }
        }
        std::println!("{n} bits protégés vérifiés");
    }

    #[cfg(feature = "unprotected")]
    #[test]
    fn unprotected_scrub_corrects_nothing() {
        let mut hal = MockHal::nominal();
        let mut ram = [0u8; RAM_SIZE];
        ram[ram::FDIR_TMR.base] ^= 1;
        ram[ram::LOG_ECC.base] ^= 1;
        for _ in 0..60 {
            run(&mut ram, &mut hal);
        }
        assert!(!image_clean(&ram));
    }
}
