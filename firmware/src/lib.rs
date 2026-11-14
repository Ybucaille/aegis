//! Firmware Aegis (no_std, sans unsafe, sans allocation).
//!
//! Contrat avec le simulateur : SPEC §3.1. Tout l'état qui survit d'un tick au suivant vit dans
//! `Firmware::ram` (règle d'honnêteté mémoire) ; `Firmware` n'a aucun autre champ.
//!
//! État T-002 : `step` est un BOUCHON (reste en `Survie`, recharge le watchdog, n'émet rien).
//! L'ordonnanceur et les modes arrivent en T-005.
#![no_std]
#![forbid(unsafe_code)]

pub mod hal;
pub mod proto;

pub use hal::{Hal, HalError, Line, Reg, ResetCause, SensorId, MAX_FRAME, NVM_SIZE, RAM_SIZE};

/// Mode de fonctionnement. Ordonné par niveau de service : `Survie < Eco < Nominal`
/// (le plafond et les comparaisons « plafond >= Eco » s'appuient sur cet ordre).
/// La valeur `u8` est celle de la balise et de l'argument de la TC `SetMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Mode {
    Survie = 0,
    Eco = 1,
    Nominal = 2,
}

impl Mode {
    pub const fn from_u8(v: u8) -> Option<Mode> {
        match v {
            0 => Some(Mode::Survie),
            1 => Some(Mode::Eco),
            2 => Some(Mode::Nominal),
            _ => None,
        }
    }
}

/// Compte rendu d'un cycle. Instrument pour l'oracle (S5 via `executed_tc`) et le débogage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepReport {
    /// TC exécutée ce cycle : `(seq, opcode)`.
    pub executed_tc: Option<(u32, u8)>,
    pub mode: Mode,
    pub kicked: bool,
}

/// Nature d'une région de la RAM (ventilation des pannes RAM par région, SPEC §3.1 et R6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionKind {
    Sched,
    Mode,
    Fdir,
    Proto,
    Tm,
    Log,
    Unused,
}

/// Région de l'image RAM : octets `[start, start + len)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub name: &'static str,
    pub start: usize,
    pub len: usize,
    pub kind: RegionKind,
}

/// Carte mémoire PROVISOIRE (T-002) : découpage par région seulement. Les cellules seront
/// placées à l'intérieur en T-005 / T-010 ; les bornes pourront bouger (entrée `decisions.md`),
/// la forme du tableau non. Contiguë, couvre toute la RAM, `Unused` < 10 %.
pub const RAM_MAP: &[Region] = &[
    Region {
        name: "sched",
        start: 0,
        len: 64,
        kind: RegionKind::Sched,
    },
    Region {
        name: "mode",
        start: 64,
        len: 64,
        kind: RegionKind::Mode,
    },
    Region {
        name: "fdir",
        start: 128,
        len: 128,
        kind: RegionKind::Fdir,
    },
    Region {
        name: "proto",
        start: 256,
        len: 64,
        kind: RegionKind::Proto,
    },
    Region {
        name: "tm",
        start: 320,
        len: 192,
        kind: RegionKind::Tm,
    },
    Region {
        name: "log",
        start: 512,
        len: 448,
        kind: RegionKind::Log,
    },
    Region {
        name: "unused",
        start: 960,
        len: 64,
        kind: RegionKind::Unused,
    },
];

/// Région contenant l'octet `offset` de la RAM (pour la ventilation des pannes par le sim).
pub fn region_of(offset: usize) -> Option<&'static Region> {
    RAM_MAP
        .iter()
        .find(|r| offset >= r.start && offset < r.start + r.len)
}

// --- Cellules du bouchon (région Sched). Provisoires, remplacées en T-005. -------------------

/// Compteur de cycles depuis le boot (`u32` gros-boutiste).
const CELL_TICKS: usize = 0;
/// Prochaine clé de watchdog (`0xA5` ou `0x5A`).
const CELL_WDG_KEY: usize = 4;

/// Clés alternées du watchdog matériel (SPEC §3).
pub const WDG_KEY_A: u8 = 0xA5;
pub const WDG_KEY_B: u8 = 0x5A;
/// Délai du watchdog, en ticks (annexe A).
pub const WDG_TIMEOUT_S: u16 = 10;

/// Le firmware : une image RAM et rien d'autre.
pub struct Firmware {
    ram: [u8; RAM_SIZE],
}

impl Default for Firmware {
    fn default() -> Self {
        Self::new()
    }
}

impl Firmware {
    pub const fn new() -> Self {
        Firmware { ram: [0; RAM_SIZE] }
    }

    /// Démarrage : efface la RAM, relit la NVM.
    /// Bouchon T-002 : la relecture NVM (3 copies + CRC) arrive en T-011.
    pub fn boot<H: Hal>(&mut self, _hal: &mut H) {
        self.ram = [0; RAM_SIZE];
        self.ram[CELL_WDG_KEY] = WDG_KEY_A;
    }

    /// Un cycle complet (= 1 tick). Bouchon T-002 : reste en `Survie`, recharge le watchdog,
    /// n'émet rien.
    pub fn step<H: Hal>(&mut self, hal: &mut H) -> StepReport {
        let ticks = self.read_u32(CELL_TICKS).wrapping_add(1);
        self.write_u32(CELL_TICKS, ticks);

        hal.reg_write(Reg::WdgTimeout, WDG_TIMEOUT_S);

        // Valeur corrompue -> on repart sur la clé A (le pire cas est un rechargement manqué).
        let key = if self.ram[CELL_WDG_KEY] == WDG_KEY_B {
            WDG_KEY_B
        } else {
            WDG_KEY_A
        };
        hal.wdg_kick(key);
        self.ram[CELL_WDG_KEY] = if key == WDG_KEY_A {
            WDG_KEY_B
        } else {
            WDG_KEY_A
        };

        StepReport {
            executed_tc: None,
            mode: Mode::Survie,
            kicked: true,
        }
    }

    /// Port d'injection de pannes RAM, réservé au simulateur.
    pub fn ram_mut(&mut self) -> &mut [u8; RAM_SIZE] {
        &mut self.ram
    }

    fn read_u32(&self, off: usize) -> u32 {
        u32::from_be_bytes([
            self.ram[off],
            self.ram[off + 1],
            self.ram[off + 2],
            self.ram[off + 3],
        ])
    }

    fn write_u32(&mut self, off: usize, v: u32) {
        self.ram[off..off + 4].copy_from_slice(&v.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// HAL minimale enregistrant les appels.
    #[derive(Default)]
    struct MockHal {
        kicks: Vec<u8>,
        sent: usize,
        regs: [u16; 4],
        resets: usize,
    }

    impl Hal for MockHal {
        fn sensor(&mut self, _id: SensorId) -> Result<i16, HalError> {
            Ok(0)
        }
        fn reg_read(&mut self, r: Reg) -> u16 {
            self.regs[r as usize]
        }
        fn reg_write(&mut self, r: Reg, v: u16) {
            self.regs[r as usize] = v;
        }
        fn wdg_kick(&mut self, key: u8) {
            self.kicks.push(key);
        }
        fn radio_recv(&mut self, _buf: &mut [u8; MAX_FRAME]) -> Option<usize> {
            None
        }
        fn radio_send(&mut self, _frame: &[u8]) -> Result<(), HalError> {
            self.sent += 1;
            Ok(())
        }
        fn nvm_read(&mut self, _off: u16, buf: &mut [u8]) -> Result<(), HalError> {
            buf.fill(0xFF);
            Ok(())
        }
        fn nvm_write(&mut self, _off: u16, _data: &[u8]) -> Result<(), HalError> {
            Ok(())
        }
        fn reset_cause(&mut self) -> ResetCause {
            ResetCause::PowerOn
        }
        fn sys_reset(&mut self) {
            self.resets += 1;
        }
    }

    #[test]
    fn firmware_is_exactly_ram() {
        assert_eq!(core::mem::size_of::<Firmware>(), RAM_SIZE);
    }

    #[test]
    fn ram_map_is_contiguous_and_complete() {
        let mut next = 0;
        for r in RAM_MAP {
            assert_eq!(r.start, next, "trou ou chevauchement avant {}", r.name);
            assert!(r.len > 0);
            next = r.start + r.len;
        }
        assert_eq!(next, RAM_SIZE);
        let unused: usize = RAM_MAP
            .iter()
            .filter(|r| r.kind == RegionKind::Unused)
            .map(|r| r.len)
            .sum();
        assert!(unused * 10 < RAM_SIZE, "Unused >= 10 % de la RAM");
        for kind in [
            RegionKind::Sched,
            RegionKind::Mode,
            RegionKind::Fdir,
            RegionKind::Proto,
            RegionKind::Tm,
            RegionKind::Log,
        ] {
            assert!(RAM_MAP.iter().any(|r| r.kind == kind), "{kind:?} absente");
        }
        assert_eq!(region_of(0).unwrap().kind, RegionKind::Sched);
        assert_eq!(region_of(RAM_SIZE - 1).unwrap().kind, RegionKind::Unused);
        assert!(region_of(RAM_SIZE).is_none());
    }

    #[test]
    fn mode_order_and_conversion() {
        assert!(Mode::Survie < Mode::Eco && Mode::Eco < Mode::Nominal);
        for m in [Mode::Survie, Mode::Eco, Mode::Nominal] {
            assert_eq!(Mode::from_u8(m as u8), Some(m));
        }
        assert_eq!(Mode::from_u8(3), None);
    }

    #[test]
    fn boot_clears_ram() {
        let mut fw = Firmware::new();
        fw.ram_mut().fill(0xEE);
        let mut hal = MockHal::default();
        fw.boot(&mut hal);
        assert!(fw.ram[..CELL_WDG_KEY].iter().all(|&b| b == 0));
        assert!(fw.ram[CELL_WDG_KEY + 1..].iter().all(|&b| b == 0));
    }

    #[test]
    fn stub_step_stays_in_survie_kicks_alternately_and_is_silent() {
        let mut fw = Firmware::new();
        let mut hal = MockHal::default();
        fw.boot(&mut hal);
        for _ in 0..100 {
            let r = fw.step(&mut hal);
            assert_eq!(r.mode, Mode::Survie);
            assert!(r.kicked);
            assert_eq!(r.executed_tc, None);
        }
        assert_eq!(hal.kicks.len(), 100);
        assert_eq!(hal.kicks[0], WDG_KEY_A);
        for w in hal.kicks.windows(2) {
            assert_ne!(w[0], w[1], "clé répétée");
        }
        assert_eq!(hal.sent, 0);
        assert_eq!(hal.resets, 0);
        assert_eq!(hal.regs[Reg::WdgTimeout as usize], WDG_TIMEOUT_S);
        assert_eq!(fw.read_u32(CELL_TICKS), 100);
    }

    /// Corruption volontaire de toute la RAM : le bouchon ne panique pas et continue de
    /// recharger le watchdog avec une clé valide.
    #[test]
    fn stub_step_survives_arbitrary_ram() {
        let mut hal = MockHal::default();
        for fill in [0x00u8, 0xFF, 0x5A, 0xA5, 0x3C] {
            let mut fw = Firmware::new();
            fw.ram_mut().fill(fill);
            for _ in 0..3 {
                fw.step(&mut hal);
            }
        }
        assert!(hal.kicks.iter().all(|&k| k == WDG_KEY_A || k == WDG_KEY_B));
    }
}
