//! Bloc persistant en NVM : 3 copies avec CRC-32 (SPEC §4.3), relecture au boot.
//!
//! ## Format d'une copie (16 octets, gros-boutiste)
//!
//! | Octets | Champ | Relecture (bornage, R9) |
//! |---|---|---|
//! | 0 | `MAGIC` = `0xAE` | sinon copie invalide |
//! | 1 | `FORMAT` = 1 | sinon copie invalide |
//! | 2..4 | `boot_count u16` | tel quel ; incrément saturant |
//! | 4..8 | `last_tc_seq u32` | tel quel (la fenêtre anti-rejeu est saturante) |
//! | 8 | plafond (`Mode as u8`) | hors domaine -> `Nominal` (défaut) |
//! | 9 | lignes isolées (bit = `Line::mask`) | masqué par `ISOLATABLE` (Payload, Heater) |
//! | 10 | `short_boots u8` (tempête de resets, T-012) | borné à `SHORT_BOOTS_MAX` |
//! | 11 | réservé, écrit 0 | ignoré |
//! | 12..16 | CRC-32 IEEE (`proto::crc32`) des octets 0..12 | sinon copie invalide |
//!
//! Copies aux décalages `COPY_OFF` = 0x00, 0x50, 0xA0 (éloignées : une rafale de bits voisins
//! n'en touche qu'une). Le reste de la NVM (208 octets) n'est pas utilisé.
//!
//! ## Relecture (`boot_load`, appelée une fois par boot, après l'effacement de la RAM)
//! Première copie valide (ordre 0, 1, 2), champs bornés ; puis `boot_count + 1` et réécriture
//! des TROIS copies (ce qui répare les copies invalides). Aucune copie valide : valeurs par
//! défaut, bit `FDIR_FLAG_NVM_LOST` dans `FDIR_FLAGS` (publié par la balise), événement
//! `NVM_LOST`. Exception : NVM entièrement vierge (`0xFF` sur les trois copies, premier
//! démarrage) -> défauts SANS `NVM_LOST` (événement `NVM_FORMAT`) : un bloc formaté ne se
//! confond pas avec une NVM détruite par des basculements de bits.
//!
//! ## Écriture (`save`)
//! Les trois copies sont écrites puis relues et comparées. Un échec (`HalError` ou relecture
//! différente) laisse `NVM_PENDING` posé ; `service` (appelée à chaque cycle par `comms_rx`)
//! retente au cycle suivant. Aucun échec NVM ne panique ni ne bloque le cycle.
//!
//! Limite assumée (SPEC « première copie valide ») : si l'écriture de la copie 0 échoue en
//! laissant son ancien contenu intact et valide, un reset avant la reprise relit cette copie
//! périmée.

use crate::hal::{Hal, Line};
use crate::log::{self, ev};
use crate::mode;
use crate::proto::crc32;
use crate::ram::{self, Ram};
use crate::Mode;

/// Octets d'une copie.
pub const BLOCK_LEN: usize = 16;
/// Octets couverts par le CRC.
pub const DATA_LEN: usize = 12;
pub const MAGIC: u8 = 0xAE;
pub const FORMAT: u8 = 1;
/// Décalages des trois copies.
pub const COPY_OFF: [u16; 3] = [0x00, 0x50, 0xA0];
const _: () = assert!(COPY_OFF[2] as usize + BLOCK_LEN <= crate::hal::NVM_SIZE);
const _: () = assert!(COPY_OFF[1] as usize >= COPY_OFF[0] as usize + BLOCK_LEN);
const _: () = assert!(COPY_OFF[2] as usize >= COPY_OFF[1] as usize + BLOCK_LEN);
/// Lignes isolables (Radio : jamais isolée, SPEC §4.4).
pub const ISOLATABLE: u8 = (Line::Payload.mask() | Line::Heater.mask()) as u8;
/// Borne du compteur de boots courts relu (le seuil de tempête est 3).
pub const SHORT_BOOTS_MAX: u8 = 15;
/// Bit de `ram::NVM_STATUS` : bloc déjà relu pendant ce boot (`boot_load` idempotente).
pub const STATUS_LOADED: u8 = 1 << 7;
/// Masque des copies valides dans `ram::NVM_STATUS`.
pub const STATUS_VALID_MASK: u8 = 0b111;

/// Contenu du bloc persistant, après bornage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Persist {
    pub boot_count: u16,
    pub last_tc_seq: u32,
    pub ceiling: Mode,
    pub isolated: u8,
    pub short_boots: u8,
}

impl Persist {
    pub const DEFAULT: Persist = Persist {
        boot_count: 0,
        last_tc_seq: 0,
        ceiling: Mode::Nominal,
        isolated: 0,
        short_boots: 0,
    };

    /// Bornage de chaque champ (R9) : la valeur rendue est toujours dans son domaine.
    pub fn sanitized(self) -> Persist {
        Persist {
            isolated: self.isolated & ISOLATABLE,
            short_boots: self.short_boots.min(SHORT_BOOTS_MAX),
            ..self
        }
    }

    pub fn encode(&self) -> [u8; BLOCK_LEN] {
        let p = self.sanitized();
        let mut b = [0u8; BLOCK_LEN];
        b[0] = MAGIC;
        b[1] = FORMAT;
        b[2..4].copy_from_slice(&p.boot_count.to_be_bytes());
        b[4..8].copy_from_slice(&p.last_tc_seq.to_be_bytes());
        b[8] = p.ceiling as u8;
        b[9] = p.isolated;
        b[10] = p.short_boots;
        b[11] = 0;
        let crc = crc32(&b[..DATA_LEN]);
        b[DATA_LEN..].copy_from_slice(&crc.to_be_bytes());
        b
    }

    /// Copie valide (magic, format, CRC) -> contenu borné ; sinon `None`.
    pub fn decode(b: &[u8; BLOCK_LEN]) -> Option<Persist> {
        let crc = u32::from_be_bytes([b[12], b[13], b[14], b[15]]);
        if b[0] != MAGIC || b[1] != FORMAT || crc32(&b[..DATA_LEN]) != crc {
            return None;
        }
        let p = Persist {
            boot_count: u16::from_be_bytes([b[2], b[3]]),
            last_tc_seq: u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
            ceiling: Mode::from_u8(b[8]).unwrap_or(Mode::Nominal),
            isolated: b[9],
            short_boots: b[10],
        };
        Some(p.sanitized())
    }

    /// État persistant courant, lu dans les cellules RAM (bornées).
    pub fn from_cells(ram: &mut Ram) -> Persist {
        Persist {
            boot_count: ram::BOOT_COUNT.get(ram),
            last_tc_seq: ram::LAST_TC_SEQ.get(ram),
            ceiling: mode::ceiling(ram),
            isolated: ram::LINE_ISOLATED.get(ram),
            short_boots: ram::SHORT_BOOTS.get(ram),
        }
        .sanitized()
    }

    fn to_cells(self, ram: &mut Ram) {
        ram::BOOT_COUNT.set(ram, self.boot_count);
        ram::LAST_TC_SEQ.set(ram, self.last_tc_seq);
        ram::CEILING.set(ram, self.ceiling as u8);
        ram::LINE_ISOLATED.set(ram, self.isolated);
        ram::SHORT_BOOTS.set(ram, self.short_boots);
    }
}

/// Écrit et relit une copie. `true` si la copie relue est exactement `b`.
fn write_copy<H: Hal>(hal: &mut H, off: u16, b: &[u8; BLOCK_LEN]) -> bool {
    if hal.nvm_write(off, b).is_err() {
        return false;
    }
    let mut back = [0u8; BLOCK_LEN];
    hal.nvm_read(off, &mut back).is_ok() && back == *b
}

/// Écrit l'état persistant courant (cellules RAM) dans les trois copies. Renvoie `true` si les
/// trois sont écrites et vérifiées ; sinon `NVM_PENDING` reste posé (nouvel essai par
/// `service`), `CNT_NVM_WERR += 1`, et un événement est journalisé au début d'une série
/// d'échecs.
pub fn save<H: Hal>(ram: &mut Ram, hal: &mut H) -> bool {
    let b = Persist::from_cells(ram).encode();
    let mut failed = 0u8;
    for (c, &off) in COPY_OFF.iter().enumerate() {
        if !write_copy(hal, off, &b) {
            failed |= 1 << c;
        }
    }
    if failed == 0 {
        ram::NVM_PENDING.set(ram, 0);
        return true;
    }
    let werr = ram::CNT_NVM_WERR.get(ram).saturating_add(1);
    ram::CNT_NVM_WERR.set(ram, werr);
    if ram::NVM_PENDING.get(ram) == 0 {
        log::push(ram, ev::NVM_WRITE_ERR, failed, werr);
    }
    ram::NVM_PENDING.set(ram, 1);
    false
}

/// Demande une écriture du bloc (effectuée par `service` au plus tard au cycle suivant, dans
/// `comms_rx`). Pour T-012 : à appeler après toute modification de `LINE_ISOLATED` ou de
/// `SHORT_BOOTS`.
pub fn request_save(ram: &mut Ram) {
    ram::NVM_PENDING.set(ram, 1);
}

/// Écriture en attente ?
pub fn pending(ram: &mut Ram) -> bool {
    ram::NVM_PENDING.get(ram) != 0
}

/// Effectue l'écriture en attente, s'il y en a une. Renvoie `true` si la NVM est à jour.
pub fn service<H: Hal>(ram: &mut Ram, hal: &mut H) -> bool {
    if !pending(ram) {
        return true;
    }
    save(ram, hal)
}

/// Relecture de la NVM au boot. À appeler APRÈS l'effacement de la RAM et l'initialisation des
/// valeurs par défaut (`sched::boot`) ; idempotente pendant un même boot (bit
/// `STATUS_LOADED`). Charge `BOOT_COUNT` (+1, saturant), `LAST_TC_SEQ`, `CEILING`,
/// `LINE_ISOLATED`, `SHORT_BOOTS` ; pose `FDIR_FLAG_NVM_LOST` si aucune copie n'est valide ;
/// réécrit les trois copies ; journalise `BOOT` ; réapplique les lignes.
pub fn boot_load<H: Hal>(ram: &mut Ram, hal: &mut H) {
    if ram::NVM_STATUS.get(ram) & STATUS_LOADED != 0 {
        return;
    }
    let mut valid = 0u8;
    let mut chosen = None;
    let mut virgin = true;
    for (c, &off) in COPY_OFF.iter().enumerate() {
        let mut b = [0u8; BLOCK_LEN];
        if hal.nvm_read(off, &mut b).is_err() {
            virgin = false;
            continue;
        }
        if b.iter().any(|&x| x != 0xFF) {
            virgin = false;
        }
        if let Some(p) = Persist::decode(&b) {
            valid |= 1 << c;
            if chosen.is_none() {
                chosen = Some(p);
            }
        }
    }
    let p = match chosen {
        Some(p) => p,
        None if virgin => {
            log::push(ram, ev::NVM_FORMAT, 0, 0);
            Persist::DEFAULT
        }
        None => {
            let f = ram::FDIR_FLAGS.get(ram) | ram::FDIR_FLAG_NVM_LOST;
            ram::FDIR_FLAGS.set(ram, f);
            log::push(ram, ev::NVM_LOST, 0, 0);
            Persist::DEFAULT
        }
    };
    let p = Persist {
        boot_count: p.boot_count.saturating_add(1),
        ..p
    };
    p.to_cells(ram);
    ram::NVM_STATUS.set(ram, valid | STATUS_LOADED);
    let cause = ram::RESET_CAUSE.get(ram);
    log::push(ram, ev::BOOT, cause, p.boot_count);
    let saved = save(ram, hal);
    if chosen.is_some() && valid != STATUS_VALID_MASK && saved {
        // `valid` a au moins un bit : au plus 2 copies réparées.
        let repaired = 3 - valid.count_ones() as u16;
        let n = ram::CNT_NVM_REPAIR.get(ram).saturating_add(repaired);
        ram::CNT_NVM_REPAIR.set(ram, n);
        log::push(ram, ev::NVM_REPAIRED, valid, repaired);
    }
    mode::apply_lines(ram, hal);
}

/// Aides de test partagées (NVM défaillante, trace d'ordre des appels matériels).
#[cfg(test)]
pub mod testkit {
    extern crate std;
    use crate::hal::{Hal, HalError, Reg, ResetCause, SensorId, MAX_FRAME};
    use crate::testhal::MockHal;
    use crate::Firmware;
    use std::vec::Vec;

    /// Appel matériel observable, dans l'ordre.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Call {
        Send(Vec<u8>),
        NvmWrite(u16, Vec<u8>),
        Reset,
    }

    /// `MockHal` + échecs NVM pilotables + trace ordonnée des émissions, écritures et resets.
    pub struct TraceHal {
        pub inner: MockHal,
        pub nvm_write_fail: bool,
        pub nvm_read_fail: bool,
        pub calls: Vec<Call>,
    }

    impl Default for TraceHal {
        fn default() -> Self {
            Self::new()
        }
    }

    impl TraceHal {
        pub fn new() -> Self {
            TraceHal {
                inner: MockHal::nominal(),
                nvm_write_fail: false,
                nvm_read_fail: false,
                calls: Vec::new(),
            }
        }
    }

    impl Hal for TraceHal {
        fn sensor(&mut self, id: SensorId) -> Result<i16, HalError> {
            self.inner.sensor(id)
        }
        fn reg_read(&mut self, r: Reg) -> u16 {
            self.inner.reg_read(r)
        }
        fn reg_write(&mut self, r: Reg, v: u16) {
            self.inner.reg_write(r, v)
        }
        fn wdg_kick(&mut self, key: u8) {
            self.inner.wdg_kick(key)
        }
        fn radio_recv(&mut self, buf: &mut [u8; MAX_FRAME]) -> Option<usize> {
            self.inner.radio_recv(buf)
        }
        fn radio_send(&mut self, frame: &[u8]) -> Result<(), HalError> {
            let r = self.inner.radio_send(frame);
            if r.is_ok() {
                self.calls.push(Call::Send(frame.to_vec()));
            }
            r
        }
        fn nvm_read(&mut self, off: u16, buf: &mut [u8]) -> Result<(), HalError> {
            if self.nvm_read_fail {
                return Err(HalError::Io);
            }
            self.inner.nvm_read(off, buf)
        }
        fn nvm_write(&mut self, off: u16, data: &[u8]) -> Result<(), HalError> {
            if self.nvm_write_fail {
                return Err(HalError::Busy);
            }
            self.calls.push(Call::NvmWrite(off, data.to_vec()));
            self.inner.nvm_write(off, data)
        }
        fn reset_cause(&mut self) -> ResetCause {
            self.inner.reset_cause()
        }
        fn sys_reset(&mut self) {
            self.calls.push(Call::Reset);
            self.inner.sys_reset()
        }
    }

    /// Boot complet dans l'ordre prévu pour `sched::boot` (câblage du manager) :
    /// `boot_erase`, relecture NVM, `boot_finish`. Reste juste une fois le câblage fait
    /// (`boot_load` idempotente pendant un boot).
    pub fn boot_full<H: Hal>(fw: &mut Firmware, hal: &mut H) {
        let ram = fw.ram_mut();
        crate::sched::boot_erase(ram, hal);
        super::boot_load(ram, hal);
        crate::sched::boot_finish(ram, hal);
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::testkit::{boot_full, TraceHal};
    use super::*;
    use crate::hal::NVM_SIZE;
    use crate::proto::{self, Beacon, Command, FrameType};
    use crate::testhal::{MockHal, TestRng};
    use crate::Firmware;
    use std::println;

    fn copy(hal: &MockHal, c: usize) -> [u8; BLOCK_LEN] {
        let o = COPY_OFF[c] as usize;
        let mut b = [0u8; BLOCK_LEN];
        b.copy_from_slice(&hal.nvm[o..o + BLOCK_LEN]);
        b
    }

    fn put(hal: &mut MockHal, c: usize, b: &[u8; BLOCK_LEN]) {
        let o = COPY_OFF[c] as usize;
        hal.nvm[o..o + BLOCK_LEN].copy_from_slice(b);
    }

    fn last_beacon(hal: &MockHal) -> Beacon {
        *hal.beacons().last().expect("aucune balise")
    }

    /// NVM préparée avec un bloc connu dans les trois copies.
    fn nvm_with(p: &Persist) -> MockHal {
        let mut hal = MockHal::nominal();
        let b = p.encode();
        for c in 0..3 {
            put(&mut hal, c, &b);
        }
        hal
    }

    const SAMPLE: Persist = Persist {
        boot_count: 41,
        last_tc_seq: 777,
        ceiling: Mode::Eco,
        isolated: 0b100,
        short_boots: 2,
    };

    #[test]
    fn block_roundtrip_crc_and_bounds() {
        let b = SAMPLE.encode();
        assert_eq!(Persist::decode(&b), Some(SAMPLE));
        for i in 0..BLOCK_LEN {
            for bit in 0..8 {
                let mut x = b;
                x[i] ^= 1 << bit;
                assert_eq!(Persist::decode(&x), None, "octet {i} bit {bit}");
            }
        }
        // CRC valide, champs hors domaine : bornés.
        let mut raw = b;
        raw[8] = 9;
        raw[9] = 0xFF;
        raw[10] = 200;
        let crc = crc32(&raw[..DATA_LEN]);
        raw[DATA_LEN..].copy_from_slice(&crc.to_be_bytes());
        let p = Persist::decode(&raw).unwrap();
        assert_eq!(p.ceiling, Mode::Nominal);
        assert_eq!(p.isolated, ISOLATABLE);
        assert_eq!(p.short_boots, SHORT_BOOTS_MAX);
    }

    #[test]
    fn virgin_nvm_gives_defaults_without_nvm_lost() {
        let mut fw = Firmware::new();
        let mut hal = MockHal::nominal();
        boot_full(&mut fw, &mut hal);
        let ram = fw.ram_mut();
        assert_eq!(ram::BOOT_COUNT.get(ram), 1);
        assert_eq!(ram::FDIR_FLAGS.get(ram) & ram::FDIR_FLAG_NVM_LOST, 0);
        assert_eq!(ram::NVM_STATUS.get(ram), STATUS_LOADED);
        let expected = Persist {
            boot_count: 1,
            ..Persist::DEFAULT
        };
        for c in 0..3 {
            assert_eq!(Persist::decode(&copy(&hal, c)), Some(expected));
        }
        assert_eq!(log::read(fw.ram_mut(), 0).unwrap().code, ev::NVM_FORMAT);
    }

    #[test]
    fn boot_loads_fields_and_increments_boot_count() {
        let mut fw = Firmware::new();
        let mut hal = nvm_with(&SAMPLE);
        // Valeurs relues, avant `boot_finish` (où le FDIR compte ce boot comme court).
        crate::sched::boot_erase(fw.ram_mut(), &mut hal);
        boot_load(fw.ram_mut(), &mut hal);
        // Idempotente : un second appel ne recompte pas le boot.
        boot_load(fw.ram_mut(), &mut hal);
        crate::sched::boot_finish(fw.ram_mut(), &mut hal);
        let ram = fw.ram_mut();
        assert_eq!(ram::BOOT_COUNT.get(ram), 42);
        assert_eq!(ram::LAST_TC_SEQ.get(ram), 777);
        assert_eq!(mode::ceiling(ram), Mode::Eco);
        assert_eq!(ram::LINE_ISOLATED.get(ram), 0b100);
        // 2 relu, + 1 par `fdir::on_boot` (sauf témoin non protégé).
        assert!((2..=3).contains(&ram::SHORT_BOOTS.get(ram)));
        assert_eq!(ram::NVM_STATUS.get(ram), STATUS_LOADED | 0b111);
        assert_eq!(ram::CNT_NVM_REPAIR.get(ram), 0);
        fw.step(&mut hal);
        let b = last_beacon(&hal);
        assert_eq!((b.boot_count, b.last_tc_seq), (42, 777));
        assert_eq!(b.fdir_flags & ram::FDIR_FLAG_NVM_LOST, 0);
        // Reboot : 43.
        boot_full(&mut fw, &mut hal);
        assert_eq!(ram::BOOT_COUNT.get(fw.ram_mut()), 43);
    }

    /// 1 puis 2 copies corrompues (toutes les combinaisons) : réparées, valeurs justes.
    #[test]
    fn one_or_two_corrupted_copies_are_repaired() {
        let seed = 0x00A1_5EED_u64;
        println!("graine NVM 1-2 copies : {seed:#x}");
        let mut rng = TestRng::new(seed);
        for mask in 1u8..7 {
            let mut hal = nvm_with(&SAMPLE);
            for (c, &off) in COPY_OFF.iter().enumerate() {
                if mask & (1 << c) != 0 {
                    let o = off as usize + rng.below(BLOCK_LEN as u64) as usize;
                    hal.nvm[o] ^= 1 << rng.below(8);
                }
            }
            let mut fw = Firmware::new();
            boot_full(&mut fw, &mut hal);
            let ram = fw.ram_mut();
            assert_eq!(ram::BOOT_COUNT.get(ram), 42, "masque {mask:#b}");
            assert_eq!(ram::LAST_TC_SEQ.get(ram), 777);
            assert_eq!(ram::FDIR_FLAGS.get(ram) & ram::FDIR_FLAG_NVM_LOST, 0);
            assert_eq!(ram::NVM_STATUS.get(ram) & 0b111, !mask & 0b111);
            assert_eq!(
                ram::CNT_NVM_REPAIR.get(ram),
                mask.count_ones() as u16,
                "masque {mask:#b}"
            );
            let want = Persist {
                boot_count: 42,
                ..SAMPLE
            };
            for c in 0..3 {
                assert_eq!(Persist::decode(&copy(&hal, c)), Some(want));
            }
        }
    }

    #[test]
    fn three_corrupted_copies_give_defaults_and_nvm_lost_in_beacon() {
        let mut hal = nvm_with(&SAMPLE);
        for &off in &COPY_OFF {
            hal.nvm[off as usize + 5] ^= 0x10;
        }
        let mut fw = Firmware::new();
        boot_full(&mut fw, &mut hal);
        fw.step(&mut hal);
        let b = last_beacon(&hal);
        assert_ne!(b.fdir_flags & ram::FDIR_FLAG_NVM_LOST, 0);
        assert_eq!(b.boot_count, 1);
        assert_eq!(b.last_tc_seq, 0);
        assert_eq!(mode::ceiling(fw.ram_mut()), Mode::Nominal);
        assert_eq!(log::read(fw.ram_mut(), 0).unwrap().code, ev::NVM_LOST);
        // Drapeau maintenu pendant tout le boot (le FDIR ne l'efface pas).
        for _ in 0..200 {
            fw.step(&mut hal);
        }
        assert_ne!(last_beacon(&hal).fdir_flags & ram::FDIR_FLAG_NVM_LOST, 0);
        // Copies réécrites avec les défauts : le boot suivant est propre.
        boot_full(&mut fw, &mut hal);
        let ram = fw.ram_mut();
        assert_eq!(ram::FDIR_FLAGS.get(ram) & ram::FDIR_FLAG_NVM_LOST, 0);
        assert_eq!(ram::BOOT_COUNT.get(ram), 2);
    }

    #[test]
    fn nvm_read_error_counts_as_invalid_copy_and_lost() {
        let mut fw = Firmware::new();
        let mut hal = TraceHal::new();
        hal.nvm_read_fail = true;
        boot_full(&mut fw, &mut hal);
        let ram = fw.ram_mut();
        assert_ne!(ram::FDIR_FLAGS.get(ram) & ram::FDIR_FLAG_NVM_LOST, 0);
        // Relecture de vérification impossible : écriture en attente.
        assert!(pending(ram));
        for _ in 0..20 {
            fw.step(&mut hal);
        }
    }

    #[test]
    fn nvm_write_error_does_not_panic_and_is_retried() {
        let mut fw = Firmware::new();
        let mut hal = TraceHal::new();
        let b = SAMPLE.encode();
        for &off in &COPY_OFF {
            let o = off as usize;
            hal.inner.nvm[o..o + BLOCK_LEN].copy_from_slice(&b);
        }
        hal.nvm_write_fail = true;
        boot_full(&mut fw, &mut hal);
        assert_eq!(ram::BOOT_COUNT.get(fw.ram_mut()), 42);
        assert!(pending(fw.ram_mut()));
        for _ in 0..10 {
            fw.step(&mut hal);
        }
        let ram = fw.ram_mut();
        assert!(pending(ram));
        assert!(ram::CNT_NVM_WERR.get(ram) >= 10);
        // Un seul événement pour la série d'échecs.
        let werr_events = (0..log::count(ram))
            .filter(|&k| log::read(ram, k).unwrap().code == ev::NVM_WRITE_ERR)
            .count();
        assert_eq!(werr_events, 1);
        // La NVM revient : écriture faite au cycle suivant.
        hal.nvm_write_fail = false;
        fw.step(&mut hal);
        assert!(!pending(fw.ram_mut()));
        let o = COPY_OFF[2] as usize;
        let mut x = [0u8; BLOCK_LEN];
        x.copy_from_slice(&hal.inner.nvm[o..o + BLOCK_LEN]);
        assert_eq!(Persist::decode(&x).unwrap().boot_count, 42);
    }

    /// Pousse quelques TC (SEQ divers, y compris autour de `last_tc_seq` et près de
    /// `u32::MAX`) puis fait tourner `n` cycles.
    fn run_with_tcs(fw: &mut Firmware, hal: &mut MockHal, rng: &mut TestRng, n: u32) {
        for k in 0..n {
            if k % 37 == 0 {
                let last = ram::LAST_TC_SEQ.get(fw.ram_mut());
                let seq = match rng.below(5) {
                    0 => last.wrapping_add(1),
                    1 => last.wrapping_add(1025),
                    2 => u32::MAX,
                    3 => rng.next_u64() as u32,
                    _ => last,
                };
                let cmd = match rng.below(4) {
                    0 => Command::Ping,
                    1 => Command::SetMode(rng.below(4) as u8),
                    2 => Command::PowerCycle(rng.below(4) as u8),
                    _ => Command::DumpEvents,
                };
                hal.push_tc(seq, &cmd);
            }
            fw.step(hal);
        }
    }

    fn assert_cells_bounded(ram: &mut Ram) {
        assert!(Mode::from_u8(ram::CEILING.get(ram)).is_some());
        assert_eq!(ram::LINE_ISOLATED.get(ram) & !ISOLATABLE, 0);
        // Relu borné à `SHORT_BOOTS_MAX`, puis + 1 par `fdir::on_boot`.
        assert!(ram::SHORT_BOOTS.get(ram) <= SHORT_BOOTS_MAX + 1);
        assert!(log::count(ram) <= log::CAPACITY);
    }

    /// R9 : 1000 blocs à CRC VALIDE et contenu aléatoire -> boot + 300 cycles sans panique,
    /// chaque champ relu borné.
    #[test]
    fn random_crc_valid_blocks_never_panic() {
        let seed = 0x0B5E_55ED_u64;
        println!("graine NVM CRC valide : {seed:#x}");
        let mut rng = TestRng::new(seed);
        for i in 0..1000 {
            let mut hal = MockHal::nominal();
            for c in 0..3 {
                let mut b = [0u8; BLOCK_LEN];
                for x in b.iter_mut() {
                    *x = rng.next_u64() as u8;
                }
                // La plupart des blocs ont l'en-tête attendu (sinon ils seraient rejetés et le
                // test ne testerait que `NVM_LOST`).
                if rng.below(8) != 0 {
                    b[0] = MAGIC;
                    b[1] = FORMAT;
                }
                if i % 4 == 0 {
                    // Cas R9 extrême : SEQ saturé.
                    b[4..8].copy_from_slice(&(u32::MAX - rng.below(2000) as u32).to_be_bytes());
                }
                let crc = crc32(&b[..DATA_LEN]);
                b[DATA_LEN..].copy_from_slice(&crc.to_be_bytes());
                put(&mut hal, c, &b);
            }
            let mut fw = Firmware::new();
            boot_full(&mut fw, &mut hal);
            assert_cells_bounded(fw.ram_mut());
            run_with_tcs(&mut fw, &mut hal, &mut rng, 300);
            assert_cells_bounded(fw.ram_mut());
            // L'image NVM réécrite est relue identique au boot suivant (champs hors FDIR).
            assert!(!pending(fw.ram_mut()), "image {i}");
            let before = Persist::from_cells(fw.ram_mut());
            crate::sched::boot_erase(fw.ram_mut(), &mut hal);
            boot_load(fw.ram_mut(), &mut hal);
            let after = Persist::from_cells(fw.ram_mut());
            assert_eq!(
                after,
                Persist {
                    boot_count: before.boot_count.saturating_add(1),
                    ..before
                },
                "image {i}"
            );
        }
    }

    /// 1000 images NVM entièrement aléatoires -> `boot` ne panique jamais.
    #[test]
    fn random_nvm_images_never_panic() {
        let seed = 0x5EED_0001_u64;
        println!("graine NVM aléatoire : {seed:#x}");
        let mut rng = TestRng::new(seed);
        for _ in 0..1000 {
            let mut hal = MockHal::nominal();
            for x in hal.nvm.iter_mut() {
                *x = rng.next_u64() as u8;
            }
            let mut fw = Firmware::new();
            boot_full(&mut fw, &mut hal);
            assert_cells_bounded(fw.ram_mut());
            // CRC-32 aléatoire valide : probabilité 2^-32 ; NVM_LOST attendu.
            assert_ne!(
                ram::FDIR_FLAGS.get(fw.ram_mut()) & ram::FDIR_FLAG_NVM_LOST,
                0
            );
            run_with_tcs(&mut fw, &mut hal, &mut rng, 20);
        }
        assert_eq!(NVM_SIZE, 256);
    }

    /// Ce qui est écrit en NVM après acceptation d'une TC porte le SEQ ; une balise suit.
    #[test]
    fn beacon_publishes_loaded_values() {
        let mut fw = Firmware::new();
        let mut hal = nvm_with(&SAMPLE);
        boot_full(&mut fw, &mut hal);
        fw.step(&mut hal);
        let f = hal.sent.last().unwrap().clone();
        let fr = proto::decode_frame(&f).unwrap();
        assert_eq!(fr.ftype, FrameType::Beacon);
        assert_eq!(fr.payload.len(), Beacon::LEN);
    }
}
