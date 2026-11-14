//! Firmware Aegis (no_std, sans unsafe, sans allocation).
//!
//! Contrat avec le simulateur : SPEC §3.1. Tout l'état qui survit d'un tick au suivant vit dans
//! `Firmware::ram` (règle d'honnêteté mémoire) ; `Firmware` n'a aucun autre champ.
//!
//! Organisation : `ram` (cellules et carte mémoire), `sched` (exécutif cyclique, watchdog),
//! `mode` (machine de modes, lignes), `tasks` (une tâche par fichier), `proto`, `hal`.
//! État T-005 : TMR/ECC (T-010), NVM et TC complètes (T-011), FDIR (T-012) restent à faire.
#![no_std]
#![forbid(unsafe_code)]

pub mod hal;
pub mod mode;
pub mod proto;
pub mod ram;
pub mod sched;
pub mod tasks;
#[cfg(test)]
mod testhal;

pub use ram::{region_of, Region, RegionKind, RAM_MAP};

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

/// Clés alternées du watchdog matériel (SPEC §3). `0xA5` d'abord après chaque boot.
pub const WDG_KEY_A: u8 = 0xA5;
pub const WDG_KEY_B: u8 = 0x5A;
/// Délai du watchdog, en ticks (annexe A).
pub const WDG_TIMEOUT_S: u16 = 10;
/// Valeur de `Reg::WdgCtrl` réécrite à chaque cycle : bit 0 = watchdog activé, autres bits à 0.
pub const WDG_CTRL_VALUE: u16 = 0x0001;
/// Valeur de `Reg::RadioCfg` réécrite à chaque cycle : bit 0 = récepteur actif,
/// bit 1 = émetteur autorisé, autres bits à 0.
pub const RADIO_CFG_VALUE: u16 = 0x0003;

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

    /// Démarrage : efface la RAM, écrit les registres. Relecture NVM : T-011.
    pub fn boot<H: Hal>(&mut self, hal: &mut H) {
        sched::boot(&mut self.ram, hal);
    }

    /// Un cycle complet (= 1 tick).
    pub fn step<H: Hal>(&mut self, hal: &mut H) -> StepReport {
        sched::step(&mut self.ram, hal)
    }

    /// Port d'injection de pannes RAM, réservé au simulateur.
    pub fn ram_mut(&mut self) -> &mut [u8; RAM_SIZE] {
        &mut self.ram
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_is_exactly_ram() {
        assert_eq!(core::mem::size_of::<Firmware>(), RAM_SIZE);
    }

    #[test]
    fn mode_order_and_conversion() {
        assert!(Mode::Survie < Mode::Eco && Mode::Eco < Mode::Nominal);
        for m in [Mode::Survie, Mode::Eco, Mode::Nominal] {
            assert_eq!(Mode::from_u8(m as u8), Some(m));
        }
        assert_eq!(Mode::from_u8(3), None);
    }
}
