//! Protocole radio (SPEC §5) : trame, CRC-32, charges utiles TC / ACK / BEACON.
//!
//! Une seule implémentation, partagée par `firmware` et `ground`. Octets en gros-boutiste.
//!
//! ```text
//! 0      2     3      7     8        8+LEN   8+LEN+4
//! | SYNC | TYPE | SEQ | LEN | payload | CRC-32 |
//! ```
//!
//! Le CRC-32 IEEE couvre `[2, 8+LEN)` et est écrit en gros-boutiste.
//! Ordre de validation : SYNC -> version/type -> LEN -> CRC. L'anti-rejeu n'est PAS ici
//! (il dépend de `last_tc_seq`, état du firmware) ; seule la largeur de fenêtre est exportée.
//! Aucune allocation : tampons de taille fixe uniquement.

use crate::hal::MAX_FRAME;

/// Mot de synchronisation.
pub const SYNC: [u8; 2] = [0xAE, 0x61];
/// Version du protocole (quartet haut de TYPE).
pub const VERSION: u8 = 1;
/// SYNC + TYPE + SEQ + LEN.
pub const HEADER_LEN: usize = 8;
/// Longueur du CRC final.
pub const CRC_LEN: usize = 4;
/// LEN maximal.
pub const MAX_PAYLOAD: usize = 48;
/// Fenêtre anti-rejeu : TC acceptée ssi `last < SEQ <= last + ANTI_REPLAY_WINDOW`.
pub const ANTI_REPLAY_WINDOW: u32 = 1024;

const _: () = assert!(HEADER_LEN + MAX_PAYLOAD + CRC_LEN == MAX_FRAME);

// ---------------------------------------------------------------------------------------------
// CRC-32 IEEE (réfléchi, poly 0xEDB88320, init et xor final 0xFFFFFFFF), table en ROM.
// ---------------------------------------------------------------------------------------------

const CRC_TABLE: [u32; 256] = make_crc_table();

const fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

/// CRC-32 IEEE de `data`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

// ---------------------------------------------------------------------------------------------
// Trame
// ---------------------------------------------------------------------------------------------

/// Type de trame (quartet bas de TYPE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FrameType {
    Tc = 1,
    Ack = 2,
    Beacon = 3,
    Events = 4,
}

impl FrameType {
    /// Octet TYPE complet (version dans le quartet haut).
    pub const fn type_byte(self) -> u8 {
        (VERSION << 4) | (self as u8)
    }

    /// Décodage de l'octet TYPE ; `None` si version ou type inconnus.
    pub const fn from_type_byte(b: u8) -> Option<FrameType> {
        if b >> 4 != VERSION {
            return None;
        }
        match b & 0x0F {
            1 => Some(FrameType::Tc),
            2 => Some(FrameType::Ack),
            3 => Some(FrameType::Beacon),
            4 => Some(FrameType::Events),
            _ => None,
        }
    }
}

/// Trame décodée ; la charge utile emprunte le tampon d'entrée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    pub ftype: FrameType,
    pub seq: u32,
    pub payload: &'a [u8],
}

/// Raison du rejet d'une trame, dans l'ordre de validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecodeError {
    /// Moins de 2 octets ou SYNC incorrect.
    BadSync,
    /// TYPE absent, version != 1 ou type inconnu.
    BadType,
    /// En-tête tronqué, LEN > 48, ou longueur reçue != 12 + LEN.
    BadLen,
    /// CRC incorrect.
    BadCrc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EncodeError {
    /// Charge utile > `MAX_PAYLOAD`.
    PayloadTooLong,
}

/// Longueur totale d'une trame portant `payload_len` octets.
pub const fn frame_len(payload_len: usize) -> usize {
    HEADER_LEN + payload_len + CRC_LEN
}

/// Encode une trame dans `out` ; renvoie le nombre d'octets écrits.
pub fn encode_frame(
    ftype: FrameType,
    seq: u32,
    payload: &[u8],
    out: &mut [u8; MAX_FRAME],
) -> Result<usize, EncodeError> {
    let len = payload.len();
    if len > MAX_PAYLOAD {
        return Err(EncodeError::PayloadTooLong);
    }
    Ok(write_frame(ftype, seq, payload, out))
}

/// Écrit la trame ; l'appelant garantit `payload.len() <= MAX_PAYLOAD`.
fn write_frame(ftype: FrameType, seq: u32, payload: &[u8], out: &mut [u8; MAX_FRAME]) -> usize {
    let len = payload.len();
    out[0..2].copy_from_slice(&SYNC);
    out[2] = ftype.type_byte();
    out[3..7].copy_from_slice(&seq.to_be_bytes());
    // `len <= 48` garanti par l'appelant : la conversion est exacte.
    out[7] = len as u8;
    out[HEADER_LEN..HEADER_LEN + len].copy_from_slice(payload);
    let crc = crc32(&out[2..HEADER_LEN + len]);
    out[HEADER_LEN + len..HEADER_LEN + len + CRC_LEN].copy_from_slice(&crc.to_be_bytes());
    frame_len(len)
}

/// Décode et valide une trame (SYNC -> type -> LEN -> CRC). Ne fait pas l'anti-rejeu.
/// `buf` doit contenir exactement la trame reçue (longueur rendue par `radio_recv`).
pub fn decode_frame(buf: &[u8]) -> Result<Frame<'_>, DecodeError> {
    if buf.len() < 2 || buf[0..2] != SYNC {
        return Err(DecodeError::BadSync);
    }
    let ftype = match buf.get(2).and_then(|&b| FrameType::from_type_byte(b)) {
        Some(t) => t,
        None => return Err(DecodeError::BadType),
    };
    if buf.len() < HEADER_LEN {
        return Err(DecodeError::BadLen);
    }
    let len = buf[7] as usize;
    if len > MAX_PAYLOAD || buf.len() != frame_len(len) {
        return Err(DecodeError::BadLen);
    }
    let crc_off = HEADER_LEN + len;
    let expected = crc32(&buf[2..crc_off]);
    let received = u32::from_be_bytes([
        buf[crc_off],
        buf[crc_off + 1],
        buf[crc_off + 2],
        buf[crc_off + 3],
    ]);
    if expected != received {
        return Err(DecodeError::BadCrc);
    }
    let seq = u32::from_be_bytes([buf[3], buf[4], buf[5], buf[6]]);
    Ok(Frame {
        ftype,
        seq,
        payload: &buf[HEADER_LEN..crc_off],
    })
}

/// Encode une trame dont la taille de charge utile `N` est fixée par son type ; la borne
/// `N <= MAX_PAYLOAD` est vérifiée à la compilation, l'encodage ne peut donc pas échouer.
fn encode_fixed<const N: usize>(
    ftype: FrameType,
    seq: u32,
    payload: &[u8],
    out: &mut [u8; MAX_FRAME],
) -> usize {
    const { assert!(N <= MAX_PAYLOAD) };
    let len = payload.len().min(N);
    write_frame(ftype, seq, &payload[..len], out)
}

// ---------------------------------------------------------------------------------------------
// Charges utiles
// ---------------------------------------------------------------------------------------------

/// Erreur de décodage d'une charge utile (trame déjà valide).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PayloadError {
    /// Charge utile vide (TC sans opcode).
    Empty,
    /// Opcode de TC inconnu.
    UnknownOpcode(u8),
    /// Longueur incompatible avec le type ou l'opcode.
    BadLength,
    /// Valeur de champ hors domaine (ex. résultat d'ACK inconnu).
    BadValue,
}

pub mod opcode {
    pub const PING: u8 = 0x01;
    pub const SET_MODE: u8 = 0x02;
    pub const REBOOT: u8 = 0x03;
    pub const POWER_CYCLE: u8 = 0x04;
    pub const CLEAR_FDIR: u8 = 0x05;
    pub const DUMP_EVENTS: u8 = 0x06;
}

/// Télécommande. Charge utile = opcode puis arguments.
///
/// Les arguments sont gardés bruts : leur validation (mode ou ligne hors bornes -> ACK `BadArg`)
/// relève de l'exécution, pas du décodage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    Ping,
    /// Plafond de mode demandé (valeur de `crate::Mode` en `u8`).
    SetMode(u8),
    Reboot,
    /// Ligne à couper 5 s (valeur de `crate::hal::Line` en `u8`).
    PowerCycle(u8),
    ClearFdir,
    DumpEvents,
}

impl Command {
    /// Taille maximale de la charge utile d'une TC.
    pub const MAX_LEN: usize = 2;

    pub const fn opcode(&self) -> u8 {
        match self {
            Command::Ping => opcode::PING,
            Command::SetMode(_) => opcode::SET_MODE,
            Command::Reboot => opcode::REBOOT,
            Command::PowerCycle(_) => opcode::POWER_CYCLE,
            Command::ClearFdir => opcode::CLEAR_FDIR,
            Command::DumpEvents => opcode::DUMP_EVENTS,
        }
    }

    /// Écrit la charge utile ; renvoie sa longueur (1 ou 2).
    pub fn encode(&self, out: &mut [u8; Command::MAX_LEN]) -> usize {
        out[0] = self.opcode();
        match *self {
            Command::SetMode(a) | Command::PowerCycle(a) => {
                out[1] = a;
                2
            }
            _ => 1,
        }
    }

    /// Décode une charge utile de TC. Longueur exacte exigée.
    pub fn decode(payload: &[u8]) -> Result<Command, PayloadError> {
        let (&op, args) = match payload.split_first() {
            Some(x) => x,
            None => return Err(PayloadError::Empty),
        };
        let cmd = match (op, args) {
            (opcode::PING, []) => Command::Ping,
            (opcode::SET_MODE, [m]) => Command::SetMode(*m),
            (opcode::REBOOT, []) => Command::Reboot,
            (opcode::POWER_CYCLE, [l]) => Command::PowerCycle(*l),
            (opcode::CLEAR_FDIR, []) => Command::ClearFdir,
            (opcode::DUMP_EVENTS, []) => Command::DumpEvents,
            (opcode::PING..=opcode::DUMP_EVENTS, _) => return Err(PayloadError::BadLength),
            (other, _) => return Err(PayloadError::UnknownOpcode(other)),
        };
        Ok(cmd)
    }
}

/// Résultat porté par un ACK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AckResult {
    Ok = 0,
    BadArg = 1,
    Refused = 2,
}

impl AckResult {
    pub const fn from_u8(v: u8) -> Option<AckResult> {
        match v {
            0 => Some(AckResult::Ok),
            1 => Some(AckResult::BadArg),
            2 => Some(AckResult::Refused),
            _ => None,
        }
    }
}

/// ACK : `tc_seq u32, opcode u8, result u8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ack {
    pub tc_seq: u32,
    pub opcode: u8,
    pub result: AckResult,
}

impl Ack {
    pub const LEN: usize = 6;

    pub fn encode(&self) -> [u8; Ack::LEN] {
        let s = self.tc_seq.to_be_bytes();
        [s[0], s[1], s[2], s[3], self.opcode, self.result as u8]
    }

    pub fn decode(p: &[u8]) -> Result<Ack, PayloadError> {
        let p: &[u8; Ack::LEN] = p.try_into().map_err(|_| PayloadError::BadLength)?;
        let result = AckResult::from_u8(p[5]).ok_or(PayloadError::BadValue)?;
        Ok(Ack {
            tc_seq: u32::from_be_bytes([p[0], p[1], p[2], p[3]]),
            opcode: p[4],
            result,
        })
    }
}

/// Balise (28 octets). Champs gardés bruts : le sol doit pouvoir lire une balise émise par un
/// firmware corrompu (mode ou cause de reset hors domaine) sans la rejeter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Beacon {
    pub uptime_s: u32,
    pub boot_count: u16,
    /// `crate::hal::ResetCause` en `u8`.
    pub reset_cause: u8,
    /// `crate::Mode` en `u8`.
    pub mode: u8,
    pub vbatt_mv: u16,
    pub temp_batt_dc: i16,
    pub last_tc_seq: u32,
    pub fdir_flags: u16,
    pub sensor_fault_mask: u16,
    pub line_state: u8,
    pub cnt_ecc_corr: u16,
    pub cnt_tmr_fix: u16,
    pub cnt_tc_rejected: u16,
    pub cnt_latchup: u8,
}

impl Beacon {
    pub const LEN: usize = 28;

    pub fn encode(&self) -> [u8; Beacon::LEN] {
        let mut o = [0u8; Beacon::LEN];
        o[0..4].copy_from_slice(&self.uptime_s.to_be_bytes());
        o[4..6].copy_from_slice(&self.boot_count.to_be_bytes());
        o[6] = self.reset_cause;
        o[7] = self.mode;
        o[8..10].copy_from_slice(&self.vbatt_mv.to_be_bytes());
        o[10..12].copy_from_slice(&self.temp_batt_dc.to_be_bytes());
        o[12..16].copy_from_slice(&self.last_tc_seq.to_be_bytes());
        o[16..18].copy_from_slice(&self.fdir_flags.to_be_bytes());
        o[18..20].copy_from_slice(&self.sensor_fault_mask.to_be_bytes());
        o[20] = self.line_state;
        o[21..23].copy_from_slice(&self.cnt_ecc_corr.to_be_bytes());
        o[23..25].copy_from_slice(&self.cnt_tmr_fix.to_be_bytes());
        o[25..27].copy_from_slice(&self.cnt_tc_rejected.to_be_bytes());
        o[27] = self.cnt_latchup;
        o
    }

    pub fn decode(p: &[u8]) -> Result<Beacon, PayloadError> {
        let p: &[u8; Beacon::LEN] = p.try_into().map_err(|_| PayloadError::BadLength)?;
        let u16_at = |i: usize| u16::from_be_bytes([p[i], p[i + 1]]);
        let u32_at = |i: usize| u32::from_be_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);
        Ok(Beacon {
            uptime_s: u32_at(0),
            boot_count: u16_at(4),
            reset_cause: p[6],
            mode: p[7],
            vbatt_mv: u16_at(8),
            temp_batt_dc: i16::from_be_bytes([p[10], p[11]]),
            last_tc_seq: u32_at(12),
            fdir_flags: u16_at(16),
            sensor_fault_mask: u16_at(18),
            line_state: p[20],
            cnt_ecc_corr: u16_at(21),
            cnt_tmr_fix: u16_at(23),
            cnt_tc_rejected: u16_at(25),
            cnt_latchup: p[27],
        })
    }
}

/// Encode une trame TC.
pub fn encode_tc(seq: u32, cmd: &Command, out: &mut [u8; MAX_FRAME]) -> usize {
    let mut p = [0u8; Command::MAX_LEN];
    let n = cmd.encode(&mut p);
    encode_fixed::<{ Command::MAX_LEN }>(FrameType::Tc, seq, &p[..n], out)
}

/// Encode une trame ACK (`seq` = compteur TM descendant).
pub fn encode_ack(seq: u32, ack: &Ack, out: &mut [u8; MAX_FRAME]) -> usize {
    encode_fixed::<{ Ack::LEN }>(FrameType::Ack, seq, &ack.encode(), out)
}

/// Encode une trame BEACON (`seq` = compteur TM descendant).
pub fn encode_beacon(seq: u32, b: &Beacon, out: &mut [u8; MAX_FRAME]) -> usize {
    encode_fixed::<{ Beacon::LEN }>(FrameType::Beacon, seq, &b.encode(), out)
}

/// Encode une trame EVENTS. Le format des entrées du journal est défini par le module journal
/// (T-011) ; ici la charge utile est opaque.
pub fn encode_events(
    seq: u32,
    payload: &[u8],
    out: &mut [u8; MAX_FRAME],
) -> Result<usize, EncodeError> {
    encode_frame(FrameType::Events, seq, payload, out)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::println;

    /// xorshift64* déterministe pour les tests de corruption. Graine loguée.
    struct TestRng(u64);
    impl TestRng {
        fn new(seed: u64) -> Self {
            TestRng(if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            })
        }
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// Graine : variable d'environnement `AEGIS_TEST_SEED` (décimal), sinon valeur fixe.
    fn test_seed(name: &str) -> u64 {
        let seed = std::env::var("AEGIS_TEST_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0xAE61_2026_1114_0002);
        println!("[{name}] graine = {seed} (rejouer avec AEGIS_TEST_SEED={seed})");
        seed
    }

    fn sample_beacon() -> Beacon {
        Beacon {
            uptime_s: 0x0102_0304,
            boot_count: 0x0506,
            reset_cause: 4,
            mode: 2,
            vbatt_mv: 7560,
            temp_batt_dc: -123,
            last_tc_seq: 0xDEAD_BEEF,
            fdir_flags: 0x8001,
            sensor_fault_mask: 0x01FF,
            line_state: 0b101,
            cnt_ecc_corr: 0xFFFF,
            cnt_tmr_fix: 42,
            cnt_tc_rejected: 7,
            cnt_latchup: 3,
        }
    }

    #[test]
    fn crc32_check_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    /// Vecteurs dorés de `docs/risks.md` (annexe), calculés hors du projet avec `zlib.crc32`.
    /// Recopiés en dur : jamais produits par `proto` (parade R7, bogue symétrique sol/bord).
    #[test]
    fn golden_frames() {
        const G1: [u8; 13] = [
            0xae, 0x61, 0x11, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x33, 0x1b, 0x23, 0xe4,
        ];
        const G2: [u8; 18] = [
            0xae, 0x61, 0x12, 0x00, 0x00, 0x00, 0x07, 0x06, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00,
            0xea, 0x3a, 0xd2, 0x59,
        ];
        const G3: [u8; 12] = [
            0xae, 0x61, 0x11, 0x01, 0x02, 0x03, 0x04, 0x00, 0x88, 0x0b, 0xea, 0xfb,
        ];

        // Encodage générique.
        let mut out = [0u8; MAX_FRAME];
        let n = encode_frame(FrameType::Tc, 1, &[0x01], &mut out).unwrap();
        assert_eq!(&out[..n], &G1[..]);
        let n = encode_frame(FrameType::Ack, 7, &[0, 0, 0, 1, 1, 0], &mut out).unwrap();
        assert_eq!(&out[..n], &G2[..]);
        let n = encode_frame(FrameType::Tc, 0x0102_0304, &[], &mut out).unwrap();
        assert_eq!(&out[..n], &G3[..]);

        // Encodage typé : G1 = TC Ping seq 1 ; G2 = ACK (tc_seq 1, Ping, Ok) en TM seq 7.
        let n = encode_tc(1, &Command::Ping, &mut out);
        assert_eq!(&out[..n], &G1[..]);
        let ack = Ack {
            tc_seq: 1,
            opcode: opcode::PING,
            result: AckResult::Ok,
        };
        let n = encode_ack(7, &ack, &mut out);
        assert_eq!(&out[..n], &G2[..]);

        // Décodage.
        let f = decode_frame(&G1).unwrap();
        assert_eq!((f.ftype, f.seq, f.payload), (FrameType::Tc, 1, &[0x01][..]));
        assert_eq!(Command::decode(f.payload), Ok(Command::Ping));
        let f = decode_frame(&G2).unwrap();
        assert_eq!((f.ftype, f.seq), (FrameType::Ack, 7));
        assert_eq!(Ack::decode(f.payload), Ok(ack));
        let f = decode_frame(&G3).unwrap();
        assert_eq!(
            (f.ftype, f.seq, f.payload),
            (FrameType::Tc, 0x0102_0304, &[][..])
        );
        assert_eq!(Command::decode(f.payload), Err(PayloadError::Empty));
    }

    #[test]
    fn roundtrip_tc_all_commands() {
        let cmds = [
            Command::Ping,
            Command::SetMode(0),
            Command::SetMode(2),
            Command::SetMode(0xFF),
            Command::Reboot,
            Command::PowerCycle(1),
            Command::PowerCycle(9),
            Command::ClearFdir,
            Command::DumpEvents,
        ];
        let mut out = [0u8; MAX_FRAME];
        for (i, c) in cmds.iter().enumerate() {
            let seq = 1000 + i as u32;
            let n = encode_tc(seq, c, &mut out);
            let f = decode_frame(&out[..n]).unwrap();
            assert_eq!(f.ftype, FrameType::Tc);
            assert_eq!(f.seq, seq);
            assert_eq!(Command::decode(f.payload), Ok(*c));
        }
    }

    #[test]
    fn roundtrip_ack() {
        let mut out = [0u8; MAX_FRAME];
        for (i, r) in [AckResult::Ok, AckResult::BadArg, AckResult::Refused]
            .iter()
            .enumerate()
        {
            let ack = Ack {
                tc_seq: u32::MAX - i as u32,
                opcode: opcode::POWER_CYCLE,
                result: *r,
            };
            let n = encode_ack(i as u32, &ack, &mut out);
            assert_eq!(n, frame_len(Ack::LEN));
            let f = decode_frame(&out[..n]).unwrap();
            assert_eq!(f.ftype, FrameType::Ack);
            assert_eq!(Ack::decode(f.payload), Ok(ack));
        }
    }

    #[test]
    fn roundtrip_beacon_28_bytes() {
        let b = sample_beacon();
        let raw = b.encode();
        assert_eq!(raw.len(), 28);
        // Contrôle de disposition indépendant de `decode` : quelques champs à leur décalage.
        assert_eq!(&raw[0..4], &[1, 2, 3, 4]);
        assert_eq!(&raw[4..6], &[5, 6]);
        assert_eq!(&raw[10..12], &(-123i16).to_be_bytes());
        assert_eq!(&raw[12..16], &[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(raw[27], 3);
        let mut out = [0u8; MAX_FRAME];
        let n = encode_beacon(99, &b, &mut out);
        assert_eq!(n, 40);
        let f = decode_frame(&out[..n]).unwrap();
        assert_eq!(
            (f.ftype, f.seq, f.payload.len()),
            (FrameType::Beacon, 99, 28)
        );
        assert_eq!(Beacon::decode(f.payload), Ok(b));
        assert_eq!(Beacon::decode(&raw[..27]), Err(PayloadError::BadLength));
    }

    #[test]
    fn roundtrip_events() {
        let mut payload = [0u8; MAX_PAYLOAD];
        for (i, b) in payload.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37);
        }
        let mut out = [0u8; MAX_FRAME];
        for len in [0, 1, 17, MAX_PAYLOAD] {
            let n = encode_events(5, &payload[..len], &mut out).unwrap();
            assert_eq!(n, frame_len(len));
            let f = decode_frame(&out[..n]).unwrap();
            assert_eq!((f.ftype, f.seq), (FrameType::Events, 5));
            assert_eq!(f.payload, &payload[..len]);
        }
    }

    #[test]
    fn encode_rejects_oversized_payload() {
        let mut out = [0u8; MAX_FRAME];
        let big = [0u8; MAX_PAYLOAD + 1];
        assert_eq!(
            encode_frame(FrameType::Events, 0, &big, &mut out),
            Err(EncodeError::PayloadTooLong)
        );
    }

    /// Basculer n'importe quel bit d'une trame valide doit la faire rejeter.
    #[test]
    fn rejects_every_single_bit_flip() {
        let mut out = [0u8; MAX_FRAME];
        let n = encode_beacon(3, &sample_beacon(), &mut out);
        for bit in 0..n * 8 {
            let mut f = out;
            f[bit / 8] ^= 1 << (bit % 8);
            assert!(decode_frame(&f[..n]).is_err(), "bit {bit} basculé accepté");
        }
        let n = encode_tc(1, &Command::Ping, &mut out);
        for bit in 0..n * 8 {
            let mut f = out;
            f[bit / 8] ^= 1 << (bit % 8);
            assert!(decode_frame(&f[..n]).is_err(), "bit {bit} basculé accepté");
        }
    }

    #[test]
    fn rejection_order_and_reasons() {
        let mut out = [0u8; MAX_FRAME];
        let n = encode_tc(1, &Command::SetMode(1), &mut out);
        let good = out;

        let mut f = good;
        f[0] = 0xAF;
        assert_eq!(decode_frame(&f[..n]), Err(DecodeError::BadSync));
        // SYNC prime sur tout le reste.
        f[2] = 0xFF;
        f[7] = 200;
        assert_eq!(decode_frame(&f[..n]), Err(DecodeError::BadSync));

        // Version 2 : rejet même avec un CRC recalculé.
        let mut f = good;
        f[2] = 0x21;
        let c = crc32(&f[2..n - 4]);
        f[n - 4..n].copy_from_slice(&c.to_be_bytes());
        assert_eq!(decode_frame(&f[..n]), Err(DecodeError::BadType));
        // Types inconnus 0 et 5..=15.
        for t in [0x10u8, 0x15, 0x1F] {
            let mut f = good;
            f[2] = t;
            assert_eq!(decode_frame(&f[..n]), Err(DecodeError::BadType));
        }

        // LEN > 48, même si la longueur reçue est cohérente.
        let mut big = [0u8; 64];
        big[0..2].copy_from_slice(&SYNC);
        big[2] = FrameType::Events.type_byte();
        big[7] = 49;
        let c = crc32(&big[2..57]);
        big[57..61].copy_from_slice(&c.to_be_bytes());
        assert_eq!(decode_frame(&big[..61]), Err(DecodeError::BadLen));
        for len in [49u8, 100, 255] {
            let mut f = good;
            f[7] = len;
            assert_eq!(decode_frame(&f[..n]), Err(DecodeError::BadLen));
        }

        // CRC.
        let mut f = good;
        f[n - 1] ^= 0x80;
        assert_eq!(decode_frame(&f[..n]), Err(DecodeError::BadCrc));
    }

    #[test]
    fn rejects_truncated_and_padded_frames() {
        let mut out = [0u8; MAX_FRAME];
        let n = encode_beacon(3, &sample_beacon(), &mut out);
        for k in 0..n {
            assert!(
                decode_frame(&out[..k]).is_err(),
                "trame tronquée à {k} acceptée"
            );
        }
        assert_eq!(decode_frame(&out[..n - 1]), Err(DecodeError::BadLen));
        assert_eq!(decode_frame(&out[..n + 1]), Err(DecodeError::BadLen));
        assert_eq!(decode_frame(&[]), Err(DecodeError::BadSync));
        assert_eq!(decode_frame(&SYNC), Err(DecodeError::BadType));
    }

    #[test]
    fn command_decode_errors() {
        assert_eq!(Command::decode(&[]), Err(PayloadError::Empty));
        assert_eq!(
            Command::decode(&[0x00]),
            Err(PayloadError::UnknownOpcode(0))
        );
        assert_eq!(
            Command::decode(&[0x07]),
            Err(PayloadError::UnknownOpcode(7))
        );
        assert_eq!(Command::decode(&[0x01, 0x00]), Err(PayloadError::BadLength));
        assert_eq!(Command::decode(&[0x02]), Err(PayloadError::BadLength));
        assert_eq!(Command::decode(&[0x04, 1, 2]), Err(PayloadError::BadLength));
        assert_eq!(
            Ack::decode(&[0, 0, 0, 1, 1, 3]),
            Err(PayloadError::BadValue)
        );
        assert_eq!(Ack::decode(&[0, 0, 0, 1, 1]), Err(PayloadError::BadLength));
    }

    /// Corruption aléatoire reproductible : 1 à 4 octets altérés, ou longueur modifiée.
    /// Une trame altérée ne doit jamais être acceptée (avec <= 3 bits basculés le CRC-32 est
    /// garanti ; au-delà la probabilité de collision est 2^-32 et la graine fixe rend le test
    /// déterministe).
    #[test]
    fn random_corruption_never_accepted() {
        let seed = test_seed("random_corruption_never_accepted");
        let mut rng = TestRng::new(seed);
        let mut out = [0u8; MAX_FRAME];
        let mut payload = [0u8; MAX_PAYLOAD];
        for iter in 0..20_000 {
            let len = rng.below(MAX_PAYLOAD + 1);
            for b in payload[..len].iter_mut() {
                *b = rng.next() as u8;
            }
            let seq = rng.next() as u32;
            let n = encode_events(seq, &payload[..len], &mut out).unwrap();
            assert_eq!(decode_frame(&out[..n]).unwrap().payload, &payload[..len]);

            let mut f = out;
            let mut m = n;
            match rng.below(3) {
                0 | 1 => {
                    let hits = 1 + rng.below(4);
                    for _ in 0..hits {
                        let pos = rng.below(n);
                        let mask = (1 + rng.below(255)) as u8;
                        f[pos] ^= mask;
                    }
                    if f[..n] == out[..n] {
                        continue; // altérations qui se sont annulées
                    }
                }
                _ => {
                    m = rng.below(MAX_FRAME + 1);
                    if m == n {
                        continue;
                    }
                }
            }
            assert!(
                decode_frame(&f[..m]).is_err(),
                "itération {iter} : trame corrompue acceptée (graine {seed})"
            );
        }
    }

    /// Entrée arbitraire : le décodeur ne panique jamais et toute trame acceptée se ré-encode
    /// à l'identique.
    #[test]
    fn decoder_total_on_random_input() {
        let seed = test_seed("decoder_total_on_random_input");
        let mut rng = TestRng::new(seed);
        let mut buf = [0u8; MAX_FRAME];
        for _ in 0..50_000 {
            let n = rng.below(MAX_FRAME + 1);
            for b in buf[..n].iter_mut() {
                *b = rng.next() as u8;
            }
            // Une fois sur deux, en-tête plausible pour aller plus loin dans la validation.
            if n >= 8 && rng.below(2) == 0 {
                buf[0..2].copy_from_slice(&SYNC);
                buf[2] = 0x10 | (1 + rng.below(4)) as u8;
                buf[7] = n.saturating_sub(12) as u8;
            }
            if let Ok(f) = decode_frame(&buf[..n]) {
                let mut out = [0u8; MAX_FRAME];
                let m = encode_frame(f.ftype, f.seq, f.payload, &mut out).unwrap();
                assert_eq!(&out[..m], &buf[..n]);
            }
        }
    }
}
