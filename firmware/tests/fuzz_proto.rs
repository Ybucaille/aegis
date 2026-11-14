//! Tests de masse et fuzzing maison du protocole radio (T-017, SPEC §5).
//!
//! Générateur : xoshiro256** local, graine affichée et rejouable (variable `AEGIS_FUZZ_SEED`).
//! Aucune dépendance externe.
//!
//! Encodeur de RÉFÉRENCE (R7) : écrit depuis docs/SPEC.md §5 seule, sans lire `firmware::proto`.
//! CRC-32 bit à bit, sans table. Sert au test différentiel et aux vecteurs dorés.

// ---------------------------------------------------------------------------
// Générateur local (xoshiro256**, initialisé par splitmix64)
// ---------------------------------------------------------------------------

struct Prng {
    s: [u64; 4],
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Prng {
    fn new(seed: u64) -> Prng {
        let mut sm = seed;
        let s = [
            splitmix(&mut sm),
            splitmix(&mut sm),
            splitmix(&mut sm),
            splitmix(&mut sm),
        ];
        Prng { s }
    }

    fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Entier dans [0, n), n > 0.
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    fn byte(&mut self) -> u8 {
        self.next_u64() as u8
    }

    fn fill(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            *b = self.byte();
        }
    }
}

/// Graine de base : variable d'environnement `AEGIS_FUZZ_SEED` (décimal ou 0x hexa), sinon valeur fixe.
fn base_seed() -> u64 {
    match std::env::var("AEGIS_FUZZ_SEED") {
        Ok(v) => {
            let v = v.trim();
            if let Some(hex) = v.strip_prefix("0x") {
                u64::from_str_radix(hex, 16).expect("AEGIS_FUZZ_SEED hexa invalide")
            } else {
                v.parse::<u64>().expect("AEGIS_FUZZ_SEED invalide")
            }
        }
        Err(_) => 0x00AE_6100_0000_0017,
    }
}

// ---------------------------------------------------------------------------
// Encodeur de référence : SPEC §5 uniquement
// ---------------------------------------------------------------------------

const REF_SYNC: [u8; 2] = [0xAE, 0x61];
const REF_MAX_LEN: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefKind {
    Tc = 1,
    Ack = 2,
    Beacon = 3,
    Events = 4,
}

/// CRC-32 IEEE réfléchi (poly 0xEDB88320), init et xor final 0xFFFFFFFF, bit à bit.
fn ref_crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 == 1 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFF_FFFF
}

/// Trame : SYNC, TYPE (version 1 en quartet haut), SEQ u32 gros-boutiste, LEN, charge utile,
/// CRC-32 gros-boutiste calculé sur les octets `[2, 8+LEN)`.
fn ref_encode(kind: RefKind, seq: u32, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= REF_MAX_LEN, "référence : LEN > 48");
    let mut f = Vec::with_capacity(payload.len() + 12);
    f.extend_from_slice(&REF_SYNC);
    f.push((1u8 << 4) | kind as u8);
    f.extend_from_slice(&seq.to_be_bytes());
    f.push(payload.len() as u8);
    f.extend_from_slice(payload);
    let crc = ref_crc32(&f[2..]);
    f.extend_from_slice(&crc.to_be_bytes());
    f
}

// ---------------------------------------------------------------------------
// Vérifications de l'encodeur de référence lui-même
// ---------------------------------------------------------------------------

#[test]
fn ref_crc_check_value() {
    assert_eq!(ref_crc32(b"123456789"), 0xCBF4_3926);
}

/// Vecteurs dorés de docs/risks.md, annexe (calculés avec zlib.crc32, hors du projet).
const GOLDEN: [(RefKind, u32, &[u8], &[u8]); 3] = [
    (
        RefKind::Tc,
        1,
        &[0x01],
        &[
            0xAE, 0x61, 0x11, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x33, 0x1B, 0x23, 0xE4,
        ],
    ),
    (
        RefKind::Ack,
        7,
        &[0x00, 0x00, 0x00, 0x01, 0x01, 0x00],
        &[
            0xAE, 0x61, 0x12, 0x00, 0x00, 0x00, 0x07, 0x06, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00,
            0xEA, 0x3A, 0xD2, 0x59,
        ],
    ),
    (
        RefKind::Tc,
        0x0102_0304,
        &[],
        &[
            0xAE, 0x61, 0x11, 0x01, 0x02, 0x03, 0x04, 0x00, 0x88, 0x0B, 0xEA, 0xFB,
        ],
    ),
];

#[test]
fn ref_golden_vectors() {
    for (i, (kind, seq, payload, expected)) in GOLDEN.iter().enumerate() {
        let got = ref_encode(*kind, *seq, payload);
        assert_eq!(
            &got[..],
            *expected,
            "vecteur doré {i} : référence différente de risks.md"
        );
    }
}

// ---------------------------------------------------------------------------
// Fuzzing : cas 1 à 4 et 6 (le cas 5 est dans ground/tests/fuzz_ground.rs)
// Les trames de base sont produites par l'encodeur de référence, jamais par `proto`.
// ---------------------------------------------------------------------------

use firmware::proto::{self, DecodeError, FrameType};
use firmware::MAX_FRAME;

const ALL_TYPES: [FrameType; 4] = [
    FrameType::Tc,
    FrameType::Ack,
    FrameType::Beacon,
    FrameType::Events,
];

fn ref_kind(t: FrameType) -> RefKind {
    match t {
        FrameType::Tc => RefKind::Tc,
        FrameType::Ack => RefKind::Ack,
        FrameType::Beacon => RefKind::Beacon,
        FrameType::Events => RefKind::Events,
    }
}

/// Trame valide aléatoire produite par la référence.
struct RandFrame {
    ftype: FrameType,
    seq: u32,
    payload: Vec<u8>,
    bytes: Vec<u8>,
}

fn rand_frame(rng: &mut Prng) -> RandFrame {
    let ftype = ALL_TYPES[rng.below(4) as usize];
    let seq = rng.next_u64() as u32;
    let len = rng.below(49) as usize;
    let mut payload = vec![0u8; len];
    rng.fill(&mut payload);
    let bytes = ref_encode(ref_kind(ftype), seq, &payload);
    RandFrame {
        ftype,
        seq,
        payload,
        bytes,
    }
}

fn reason_index(e: DecodeError) -> usize {
    match e {
        DecodeError::BadSync => 0,
        DecodeError::BadType => 1,
        DecodeError::BadLen => 2,
        DecodeError::BadCrc => 3,
    }
}

/// Cas 1 : 1 000 000 tampons aléatoires de 0 à 60 octets ; le décodeur ne panique jamais.
#[test]
fn cas1_tampons_aleatoires_sans_panique() {
    let seed = base_seed();
    let mut rng = Prng::new(seed ^ 0x01);
    let mut buf = [0u8; MAX_FRAME];
    let mut accepted = 0u64;
    for _ in 0..1_000_000u32 {
        let n = rng.below(MAX_FRAME as u64 + 1) as usize;
        rng.fill(&mut buf[..n]);
        if proto::decode_frame(&buf[..n]).is_ok() {
            accepted += 1;
        }
    }
    println!("cas=1 essais=1000000 panique=0 acceptés={accepted} (graine_base={seed}, AEGIS_FUZZ_SEED={seed})");
}

/// Cas 1b (complément) : tampons à SYNC et TYPE plausibles pour atteindre LEN et CRC.
#[test]
fn cas1b_tampons_en_tete_plausible() {
    let seed = base_seed();
    let mut rng = Prng::new(seed ^ 0x1B);
    let mut buf = [0u8; MAX_FRAME];
    let mut accepted = 0u64;
    let mut hist = [0u64; 4];
    for _ in 0..1_000_000u32 {
        let n = rng.below(MAX_FRAME as u64 + 1) as usize;
        rng.fill(&mut buf[..n]);
        if n >= 8 {
            buf[0..2].copy_from_slice(&ref_sync());
            buf[2] = 0x10 | rng.below(6) as u8;
            if rng.below(2) == 0 {
                buf[7] = n.saturating_sub(12) as u8;
            }
        }
        match proto::decode_frame(&buf[..n]) {
            Ok(_) => accepted += 1,
            Err(e) => hist[reason_index(e)] += 1,
        }
    }
    println!(
        "cas=1b essais=1000000 panique=0 acceptés={accepted} rejets: sync={} type={} len={} crc={} (graine_base={seed})",
        hist[0], hist[1], hist[2], hist[3]
    );
}

fn ref_sync() -> [u8; 2] {
    REF_SYNC
}

/// Cas 2 : 1 à 8 bits basculés dans une trame valide. Une trame acceptée au contenu différent
/// est un défaut (collision CRC ou bogue) : elle est comptée, affichée, et fait échouer le test.
#[test]
fn cas2_bits_bascules() {
    let seed = base_seed();
    let mut rng = Prng::new(seed ^ 0x02);
    const TRIALS_PER_K: u64 = 200_000;
    let mut total_bad = 0u64;
    let mut first_bad: Option<(usize, Vec<u8>, Vec<u8>)> = None;
    for k in 1..=8usize {
        let mut essais = 0u64;
        let mut acc_tort = 0u64;
        let mut identiques = 0u64;
        let mut hist = [0u64; 4];
        for _ in 0..TRIALS_PER_K {
            let rf = rand_frame(&mut rng);
            let total_bits = (rf.bytes.len() * 8) as u64;
            let mut pos: Vec<u64> = Vec::with_capacity(k);
            while pos.len() < k {
                let p = rng.below(total_bits);
                if !pos.contains(&p) {
                    pos.push(p);
                }
            }
            let mut f = rf.bytes.clone();
            for &p in &pos {
                f[(p / 8) as usize] ^= 1 << (p % 8);
            }
            essais += 1;
            match proto::decode_frame(&f) {
                Ok(fr) => {
                    if fr.ftype == rf.ftype
                        && fr.seq == rf.seq
                        && fr.payload == rf.payload.as_slice()
                    {
                        identiques += 1;
                    } else {
                        acc_tort += 1;
                        if first_bad.is_none() {
                            first_bad = Some((k, rf.bytes.clone(), f.clone()));
                        }
                    }
                }
                Err(e) => hist[reason_index(e)] += 1,
            }
        }
        total_bad += acc_tort;
        println!(
            "cas=2 k={k} essais={essais} acceptés_à_tort={acc_tort} collisions_crc={acc_tort} identiques={identiques} rejets: sync={} type={} len={} crc={} (graine_base={seed})",
            hist[0], hist[1], hist[2], hist[3]
        );
    }
    if let Some((k, orig, bad)) = &first_bad {
        println!(
            "cas=2 premier cas accepté à tort (k={k}) : original={orig:02x?} altéré={bad:02x?}"
        );
    }
    assert_eq!(
        total_bad, 0,
        "cas 2 : trames acceptées à tort (graine {seed})"
    );
}

/// Cas 3 : toute troncature d'une trame valide est rejetée.
#[test]
fn cas3_troncatures_rejetees() {
    let seed = base_seed();
    let mut rng = Prng::new(seed ^ 0x03);
    let mut essais = 0u64;
    let mut acc = 0u64;
    for _ in 0..20_000 {
        let rf = rand_frame(&mut rng);
        for k in 0..rf.bytes.len() {
            essais += 1;
            if proto::decode_frame(&rf.bytes[..k]).is_ok() {
                acc += 1;
                println!(
                    "cas=3 acceptée : type={:?} tronquée à {k}/{}",
                    rf.ftype,
                    rf.bytes.len()
                );
            }
        }
    }
    println!("cas=3 essais={essais} acceptés_à_tort={acc} (graine_base={seed})");
    assert_eq!(acc, 0, "cas 3 : troncature acceptée (graine {seed})");
}

/// Cas 4 : LEN incohérent. (a) LEN modifié, longueur reçue inchangée ; (b) LEN inchangé,
/// longueur reçue allongée ou raccourcie ; (c) LEN > 48 avec longueur reçue cohérente avec ce LEN.
/// (a) et (c) doivent donner exactement `BadLen` ; (b) doit être rejeté.
#[test]
fn cas4_len_incoherent_rejete() {
    let seed = base_seed();
    let mut rng = Prng::new(seed ^ 0x04);
    let mut essais = 0u64;
    let mut acc = 0u64;
    let mut mauvais_motif = 0u64;
    for _ in 0..50_000 {
        let rf = rand_frame(&mut rng);
        let orig = rf.bytes.clone();
        let orig_len = rf.payload.len();

        // (a) LEN changé, longueur inchangée.
        for _ in 0..4 {
            let mut new_len = rng.below(256) as u8;
            if new_len as usize == orig_len {
                new_len = new_len.wrapping_add(1);
            }
            let mut f = orig.clone();
            f[7] = new_len;
            essais += 1;
            match proto::decode_frame(&f) {
                Ok(_) => acc += 1,
                Err(DecodeError::BadLen) => {}
                Err(_) => mauvais_motif += 1,
            }
        }

        // (b) LEN inchangé, longueur reçue modifiée.
        for _ in 0..4 {
            let mut f = orig.clone();
            let grow = rng.below(2) == 0;
            let delta = 1 + rng.below(8) as usize;
            if grow {
                for _ in 0..delta {
                    f.push(rng.byte());
                }
            } else if delta < f.len() {
                f.truncate(f.len() - delta);
            } else {
                continue;
            }
            essais += 1;
            match proto::decode_frame(&f) {
                Ok(_) => acc += 1,
                Err(e) => {
                    if grow && e != DecodeError::BadLen {
                        mauvais_motif += 1;
                    }
                }
            }
        }

        // (c) LEN > 48, longueur reçue = 12 + LEN, CRC quelconque.
        let big = 49 + rng.below(207) as usize;
        let mut f = orig[..7].to_vec();
        f.push(big as u8);
        for _ in 0..(big + 5) {
            f.push(rng.byte());
        }
        essais += 1;
        match proto::decode_frame(&f) {
            Ok(_) => acc += 1,
            Err(DecodeError::BadLen) => {}
            Err(_) => mauvais_motif += 1,
        }
    }
    println!(
        "cas=4 essais={essais} acceptés_à_tort={acc} motif_inattendu={mauvais_motif} (graine_base={seed})"
    );
    assert_eq!(acc, 0, "cas 4 : LEN incohérent accepté (graine {seed})");
    assert_eq!(
        mauvais_motif, 0,
        "cas 4 : mauvais motif de rejet (graine {seed})"
    );
}

/// Cas 6 : encodeur de référence indépendant. 100 000 trames aléatoires ; octets identiques à
/// `firmware::proto`, et le décodage de `proto` retrouve le contenu. Les trois vecteurs dorés de
/// risks.md sont retrouvés par les deux implémentations.
#[test]
fn cas6_differentiel_encodeur_reference() {
    let seed = base_seed();
    let mut rng = Prng::new(seed ^ 0x06);
    let edge_seq = [0u32, 1, u32::MAX, 0x0102_0304];
    let mut divergences = 0u64;
    let mut premiere: Option<String> = None;
    let note = |msg: String, premiere: &mut Option<String>, div: &mut u64| {
        *div += 1;
        if premiere.is_none() {
            *premiere = Some(msg);
        }
    };
    for i in 0..100_000u32 {
        let ftype = ALL_TYPES[rng.below(4) as usize];
        let seq = if i % 4 == 0 {
            edge_seq[rng.below(4) as usize]
        } else {
            rng.next_u64() as u32
        };
        let len = rng.below(49) as usize;
        let mut payload = vec![0u8; len];
        rng.fill(&mut payload);

        let reference = ref_encode(ref_kind(ftype), seq, &payload);
        let mut out = [0u8; MAX_FRAME];
        let n = proto::encode_frame(ftype, seq, &payload, &mut out).expect("LEN <= 48");
        if out[..n] != reference[..] {
            note(
                format!("encodage : type={ftype:?} seq={seq} payload={payload:02x?} ref={reference:02x?} proto={:02x?}", &out[..n]),
                &mut premiere,
                &mut divergences,
            );
        }
        match proto::decode_frame(&reference) {
            Ok(fr) if fr.ftype == ftype && fr.seq == seq && fr.payload == payload.as_slice() => {}
            other => note(
                format!("décodage : type={ftype:?} seq={seq} ref={reference:02x?} proto={other:?}"),
                &mut premiere,
                &mut divergences,
            ),
        }
    }

    // Vecteurs dorés de risks.md : trouvés par les deux côtés.
    let mut dorés_ok = 0;
    for (kind, seq, payload, expected) in GOLDEN.iter() {
        let ftype = match kind {
            RefKind::Tc => FrameType::Tc,
            RefKind::Ack => FrameType::Ack,
            RefKind::Beacon => FrameType::Beacon,
            RefKind::Events => FrameType::Events,
        };
        let mut out = [0u8; MAX_FRAME];
        let n = proto::encode_frame(ftype, *seq, payload, &mut out).unwrap();
        if &out[..n] == *expected {
            dorés_ok += 1;
        }
    }

    println!(
        "cas=6 essais=100000 divergences={divergences} vecteurs_dorés_proto={dorés_ok}/3 (graine_base={seed})"
    );
    if let Some(m) = &premiere {
        println!("cas=6 première divergence : {m}");
    }
    assert_eq!(
        divergences, 0,
        "cas 6 : écart référence / proto (graine {seed})"
    );
    assert_eq!(dorés_ok, 3, "cas 6 : vecteur doré non retrouvé par proto");
}
