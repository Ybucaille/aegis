//! Journal d'événements borné, protégé SECDED (SPEC §4.3), et format des trames EVENTS (§5).
//!
//! Stockage : anneau de `CAPACITY` = 40 entrées de 8 octets dans `ram::LOG_BUF` (SECDED (13,8)
//! entrelacé), en-tête `LOG_HEAD` (prochaine entrée à écrire), `LOG_COUNT` (entrées valides),
//! `LOG_TOTAL` (événements depuis le boot, saturant). Le journal est en RAM : il est perdu à
//! tout reset (l'événement `BOOT` ouvre chaque journal).
//!
//! Lectures bornées (R9/R13) : `LOG_HEAD` est lu modulo `CAPACITY`, `LOG_COUNT` borné à
//! `CAPACITY` ; aucun index calculé ne peut sortir du tampon. Une entrée dont un octet est en
//! erreur double (SECDED) est réécrite comme entrée `ev::CORRUPT` (temps et arguments à 0) : la
//! donnée invalide n'est jamais utilisée, et l'erreur n'est comptée qu'une fois.
//!
//! ## Format d'une entrée (8 octets, gros-boutiste)
//!
//! | Octets | Champ |
//! |---|---|
//! | 0..4 | `t` : uptime du boot courant, en s (`ram::UPTIME` au moment de l'écriture) |
//! | 4 | `code` : voir `ev` |
//! | 5 | `a` : argument 1 (sens selon le code) |
//! | 6..8 | `b` : argument 2 (sens selon le code) |
//!
//! ## Charge utile d'une trame EVENTS (TYPE = 4)
//!
//! | Octets | Champ |
//! |---|---|
//! | 0..2 | `log_total u16` : `LOG_TOTAL` au moment de l'émission |
//! | 2 | `left u8` : entrées restant à émettre APRÈS cette trame (0 = dernière trame du vidage) |
//! | 3 | `n u8` : entrées dans cette trame (0..=5) |
//! | 4.. | `n` entrées de 8 octets, de la plus ancienne à la plus récente |
//!
//! Longueur = 4 + 8 n <= 44 <= `MAX_PAYLOAD`. Un `DumpEvents` émet `ceil(count / 5)` trames
//! (au moins une, `n = 0` si le journal est vide), UNE par cycle. Les entrées écrites pendant le
//! vidage peuvent écraser des entrées pas encore émises si l'anneau est plein (best effort,
//! chaque entrée porte son temps).

use crate::proto::MAX_PAYLOAD;
use crate::ram::{self, Ram, LOG_BUF, LOG_BUF_LEN};

/// Octets par entrée.
pub const ENTRY_LEN: usize = 8;
/// Entrées dans l'anneau.
pub const CAPACITY: usize = LOG_BUF_LEN / ENTRY_LEN;
const _: () = assert!(CAPACITY == 40);
/// En-tête d'une charge utile EVENTS.
pub const EVENTS_HDR_LEN: usize = 4;
/// Entrées par trame EVENTS.
pub const ENTRIES_PER_FRAME: usize = 5;
const _: () = assert!(EVENTS_HDR_LEN + ENTRIES_PER_FRAME * ENTRY_LEN <= MAX_PAYLOAD);

/// Valeur de `ram::DUMP_ACTIVE` signifiant « vidage en cours ». Toute autre valeur = inactif.
pub const DUMP_ACTIVE_YES: u8 = 0xD5;

/// Codes d'événements. `0x20..=0x3F` : réservés au FDIR (T-012), qui choisit leur sens.
pub mod ev {
    /// Boot : `a` = cause du reset (`ResetCause as u8`), `b` = `boot_count`.
    pub const BOOT: u8 = 0x01;
    /// Aucune copie NVM valide : valeurs par défaut. `a` = 0, `b` = 0.
    pub const NVM_LOST: u8 = 0x02;
    /// Copies NVM réparées au boot : `a` = masque des copies trouvées valides, `b` = réparées.
    pub const NVM_REPAIRED: u8 = 0x03;
    /// Échec d'écriture NVM (début d'une série) : `a` = masque des copies en échec,
    /// `b` = `CNT_NVM_WERR`.
    pub const NVM_WRITE_ERR: u8 = 0x04;
    /// NVM vierge (tout à `0xFF`) : formatage avec les valeurs par défaut.
    pub const NVM_FORMAT: u8 = 0x05;
    /// TC exécutée : `a` = opcode, `b` = 16 bits de poids faible du SEQ.
    pub const TC_EXEC: u8 = 0x10;
    /// Plafond changé par `SetMode` : `a` = nouveau plafond.
    pub const CEILING_SET: u8 = 0x11;
    /// `Reboot` accepté : `a` = 0.
    pub const REBOOT_CMD: u8 = 0x12;
    /// `PowerCycle` : `a` = ligne coupée.
    pub const POWER_CYCLE: u8 = 0x13;
    /// `ClearFdir` exécutée.
    pub const CLEAR_FDIR: u8 = 0x15;
    /// `sys_reset` émis après l'ACK d'un `Reboot` : `a` = 1 si la NVM était à jour.
    pub const REBOOT_RESET: u8 = 0x16;
    /// Entrée trouvée en erreur double (SECDED), invalidée.
    pub const CORRUPT: u8 = 0xEE;
}

/// Une entrée du journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub t: u32,
    pub code: u8,
    pub a: u8,
    pub b: u16,
}

impl Entry {
    pub const CORRUPT: Entry = Entry {
        t: 0,
        code: ev::CORRUPT,
        a: 0,
        b: 0,
    };

    pub fn encode(&self) -> [u8; ENTRY_LEN] {
        let t = self.t.to_be_bytes();
        let b = self.b.to_be_bytes();
        [t[0], t[1], t[2], t[3], self.code, self.a, b[0], b[1]]
    }

    pub fn decode(x: &[u8; ENTRY_LEN]) -> Entry {
        Entry {
            t: u32::from_be_bytes([x[0], x[1], x[2], x[3]]),
            code: x[4],
            a: x[5],
            b: u16::from_be_bytes([x[6], x[7]]),
        }
    }
}

/// En-tête décodé d'une charge utile EVENTS (aide pour le sol et les tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventsHeader {
    pub log_total: u16,
    pub left: u8,
    pub n: u8,
}

/// Décode une charge utile EVENTS : en-tête et entrées (au plus `ENTRIES_PER_FRAME`).
/// `None` si la longueur est incohérente avec `n`.
pub fn decode_events(p: &[u8]) -> Option<(EventsHeader, [Entry; ENTRIES_PER_FRAME])> {
    if p.len() < EVENTS_HDR_LEN {
        return None;
    }
    let h = EventsHeader {
        log_total: u16::from_be_bytes([p[0], p[1]]),
        left: p[2],
        n: p[3],
    };
    let n = h.n as usize;
    if n > ENTRIES_PER_FRAME || p.len() != EVENTS_HDR_LEN + n * ENTRY_LEN {
        return None;
    }
    let mut out = [Entry::CORRUPT; ENTRIES_PER_FRAME];
    for (k, e) in out.iter_mut().enumerate().take(n) {
        let s = EVENTS_HDR_LEN + k * ENTRY_LEN;
        let mut x = [0u8; ENTRY_LEN];
        x.copy_from_slice(&p[s..s + ENTRY_LEN]);
        *e = Entry::decode(&x);
    }
    Some((h, out))
}

fn head(ram: &mut Ram) -> usize {
    ram::LOG_HEAD.get(ram) as usize % CAPACITY
}

/// Nombre d'entrées valides (borné à `CAPACITY`).
pub fn count(ram: &mut Ram) -> usize {
    (ram::LOG_COUNT.get(ram) as usize).min(CAPACITY)
}

/// Événements journalisés depuis le boot (saturant).
pub fn total(ram: &mut Ram) -> u16 {
    ram::LOG_TOTAL.get(ram)
}

fn write_slot(ram: &mut Ram, slot: usize, e: &Entry) {
    let base = (slot % CAPACITY) * ENTRY_LEN;
    for (k, &byte) in e.encode().iter().enumerate() {
        LOG_BUF.set(ram, base + k, byte);
    }
}

/// Lit l'entrée de l'emplacement physique `slot` (modulo `CAPACITY`). Erreur double sur un
/// octet : l'entrée est réécrite `CORRUPT` et rendue comme telle.
fn read_slot(ram: &mut Ram, slot: usize) -> Entry {
    let base = (slot % CAPACITY) * ENTRY_LEN;
    let mut x = [0u8; ENTRY_LEN];
    let mut ok = true;
    for (k, o) in x.iter_mut().enumerate() {
        match LOG_BUF.get(ram, base + k) {
            Some(v) => *o = v,
            None => ok = false,
        }
    }
    if ok {
        Entry::decode(&x)
    } else {
        write_slot(ram, slot, &Entry::CORRUPT);
        Entry::CORRUPT
    }
}

/// Emplacement physique de la `k`-ième entrée la plus ancienne (`k < count`).
fn slot_of(ram: &mut Ram, k: usize) -> usize {
    let n = count(ram);
    (head(ram) + CAPACITY - n + k % CAPACITY) % CAPACITY
}

/// `k`-ième entrée la plus ancienne ; `None` si `k >= count`.
pub fn read(ram: &mut Ram, k: usize) -> Option<Entry> {
    if k >= count(ram) {
        return None;
    }
    let s = slot_of(ram, k);
    Some(read_slot(ram, s))
}

/// Ajoute un événement daté de l'uptime courant. L'entrée la plus ancienne est écrasée si
/// l'anneau est plein.
pub fn push(ram: &mut Ram, code: u8, a: u8, b: u16) {
    let e = Entry {
        t: ram::UPTIME.get(ram),
        code,
        a,
        b,
    };
    let h = head(ram);
    write_slot(ram, h, &e);
    // `h + 1 <= CAPACITY = 40` : conversion exacte.
    ram::LOG_HEAD.set(ram, ((h + 1) % CAPACITY) as u16);
    let n = (count(ram) + 1).min(CAPACITY);
    ram::LOG_COUNT.set(ram, n as u16);
    let t = ram::LOG_TOTAL.get(ram).saturating_add(1);
    ram::LOG_TOTAL.set(ram, t);
}

/// Démarre (ou redémarre) un vidage du journal entier, du plus ancien au plus récent.
pub fn start_dump(ram: &mut Ram) {
    let n = count(ram);
    let first = slot_of(ram, 0);
    // `first < 40`, `n <= 40` : conversions exactes.
    ram::DUMP_CURSOR.set(ram, first as u16);
    ram::DUMP_LEFT.set(ram, n as u16);
    ram::DUMP_ACTIVE.set(ram, DUMP_ACTIVE_YES);
}

/// Vidage en cours ?
pub fn dump_active(ram: &mut Ram) -> bool {
    ram::DUMP_ACTIVE.get(ram) == DUMP_ACTIVE_YES
}

/// Prépare la prochaine charge utile EVENTS d'un vidage en cours et avance le curseur.
/// Renvoie sa longueur, ou `None` si aucun vidage n'est en cours. Le vidage se termine quand
/// la dernière trame est préparée (que son émission réussisse ou non : pas de nouvel essai).
pub fn dump_next(ram: &mut Ram, out: &mut [u8; MAX_PAYLOAD]) -> Option<usize> {
    if !dump_active(ram) {
        return None;
    }
    let cursor = ram::DUMP_CURSOR.get(ram) as usize % CAPACITY;
    let left = (ram::DUMP_LEFT.get(ram) as usize).min(CAPACITY);
    let n = left.min(ENTRIES_PER_FRAME);
    let after = left - n;
    out[0..2].copy_from_slice(&total(ram).to_be_bytes());
    // `after <= 40`, `n <= 5` : conversions exactes.
    out[2] = after as u8;
    out[3] = n as u8;
    for k in 0..n {
        let e = read_slot(ram, cursor + k);
        let s = EVENTS_HDR_LEN + k * ENTRY_LEN;
        out[s..s + ENTRY_LEN].copy_from_slice(&e.encode());
    }
    ram::DUMP_CURSOR.set(ram, ((cursor + n) % CAPACITY) as u16);
    ram::DUMP_LEFT.set(ram, after as u16);
    if after == 0 {
        ram::DUMP_ACTIVE.set(ram, 0);
    }
    Some(EVENTS_HDR_LEN + n * ENTRY_LEN)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::hal::RAM_SIZE;
    use crate::testhal::TestRng;
    use std::println;
    use std::vec::Vec;

    fn drain(ram: &mut Ram) -> (Vec<EventsHeader>, Vec<Entry>) {
        let mut hs = Vec::new();
        let mut es = Vec::new();
        let mut out = [0u8; MAX_PAYLOAD];
        while let Some(n) = dump_next(ram, &mut out) {
            let (h, e) = decode_events(&out[..n]).expect("charge EVENTS invalide");
            hs.push(h);
            es.extend_from_slice(&e[..h.n as usize]);
            assert!(hs.len() <= 9, "vidage sans fin");
        }
        (hs, es)
    }

    #[test]
    fn entry_roundtrip() {
        let e = Entry {
            t: 0x0102_0304,
            code: 0x10,
            a: 0xAB,
            b: 0xCDEF,
        };
        assert_eq!(e.encode(), [1, 2, 3, 4, 0x10, 0xAB, 0xCD, 0xEF]);
        assert_eq!(Entry::decode(&e.encode()), e);
    }

    #[test]
    fn push_read_and_wraparound() {
        let mut ram = [0u8; RAM_SIZE];
        assert_eq!(count(&mut ram), 0);
        assert_eq!(read(&mut ram, 0), None);
        for i in 0..50u16 {
            ram::UPTIME.set(&mut ram, i as u32);
            push(&mut ram, 0x30, i as u8, i);
        }
        assert_eq!(count(&mut ram), CAPACITY);
        assert_eq!(total(&mut ram), 50);
        // Les 40 plus récentes, de la plus ancienne (10) à la plus récente (49).
        for k in 0..CAPACITY {
            let e = read(&mut ram, k).unwrap();
            assert_eq!(e.b as usize, 10 + k);
            assert_eq!(e.t as usize, 10 + k);
        }
        assert_eq!(read(&mut ram, CAPACITY), None);
    }

    #[test]
    fn dump_of_full_log_is_ordered_and_bounded() {
        let mut ram = [0u8; RAM_SIZE];
        for i in 0..45u16 {
            push(&mut ram, 0x31, 0, i);
        }
        start_dump(&mut ram);
        let (hs, es) = drain(&mut ram);
        assert_eq!(hs.len(), 8);
        assert_eq!(es.len(), 40);
        assert!(es.iter().enumerate().all(|(k, e)| e.b as usize == 5 + k));
        let lefts: Vec<u8> = hs.iter().map(|h| h.left).collect();
        assert_eq!(lefts, [35, 30, 25, 20, 15, 10, 5, 0]);
        assert!(hs.iter().all(|h| h.log_total == 45 && h.n == 5));
        assert!(!dump_active(&mut ram));
    }

    #[test]
    fn dump_of_empty_and_partial_log() {
        let mut ram = [0u8; RAM_SIZE];
        start_dump(&mut ram);
        let (hs, es) = drain(&mut ram);
        assert_eq!(hs.len(), 1);
        assert_eq!((hs[0].n, hs[0].left), (0, 0));
        assert!(es.is_empty());
        for i in 0..7u16 {
            push(&mut ram, 0x31, 0, i);
        }
        start_dump(&mut ram);
        let (hs, es) = drain(&mut ram);
        assert_eq!(hs.iter().map(|h| h.n).collect::<Vec<_>>(), [5, 2]);
        assert_eq!(
            es.iter().map(|e| e.b).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn decode_events_rejects_bad_lengths() {
        assert!(decode_events(&[0, 0, 0]).is_none());
        assert!(decode_events(&[0, 0, 0, 1]).is_none());
        assert!(decode_events(&[0, 0, 0, 6]).is_none());
        let mut p = [0u8; 12];
        p[3] = 1;
        assert!(decode_events(&p).is_some());
    }

    /// Header et curseurs corrompus (valeurs arbitraires) : aucune panique, vidage borné.
    #[test]
    fn corrupted_header_and_cursor_are_bounded() {
        let seed = 0x10C0_FFEE_u64;
        println!("graine journal corrompu : {seed:#x}");
        let mut rng = TestRng::new(seed);
        for _ in 0..2000 {
            let mut ram = [0u8; RAM_SIZE];
            ram::LOG_HEAD.set(&mut ram, rng.next_u64() as u16);
            ram::LOG_COUNT.set(&mut ram, rng.next_u64() as u16);
            ram::LOG_TOTAL.set(&mut ram, rng.next_u64() as u16);
            ram::DUMP_CURSOR.set(&mut ram, rng.next_u64() as u16);
            ram::DUMP_LEFT.set(&mut ram, rng.next_u64() as u16);
            ram::DUMP_ACTIVE.set(&mut ram, DUMP_ACTIVE_YES);
            assert!(count(&mut ram) <= CAPACITY);
            let (hs, es) = drain(&mut ram);
            assert!(es.len() <= CAPACITY && hs.len() <= 8);
            push(&mut ram, 0x30, 1, 2);
            let k = count(&mut ram) - 1;
            assert_eq!(read(&mut ram, k).unwrap().b, 2);
            start_dump(&mut ram);
            let (_, es) = drain(&mut ram);
            assert_eq!(es.last().unwrap().b, 2);
        }
    }

    /// Erreur double dans une entrée : rendue `CORRUPT`, réécrite, comptée une seule fois.
    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn double_error_entry_is_invalidated() {
        let mut ram = [0u8; RAM_SIZE];
        for i in 0..3u16 {
            push(&mut ram, 0x30, 0, 100 + i);
        }
        // Entrée 1 (emplacement 1), octet 6 : deux bits du même mot de code.
        let idx = ram::LOG_BUF_START + ENTRY_LEN + 6;
        let (b0, l0) = ram::LOG_ECC.phys_bit(idx, 2);
        let (b1, l1) = ram::LOG_ECC.phys_bit(idx, 9);
        ram[b0] ^= 1 << l0;
        ram[b1] ^= 1 << l1;
        assert_eq!(read(&mut ram, 0).unwrap().b, 100);
        assert_eq!(read(&mut ram, 1), Some(Entry::CORRUPT));
        assert_eq!(ram::CNT_ECC_UNCORR.get(&mut ram), 1);
        assert_eq!(read(&mut ram, 1), Some(Entry::CORRUPT));
        assert_eq!(ram::CNT_ECC_UNCORR.get(&mut ram), 1);
        assert_eq!(read(&mut ram, 2).unwrap().b, 102);
    }
}
