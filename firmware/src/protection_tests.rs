//! Tests de masse des protections (T-010), compilés seulement avec protections.
//! Les `#[cfg(test)]` redondants sur les imports `std` servent au contrôle textuel de
//! `scripts/ci.sh`.
//!
//! - Tout bit de toute région TMR/SECDED de `RAM_MAP` (Sched, Mode, Fdir, Proto, Tm, Log)
//!   basculé une fois, puis 120 cycles `MockHal` : même mode, mêmes rechargements, mêmes TC
//!   exécutées et mêmes trames émises qu'un témoin sans basculement (les balises sont comparées
//!   champs `cnt_tmr_fix` / `cnt_ecc_corr` exceptés, qui comptent justement la réparation), et
//!   image finale identique au témoin (mêmes exceptions).
//! - Tout MBU de 2 à 4 bits adjacents (LSB d'abord et MSB d'abord) dans toute région protégée :
//!   toutes les cellules relues à leur valeur juste, image réparée, aucune alarme TMR.

#[cfg(test)]
extern crate std;

use crate::proto::{self, Beacon, Command, FrameType};
use crate::ram::{self, Ram, RegionKind};
use crate::tasks::scrub;
use crate::testhal::MockHal;
use crate::{Firmware, Mode, StepReport};
#[cfg(test)]
use std::vec::Vec;

const CYCLES: u32 = 120;

/// Scénario commun : Ping réguliers, abaissement puis relèvement du plafond, une trame
/// corrompue (rejet compté).
fn feed(hal: &mut MockHal, k: u32) {
    match k {
        5 | 25 | 45 | 85 | 105 => {
            hal.push_tc(100 + k, &Command::Ping);
        }
        30 => {
            hal.push_tc(100 + k, &Command::SetMode(Mode::Eco as u8));
        }
        60 => {
            hal.push_tc(100 + k, &Command::SetMode(Mode::Nominal as u8));
        }
        70 => hal.rx.push_back([0xAE, 0x61, 0x11, 0, 0].to_vec()),
        _ => {}
    }
}

struct Trace {
    reports: Vec<StepReport>,
    frames: Vec<Vec<u8>>,
    kicks: Vec<u8>,
    regs: [u16; 4],
    ram: Ram,
}

/// Trame normalisée : balise sans les compteurs de réparation ; autres trames telles quelles.
fn normalize(f: &[u8]) -> Vec<u8> {
    let fr = proto::decode_frame(f).expect("trame émise invalide");
    if fr.ftype != FrameType::Beacon {
        return f.to_vec();
    }
    let mut b = Beacon::decode(fr.payload).expect("balise invalide");
    b.cnt_tmr_fix = 0;
    b.cnt_ecc_corr = 0;
    let mut v = b.encode().to_vec();
    v.extend_from_slice(&fr.seq.to_be_bytes());
    v
}

fn run_from(ram0: &Ram, hal0: &MockHal, flip: Option<(usize, u8)>) -> Trace {
    let mut fw = Firmware::new();
    *fw.ram_mut() = *ram0;
    let mut hal = hal0.clone();
    let sent0 = hal.sent.len();
    let kicks0 = hal.kicks.len();
    if let Some((byte, mask)) = flip {
        fw.ram_mut()[byte] ^= mask;
    }
    let mut reports = Vec::new();
    for k in 0..CYCLES {
        feed(&mut hal, k);
        reports.push(fw.step(&mut hal));
    }
    let mut ram = *fw.ram_mut();
    ram::CNT_TMR_FIX.set(&mut ram, 0);
    ram::CNT_ECC_CORR.set(&mut ram, 0);
    Trace {
        reports,
        frames: hal.sent[sent0..].iter().map(|f| normalize(f)).collect(),
        kicks: hal.kicks[kicks0..].to_vec(),
        regs: hal.regs,
        ram,
    }
}

fn warm() -> (Ram, MockHal) {
    let mut fw = Firmware::new();
    let mut hal = MockHal::nominal();
    fw.boot(&mut hal);
    for _ in 0..200 {
        fw.step(&mut hal);
    }
    assert_eq!(crate::mode::current(fw.ram_mut()), Mode::Nominal);
    (*fw.ram_mut(), hal)
}

#[test]
fn every_bit_of_protected_regions_is_harmless() {
    let (ram0, hal0) = warm();
    let w = run_from(&ram0, &hal0, None);
    assert!(
        w.reports.iter().any(|r| r.mode == Mode::Eco),
        "scénario sans Eco"
    );
    assert!(w.reports.iter().filter(|r| r.executed_tc.is_some()).count() >= 7);
    assert!(scrub::image_clean(&w.ram));
    let mut flips = 0usize;
    for r in ram::RAM_MAP.iter().filter(|r| r.kind != RegionKind::Unused) {
        for bit in r.start * 8..(r.start + r.len) * 8 {
            let t = run_from(&ram0, &hal0, Some((bit / 8, 1 << (bit % 8))));
            let at = || std::format!("région {} octet {} bit {}", r.name, bit / 8, bit % 8);
            assert_eq!(t.reports, w.reports, "{}", at());
            assert_eq!(t.kicks, w.kicks, "{}", at());
            assert_eq!(t.frames, w.frames, "{}", at());
            assert_eq!(t.regs, w.regs, "{}", at());
            assert_eq!(t.ram[..], w.ram[..], "{}", at());
            flips += 1;
        }
    }
    std::println!(
        "{flips} basculements, {} cycles chacun : comportement identique au témoin",
        CYCLES
    );
    assert_eq!(flips, ram::UNUSED_START * 8);
}

#[test]
fn adjacent_mbu_in_protected_regions_reads_right_values() {
    let (ram0, _) = warm();
    let mut reference = ram0;
    let expected: Vec<Option<u32>> = ram::SLOTS
        .iter()
        .map(|s| s.read_raw(&mut reference))
        .collect();
    assert_eq!(reference, ram0, "image de départ non propre");
    let mut n = 0;
    for r in ram::RAM_MAP.iter().filter(|r| r.kind != RegionKind::Unused) {
        for msb_first in [false, true] {
            for len in 2..=4usize {
                for start in r.start * 8..(r.start + r.len) * 8 - len + 1 {
                    let mut ram = ram0;
                    for p in start..start + len {
                        let b = if msb_first { 7 - p % 8 } else { p % 8 };
                        ram[p / 8] ^= 1 << b;
                    }
                    for (s, e) in ram::SLOTS.iter().zip(&expected) {
                        // Mesures du cycle (réécrites avant lecture) et compteurs de réparation
                        // (qui comptent justement ce MBU) exceptés.
                        if s.prot == ram::Prot::Plain
                            || s.name == "cnt_tmr_fix"
                            || s.name == "cnt_ecc_corr"
                        {
                            continue;
                        }
                        assert_eq!(
                            s.read_raw(&mut ram),
                            *e,
                            "{} : MBU {len} bits en {start} (msb {msb_first})",
                            s.name
                        );
                    }
                    // Le reste de l'image (réservé, journal) est réparé par le scrub.
                    let mut hal = MockHal::nominal();
                    for _ in 0..scrub::SWEEP_S {
                        scrub::run(&mut ram, &mut hal);
                    }
                    assert!(scrub::image_clean(&ram), "MBU {len} en {start}");
                    assert_eq!(ram::TMR_ALARM.get(&mut ram), 0, "MBU {len} en {start}");
                    n += 1;
                }
            }
        }
    }
    std::println!("{n} MBU vérifiés");
}

#[test]
fn beacon_publishes_repair_counters() {
    let (ram0, hal0) = warm();
    let mut fw = Firmware::new();
    *fw.ram_mut() = ram0;
    let mut hal = hal0.clone();
    let ram = fw.ram_mut();
    ram[ram::MODE.off()] ^= 0x40; // une copie TMR
    let (b, l) = ram::TM_ECC.phys_bit(ram::TM_SEQ.off(), 0); // un bit SECDED
    ram[b] ^= 1 << l;
    let before = hal.sent.len();
    for _ in 0..25 {
        fw.step(&mut hal);
    }
    let beacons: Vec<Beacon> = hal.sent[before..]
        .iter()
        .filter_map(|f| {
            let fr = proto::decode_frame(f).ok()?;
            (fr.ftype == FrameType::Beacon).then(|| Beacon::decode(fr.payload).ok())?
        })
        .collect();
    let last = beacons.last().expect("aucune balise");
    assert_eq!(last.cnt_tmr_fix, 1);
    assert_eq!(last.cnt_ecc_corr, 1);
    assert_eq!(last.mode, Mode::Nominal as u8);
}
