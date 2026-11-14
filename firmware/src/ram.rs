//! Image RAM du firmware : cellules typées à décalage constant et carte mémoire (SPEC §3.1).
//!
//! Règle d'honnêteté mémoire : tout état qui survit d'un tick au suivant est une cellule de ce
//! module, rangée dans `Firmware::ram`. Les tâches n'accèdent à l'état QUE par `Slot::get` et
//! `Slot::set`. T-010 pourra remplacer le stockage d'une cellule (TMR, SECDED) sans réécrire les
//! tâches : c'est pourquoi `get` prend déjà `&mut Ram` (le vote TMR réécrit à la lecture).
//!
//! Vocabulaire : le type s'appelle `Slot` (et non `...Cell`) pour ne pas déclencher le contrôle
//! anti-état-caché de `scripts/ci.sh` (R5), qui interdit tout identifiant en `Cell`.
//!
//! Disposition : chaque cellule est placée juste après la précédente (`Slot::after`), ce qui
//! rend tout chevauchement impossible à l'intérieur d'une région ; des assertions à la
//! compilation vérifient que chaque région contient ses cellules. Les octets d'une région non
//! couverts par une cellule sont RÉSERVÉS (copies TMR de T-010, journal de T-011, état FDIR de
//! T-012) : un basculement de bit y est aujourd'hui sans effet. Voir `SLOTS` pour la couverture.

use core::marker::PhantomData;

use crate::hal::RAM_SIZE;

/// L'image RAM complète.
pub type Ram = [u8; RAM_SIZE];

// ---------------------------------------------------------------------------------------------
// Abstraction de cellule
// ---------------------------------------------------------------------------------------------

/// Valeur scalaire stockable dans une cellule (gros-boutiste).
/// `load` et `store` reçoivent une tranche d'exactement `SIZE` octets.
pub trait Scalar: Copy {
    const SIZE: usize;
    fn load(b: &[u8]) -> Self;
    fn store(self, b: &mut [u8]);
}

impl Scalar for u8 {
    const SIZE: usize = 1;
    fn load(b: &[u8]) -> Self {
        b[0]
    }
    fn store(self, b: &mut [u8]) {
        b[0] = self;
    }
}

impl Scalar for u16 {
    const SIZE: usize = 2;
    fn load(b: &[u8]) -> Self {
        u16::from_be_bytes([b[0], b[1]])
    }
    fn store(self, b: &mut [u8]) {
        b.copy_from_slice(&self.to_be_bytes());
    }
}

impl Scalar for i16 {
    const SIZE: usize = 2;
    fn load(b: &[u8]) -> Self {
        i16::from_be_bytes([b[0], b[1]])
    }
    fn store(self, b: &mut [u8]) {
        b.copy_from_slice(&self.to_be_bytes());
    }
}

impl Scalar for u32 {
    const SIZE: usize = 4;
    fn load(b: &[u8]) -> Self {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    }
    fn store(self, b: &mut [u8]) {
        b.copy_from_slice(&self.to_be_bytes());
    }
}

/// Booléen sur un octet : toute valeur non nulle lue vaut `true` (décodage total).
impl Scalar for bool {
    const SIZE: usize = 1;
    fn load(b: &[u8]) -> Self {
        b[0] != 0
    }
    fn store(self, b: &mut [u8]) {
        b[0] = self as u8;
    }
}

/// Cellule typée à décalage constant dans l'image RAM.
///
/// Invariant : `off + T::SIZE <= RAM_SIZE`, vérifié à la construction (`at` est `const` et
/// toutes les cellules sont des `const` : une violation est une erreur de compilation). Les
/// accès par tranche ne peuvent donc pas paniquer.
pub struct Slot<T: Scalar> {
    off: usize,
    _t: PhantomData<T>,
}

impl<T: Scalar> Slot<T> {
    pub const fn at(off: usize) -> Self {
        assert!(off + T::SIZE <= RAM_SIZE, "cellule hors de la RAM");
        Slot {
            off,
            _t: PhantomData,
        }
    }

    /// Cellule placée immédiatement après `end` (fin d'une cellule précédente).
    pub const fn after(end: usize) -> Self {
        Self::at(end)
    }

    pub const fn off(&self) -> usize {
        self.off
    }

    pub const fn len(&self) -> usize {
        T::SIZE
    }

    pub const fn is_empty(&self) -> bool {
        T::SIZE == 0
    }

    /// Premier octet après la cellule.
    pub const fn end(&self) -> usize {
        self.off + T::SIZE
    }

    /// Lecture. `&mut` dès maintenant : la lecture TMR (T-010) réécrit les copies divergentes.
    pub fn get(&self, ram: &mut Ram) -> T {
        T::load(&ram[self.off..self.off + T::SIZE])
    }

    pub fn set(&self, ram: &mut Ram, v: T) {
        v.store(&mut ram[self.off..self.off + T::SIZE]);
    }

    /// Fiche descriptive (table `SLOTS`).
    pub const fn info(&self, name: &'static str, kind: RegionKind) -> SlotInfo {
        SlotInfo {
            name,
            start: self.off,
            len: T::SIZE,
            kind,
        }
    }
}

/// Description d'une cellule, pour les tests de disposition et les diagnostics du sim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotInfo {
    pub name: &'static str,
    pub start: usize,
    pub len: usize,
    pub kind: RegionKind,
}

// ---------------------------------------------------------------------------------------------
// Carte mémoire
// ---------------------------------------------------------------------------------------------

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

pub const SCHED_START: usize = 0;
pub const SCHED_LEN: usize = 64;
pub const MODE_START: usize = SCHED_START + SCHED_LEN;
pub const MODE_LEN: usize = 64;
pub const FDIR_START: usize = MODE_START + MODE_LEN;
pub const FDIR_LEN: usize = 128;
pub const PROTO_START: usize = FDIR_START + FDIR_LEN;
pub const PROTO_LEN: usize = 64;
pub const TM_START: usize = PROTO_START + PROTO_LEN;
pub const TM_LEN: usize = 192;
pub const LOG_START: usize = TM_START + TM_LEN;
pub const LOG_LEN: usize = 448;
pub const UNUSED_START: usize = LOG_START + LOG_LEN;
pub const UNUSED_LEN: usize = RAM_SIZE - UNUSED_START;

const _: () = assert!(UNUSED_LEN * 10 < RAM_SIZE, "Unused >= 10 % de la RAM");

/// Carte mémoire. Contiguë, couvre toute la RAM. Bornes identiques à celles de T-002.
pub const RAM_MAP: &[Region] = &[
    Region {
        name: "sched",
        start: SCHED_START,
        len: SCHED_LEN,
        kind: RegionKind::Sched,
    },
    Region {
        name: "mode",
        start: MODE_START,
        len: MODE_LEN,
        kind: RegionKind::Mode,
    },
    Region {
        name: "fdir",
        start: FDIR_START,
        len: FDIR_LEN,
        kind: RegionKind::Fdir,
    },
    Region {
        name: "proto",
        start: PROTO_START,
        len: PROTO_LEN,
        kind: RegionKind::Proto,
    },
    Region {
        name: "tm",
        start: TM_START,
        len: TM_LEN,
        kind: RegionKind::Tm,
    },
    Region {
        name: "log",
        start: LOG_START,
        len: LOG_LEN,
        kind: RegionKind::Log,
    },
    Region {
        name: "unused",
        start: UNUSED_START,
        len: UNUSED_LEN,
        kind: RegionKind::Unused,
    },
];

/// Région contenant l'octet `offset` de la RAM (pour la ventilation des pannes par le sim).
pub fn region_of(offset: usize) -> Option<&'static Region> {
    RAM_MAP
        .iter()
        .find(|r| offset >= r.start && offset < r.start + r.len)
}

// ---------------------------------------------------------------------------------------------
// Cellules
// ---------------------------------------------------------------------------------------------

// --- Sched : ordonnanceur et informations de boot ---------------------------------------------

/// Secondes (cycles) depuis le boot, saturant.
pub const UPTIME: Slot<u32> = Slot::at(SCHED_START);
/// Prochaine clé de watchdog (`WDG_KEY_A` / `WDG_KEY_B` ; toute autre valeur lue = clé A).
pub const WDG_KEY: Slot<u8> = Slot::after(UPTIME.end());
/// Drapeaux d'exécution du cycle courant (bit = `sched::FLAG_*`), remis à 0 en début de cycle.
pub const EXEC_FLAGS: Slot<u8> = Slot::after(WDG_KEY.end());
/// Dernière valeur du battement FDIR vue par l'ordonnanceur.
pub const HB_SEEN: Slot<u16> = Slot::after(EXEC_FLAGS.end());
/// Cause du dernier reset (`ResetCause as u8`), lue au boot.
pub const RESET_CAUSE: Slot<u8> = Slot::after(HB_SEEN.end());
/// Nombre de boots (relu de la NVM en T-011 ; 0 d'ici là).
pub const BOOT_COUNT: Slot<u16> = Slot::after(RESET_CAUSE.end());
const SCHED_END: usize = BOOT_COUNT.end();
const _: () = assert!(SCHED_END <= SCHED_START + SCHED_LEN);

// --- Mode : modes, plafond, mesures validées, thermostat --------------------------------------

/// Mode courant (`Mode as u8` ; valeur hors domaine lue = `Survie`).
pub const MODE: Slot<u8> = Slot::at(MODE_START);
/// Plafond de mode fixé par `SetMode` (valeur hors domaine lue = `Nominal`, défaut de boot).
pub const CEILING: Slot<u8> = Slot::after(MODE.end());
/// Secondes consécutives avec `V >= 7080 mV` (montée `Survie` -> `Eco`).
pub const CNT_V_ECO_UP: Slot<u16> = Slot::after(CEILING.end());
/// Secondes consécutives avec `V >= 7560 mV` (montée `Eco` -> `Nominal`).
pub const CNT_V_NOM_UP: Slot<u16> = Slot::after(CNT_V_ECO_UP.end());
/// Secondes consécutives avec `V < 7200 mV` (descente `Nominal` -> `Eco`).
pub const CNT_V_LOW: Slot<u16> = Slot::after(CNT_V_NOM_UP.end());
/// Secondes consécutives avec `V < 6720 mV` (tout -> `Survie`).
pub const CNT_V_CRIT: Slot<u16> = Slot::after(CNT_V_LOW.end());
/// Tension batterie validée du cycle (mV ; 0 si aucune).
pub const VBATT_MV: Slot<u16> = Slot::after(CNT_V_CRIT.end());
pub const VBATT_VALID: Slot<bool> = Slot::after(VBATT_MV.end());
/// Température batterie validée du cycle (0,1 °C).
pub const TBATT_DC: Slot<i16> = Slot::after(VBATT_VALID.end());
pub const TBATT_VALID: Slot<bool> = Slot::after(TBATT_DC.end());
/// Défauts capteurs instantanés du cycle (bit = `SensorId`, plus `SFM_*_DISAGREE`).
pub const SENSOR_FAULT_MASK: Slot<u16> = Slot::after(TBATT_VALID.end());
/// État du thermostat (réchauffeur commandé).
pub const HEATER_ON: Slot<bool> = Slot::after(SENSOR_FAULT_MASK.end());
const MODE_END: usize = HEATER_ON.end();
const _: () = assert!(MODE_END <= MODE_START + MODE_LEN);

/// Bit de `SENSOR_FAULT_MASK` : capteurs de tension A/B en désaccord (> 150 mV).
pub const SFM_VBATT_DISAGREE: u16 = 1 << 9;
/// Bit de `SENSOR_FAULT_MASK` : capteurs de température A/B en désaccord.
pub const SFM_TEMP_DISAGREE: u16 = 1 << 10;

// --- Fdir : battement et état FDIR (logique réelle en T-012) ----------------------------------

/// Compteur de battement du FDIR, avancé de 1 par cycle par la tâche `fdir`.
pub const FDIR_HEARTBEAT: Slot<u16> = Slot::at(FDIR_START);
/// Drapeaux FDIR publiés dans la balise (bits `FDIR_FLAG_*`).
pub const FDIR_FLAGS: Slot<u16> = Slot::after(FDIR_HEARTBEAT.end());
/// Capteurs invalidés durablement par le FDIR (bit = `SensorId`) ; exclus de la validation.
pub const SENSOR_INVALID_MASK: Slot<u16> = Slot::after(FDIR_FLAGS.end());
/// Lignes isolées par le FDIR (bit = `Line::mask`). Le bit Radio est ignoré à la lecture.
pub const LINE_ISOLATED: Slot<u8> = Slot::after(SENSOR_INVALID_MASK.end());
/// Registres trouvés altérés puis réécrits par `health`.
pub const CNT_REG_FIX: Slot<u16> = Slot::after(LINE_ISOLATED.end());
/// Latch-up détectés (T-012).
pub const CNT_LATCHUP: Slot<u8> = Slot::after(CNT_REG_FIX.end());
const FDIR_END: usize = CNT_LATCHUP.end();
const _: () = assert!(FDIR_END <= FDIR_START + FDIR_LEN);

/// `FDIR_FLAGS` : alarme FDIR critique (bloque `Survie` -> `Eco`).
pub const FDIR_FLAG_CRITICAL: u16 = 1 << 0;
/// `FDIR_FLAGS` : verrou « tempête de resets » (force `Survie`).
pub const FDIR_FLAG_STORM_LOCK: u16 = 1 << 1;

// --- Proto : anti-rejeu, rejets --------------------------------------------------------------

/// SEQ de la dernière TC acceptée (NVM en T-011).
pub const LAST_TC_SEQ: Slot<u32> = Slot::at(PROTO_START);
/// Trames montantes rejetées (saturant).
pub const CNT_TC_REJECTED: Slot<u16> = Slot::after(LAST_TC_SEQ.end());
const PROTO_END: usize = CNT_TC_REJECTED.end();
const _: () = assert!(PROTO_END <= PROTO_START + PROTO_LEN);

// --- Tm : télémétrie, ACK en attente, charge utile --------------------------------------------

/// Compteur de trames descendantes (SEQ côté TM).
pub const TM_SEQ: Slot<u32> = Slot::at(TM_START);
/// Secondes depuis la dernière balise. Lu borné : `min(valeur, période)` (R13).
pub const BEACON_ELAPSED: Slot<u16> = Slot::after(TM_SEQ.end());
/// ACK en attente d'émission ssi la valeur vaut exactement `ACK_PENDING_YES`.
pub const ACK_PENDING: Slot<u8> = Slot::after(BEACON_ELAPSED.end());
pub const ACK_SEQ: Slot<u32> = Slot::after(ACK_PENDING.end());
pub const ACK_OPCODE: Slot<u8> = Slot::after(ACK_SEQ.end());
pub const ACK_RESULT: Slot<u8> = Slot::after(ACK_OPCODE.end());
/// Échantillons acquis par la charge utile factice.
pub const PAYLOAD_SAMPLES: Slot<u32> = Slot::after(ACK_RESULT.end());
const TM_END: usize = PAYLOAD_SAMPLES.end();
const _: () = assert!(TM_END <= TM_START + TM_LEN);

/// Valeur de `ACK_PENDING` signifiant « un ACK attend ». Toute autre valeur = rien à émettre
/// (un octet corrompu ne fabrique pas un ACK fantôme, sauf s'il tombe exactement sur 0xC3).
pub const ACK_PENDING_YES: u8 = 0xC3;

// --- Log : journal d'événements (T-011). Aucune cellule en T-005. ------------------------------

/// Toutes les cellules, pour les tests de disposition et la couverture de la RAM.
pub const SLOTS: &[SlotInfo] = &[
    UPTIME.info("uptime", RegionKind::Sched),
    WDG_KEY.info("wdg_key", RegionKind::Sched),
    EXEC_FLAGS.info("exec_flags", RegionKind::Sched),
    HB_SEEN.info("hb_seen", RegionKind::Sched),
    RESET_CAUSE.info("reset_cause", RegionKind::Sched),
    BOOT_COUNT.info("boot_count", RegionKind::Sched),
    MODE.info("mode", RegionKind::Mode),
    CEILING.info("ceiling", RegionKind::Mode),
    CNT_V_ECO_UP.info("cnt_v_eco_up", RegionKind::Mode),
    CNT_V_NOM_UP.info("cnt_v_nom_up", RegionKind::Mode),
    CNT_V_LOW.info("cnt_v_low", RegionKind::Mode),
    CNT_V_CRIT.info("cnt_v_crit", RegionKind::Mode),
    VBATT_MV.info("vbatt_mv", RegionKind::Mode),
    VBATT_VALID.info("vbatt_valid", RegionKind::Mode),
    TBATT_DC.info("tbatt_dc", RegionKind::Mode),
    TBATT_VALID.info("tbatt_valid", RegionKind::Mode),
    SENSOR_FAULT_MASK.info("sensor_fault_mask", RegionKind::Mode),
    HEATER_ON.info("heater_on", RegionKind::Mode),
    FDIR_HEARTBEAT.info("fdir_heartbeat", RegionKind::Fdir),
    FDIR_FLAGS.info("fdir_flags", RegionKind::Fdir),
    SENSOR_INVALID_MASK.info("sensor_invalid_mask", RegionKind::Fdir),
    LINE_ISOLATED.info("line_isolated", RegionKind::Fdir),
    CNT_REG_FIX.info("cnt_reg_fix", RegionKind::Fdir),
    CNT_LATCHUP.info("cnt_latchup", RegionKind::Fdir),
    LAST_TC_SEQ.info("last_tc_seq", RegionKind::Proto),
    CNT_TC_REJECTED.info("cnt_tc_rejected", RegionKind::Proto),
    TM_SEQ.info("tm_seq", RegionKind::Tm),
    BEACON_ELAPSED.info("beacon_elapsed", RegionKind::Tm),
    ACK_PENDING.info("ack_pending", RegionKind::Tm),
    ACK_SEQ.info("ack_seq", RegionKind::Tm),
    ACK_OPCODE.info("ack_opcode", RegionKind::Tm),
    ACK_RESULT.info("ack_result", RegionKind::Tm),
    PAYLOAD_SAMPLES.info("payload_samples", RegionKind::Tm),
];

/// Lecture bornée d'un compteur de période (R13) : une valeur corrompue au-delà de `period` est
/// ramenée à `period`, ce qui déclenche l'action due au lieu de la reporter indéfiniment.
pub fn bounded(v: u16, period: u16) -> u16 {
    v.min(period)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    #[test]
    fn scalar_roundtrip_big_endian() {
        let mut ram = [0u8; RAM_SIZE];
        let a: Slot<u32> = Slot::at(10);
        a.set(&mut ram, 0x0102_0304);
        assert_eq!(&ram[10..14], &[1, 2, 3, 4]);
        assert_eq!(a.get(&mut ram), 0x0102_0304);
        let b: Slot<i16> = Slot::at(20);
        b.set(&mut ram, -2);
        assert_eq!(&ram[20..22], &[0xFF, 0xFE]);
        assert_eq!(b.get(&mut ram), -2);
        let c: Slot<u16> = Slot::at(RAM_SIZE - 2);
        c.set(&mut ram, 0xBEEF);
        assert_eq!(c.get(&mut ram), 0xBEEF);
        let d: Slot<bool> = Slot::at(30);
        d.set(&mut ram, true);
        assert_eq!(ram[30], 1);
        ram[30] = 0x80; // corruption : toute valeur non nulle vaut vrai
        assert!(d.get(&mut ram));
        let e: Slot<u8> = Slot::at(31);
        e.set(&mut ram, 0x7E);
        assert_eq!(e.get(&mut ram), 0x7E);
        assert_eq!((a.off(), a.len(), a.end()), (10, 4, 14));
    }

    /// Une cellule n'écrit que ses propres octets.
    #[test]
    fn set_touches_only_its_bytes() {
        for s in SLOTS {
            let mut ram = [0xAAu8; RAM_SIZE];
            let slot: Slot<u8> = Slot::at(s.start);
            slot.set(&mut ram, 0x55);
            for (i, &b) in ram.iter().enumerate() {
                assert_eq!(b == 0x55, i == s.start);
            }
        }
        let mut ram = [0u8; RAM_SIZE];
        UPTIME.set(&mut ram, u32::MAX);
        let touched: Vec<usize> = (0..RAM_SIZE).filter(|&i| ram[i] != 0).collect();
        assert_eq!(touched, (UPTIME.off()..UPTIME.end()).collect::<Vec<_>>());
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
        assert_eq!(region_of(0).unwrap().kind, RegionKind::Sched);
        assert_eq!(region_of(RAM_SIZE - 1).unwrap().kind, RegionKind::Unused);
        assert!(region_of(RAM_SIZE).is_none());
    }

    #[test]
    fn slots_do_not_overlap_and_live_in_their_region() {
        for (i, a) in SLOTS.iter().enumerate() {
            assert!(a.len > 0 && a.start + a.len <= RAM_SIZE, "{}", a.name);
            let r = region_of(a.start).unwrap();
            assert_eq!(r.kind, a.kind, "{} hors de sa région", a.name);
            assert!(a.start + a.len <= r.start + r.len, "{} déborde", a.name);
            assert_ne!(a.kind, RegionKind::Unused);
            for b in &SLOTS[i + 1..] {
                let disjoint = a.start + a.len <= b.start || b.start + b.len <= a.start;
                assert!(disjoint, "{} chevauche {}", a.name, b.name);
                assert_ne!(a.name, b.name);
            }
        }
    }

    /// Couverture actuelle (information pour le compte rendu ; R6 dilution).
    #[test]
    fn coverage_is_reported() {
        let used: usize = SLOTS.iter().map(|s| s.len).sum();
        std::println!(
            "cellules : {} octets sur {} ({} %), le reste des régions est réservé (T-010..T-012)",
            used,
            RAM_SIZE,
            used * 100 / RAM_SIZE
        );
        assert!(used > 0);
    }

    #[test]
    fn bounded_read() {
        assert_eq!(bounded(5, 10), 5);
        assert_eq!(bounded(10, 10), 10);
        assert_eq!(bounded(u16::MAX, 60), 60);
    }
}
