//! Fuzzing maison de la station sol (T-017, cas 5) : mêmes mutations que `firmware/tests/fuzz_proto.rs`
//! appliquées à des BEACON et ACK valides. Pour chaque trame altérée, la vue `GroundView` (compteurs
//! de rejets exclus) doit rester strictement inchangée, et la trame doit être comptée comme rejetée.
//!
//! Générateur : xoshiro256** local, graine `AEGIS_FUZZ_SEED` (décimal ou 0x hexa), sinon valeur fixe.
//! Les trames de base sont produites par un encodeur de référence local (SPEC §5), pas par `proto`.

use ground::{Ground, Policy};

// ---------------------------------------------------------------------------
// Générateur local
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

    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    fn byte(&mut self) -> u8 {
        self.next_u64() as u8
    }
}

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
// Encodeur de référence (SPEC §5), copie locale
// ---------------------------------------------------------------------------

const REF_SYNC: [u8; 2] = [0xAE, 0x61];

#[derive(Clone, Copy)]
enum RefKind {
    Ack = 2,
    Beacon = 3,
}

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

fn ref_encode(kind: RefKind, seq: u32, payload: &[u8]) -> Vec<u8> {
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

/// BEACON (28 octets, contenu aléatoire : tout motif est un BEACON décodable) ou ACK valide
/// (`result` dans 0..=2, sinon `Ack::decode` rejette la charge utile).
fn base_frame(rng: &mut Prng) -> Vec<u8> {
    let seq = rng.next_u64() as u32;
    if rng.below(2) == 0 {
        let payload: Vec<u8> = (0..28).map(|_| rng.byte()).collect();
        ref_encode(RefKind::Beacon, seq, &payload)
    } else {
        let mut payload = [0u8; 6];
        for b in payload.iter_mut().take(5) {
            *b = rng.byte();
        }
        payload[5] = rng.below(3) as u8;
        ref_encode(RefKind::Ack, seq, &payload)
    }
}

/// Station dans un état non vide : un BEACON et un ACK valides déjà reçus.
fn preloaded(rng: &mut Prng) -> Ground {
    let mut g = Ground::new(Policy::Manual);
    let b = ref_encode(RefKind::Beacon, 1, &[7u8; 28]);
    let a = ref_encode(RefKind::Ack, 2, &[0, 0, 0, 5, 1, 0]);
    g.on_frame(10, &b);
    g.on_frame(11, &a);
    // Une trame aléatoire valide en plus, pour que la vue ait un historique non trivial.
    g.on_frame(12, &base_frame(rng));
    g
}

/// Vue sans les compteurs de rejets, sérialisée : toute différence de données est visible.
fn snap(g: &Ground) -> String {
    let mut v = g.view().clone();
    v.rejects = Default::default();
    format!("{v:?}")
}

fn rejects(g: &Ground) -> u32 {
    g.view().rejects.total()
}

#[derive(Default, Debug)]
struct Tally {
    essais: u64,
    acceptés_à_tort: u64,
    rejetés: u64,
    autre: u64,
}

/// Applique une trame altérée à une station préchargée et classe le résultat.
fn check(rng: &mut Prng, frame: &[u8], tally: &mut Tally, first: &mut Option<String>, tag: &str) {
    let mut g = preloaded(rng);
    let before_snap = snap(&g);
    let before_rej = rejects(&g);
    g.on_frame(100, frame);
    tally.essais += 1;
    if snap(&g) != before_snap {
        tally.acceptés_à_tort += 1;
        if first.is_none() {
            *first = Some(format!("{tag} : trame={frame:02x?}"));
        }
    } else if rejects(&g) == before_rej + 1 {
        tally.rejetés += 1;
    } else {
        tally.autre += 1;
    }
}

/// Cas 5a : 1 à 8 bits basculés sur des BEACON / ACK valides.
#[test]
fn cas5a_bits_bascules_beacon_ack() {
    let seed = base_seed();
    let mut rng = Prng::new(seed ^ 0x05A);
    const TRIALS_PER_K: u64 = 50_000;
    let mut total_bad = 0u64;
    for k in 1..=8usize {
        let mut tally = Tally::default();
        let mut first = None;
        for _ in 0..TRIALS_PER_K {
            let base = base_frame(&mut rng);
            let total_bits = (base.len() * 8) as u64;
            let mut pos: Vec<u64> = Vec::with_capacity(k);
            while pos.len() < k {
                let p = rng.below(total_bits);
                if !pos.contains(&p) {
                    pos.push(p);
                }
            }
            let mut f = base.clone();
            for &p in &pos {
                f[(p / 8) as usize] ^= 1 << (p % 8);
            }
            check(&mut rng, &f, &mut tally, &mut first, "cas5a");
        }
        total_bad += tally.acceptés_à_tort;
        println!(
            "cas=5a k={k} essais={} acceptés_à_tort={} rejetés={} autre={} (graine_base={seed})",
            tally.essais, tally.acceptés_à_tort, tally.rejetés, tally.autre
        );
        if let Some(m) = first {
            println!("cas=5a première vue modifiée à tort : {m}");
        }
    }
    assert_eq!(
        total_bad, 0,
        "cas 5a : vue modifiée par une trame altérée (graine {seed})"
    );
}

/// Cas 5b : toute troncature de BEACON / ACK valides.
#[test]
fn cas5b_troncatures_beacon_ack() {
    let seed = base_seed();
    let mut rng = Prng::new(seed ^ 0x05B);
    let mut tally = Tally::default();
    let mut first = None;
    for _ in 0..10_000 {
        let base = base_frame(&mut rng);
        for k in 0..base.len() {
            check(&mut rng, &base[..k], &mut tally, &mut first, "cas5b");
        }
    }
    println!(
        "cas=5b essais={} acceptés_à_tort={} rejetés={} autre={} (graine_base={seed})",
        tally.essais, tally.acceptés_à_tort, tally.rejetés, tally.autre
    );
    if let Some(m) = first {
        println!("cas=5b première vue modifiée à tort : {m}");
    }
    assert_eq!(
        tally.acceptés_à_tort, 0,
        "cas 5b : troncature acceptée (graine {seed})"
    );
}

/// Cas 5c : LEN incohérent (LEN modifié, longueur reçue modifiée, LEN > 48).
#[test]
fn cas5c_len_incoherent_beacon_ack() {
    let seed = base_seed();
    let mut rng = Prng::new(seed ^ 0x05C);
    let mut tally = Tally::default();
    let mut first = None;
    for _ in 0..20_000 {
        let base = base_frame(&mut rng);
        let orig_len = base[7];
        for _ in 0..3 {
            let mut new_len = rng.byte();
            if new_len == orig_len {
                new_len = new_len.wrapping_add(1);
            }
            let mut f = base.clone();
            f[7] = new_len;
            check(&mut rng, &f, &mut tally, &mut first, "cas5c-LEN");
        }
        for _ in 0..3 {
            let mut f = base.clone();
            let delta = 1 + rng.below(8) as usize;
            if rng.below(2) == 0 {
                for _ in 0..delta {
                    f.push(rng.byte());
                }
            } else if delta < f.len() {
                f.truncate(f.len() - delta);
            } else {
                continue;
            }
            check(&mut rng, &f, &mut tally, &mut first, "cas5c-longueur");
        }
        let big = 49 + rng.below(207) as usize;
        let mut f = base[..7].to_vec();
        f.push(big as u8);
        for _ in 0..(big + 5) {
            f.push(rng.byte());
        }
        check(&mut rng, &f, &mut tally, &mut first, "cas5c-LEN>48");
    }
    println!(
        "cas=5c essais={} acceptés_à_tort={} rejetés={} autre={} (graine_base={seed})",
        tally.essais, tally.acceptés_à_tort, tally.rejetés, tally.autre
    );
    if let Some(m) = first {
        println!("cas=5c première vue modifiée à tort : {m}");
    }
    assert_eq!(
        tally.acceptés_à_tort, 0,
        "cas 5c : LEN incohérent accepté (graine {seed})"
    );
}
