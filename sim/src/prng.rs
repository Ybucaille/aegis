//! PRNG maison : `splitmix64` (dérivation) et `xoshiro256**` (tirages).
//! Aucun appel horloge/OS. Gaussienne par somme de 12 uniformes (arithmétique pure,
//! donc identique sur toutes les plateformes ; queues tronquées à ±6 sigma).

/// Un pas de `splitmix64` : avance l'état et renvoie la sortie.
pub fn splitmix64_next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// `splitmix64` sans état : sert à dériver la graine du run `i` (SPEC §7.1).
pub fn splitmix64(x: u64) -> u64 {
    let mut s = x;
    splitmix64_next(&mut s)
}

/// Générateur `xoshiro256**`.
#[derive(Clone, Debug)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    /// Initialise l'état par 4 sorties successives de `splitmix64`.
    pub fn new(seed: u64) -> Rng {
        let mut sm = seed;
        let s = [
            splitmix64_next(&mut sm),
            splitmix64_next(&mut sm),
            splitmix64_next(&mut sm),
            splitmix64_next(&mut sm),
        ];
        Rng { s }
    }

    pub fn next_u64(&mut self) -> u64 {
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

    /// Uniforme dans [0, 1) (53 bits).
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniforme dans [lo, hi).
    pub fn uniform_range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.uniform()
    }

    /// Entier dans [0, n) (biais modulo négligeable pour n << 2^64). `n` doit être > 0.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// Gaussienne centrée réduite (Irwin-Hall, 12 uniformes).
    pub fn gauss(&mut self) -> f64 {
        let mut s = 0.0;
        for _ in 0..12 {
            s += self.uniform();
        }
        s - 6.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix_reference() {
        // Vecteur publié : graine 0 -> 0xE220A8397B1DCDAF.
        assert_eq!(splitmix64(0), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn xoshiro_reference_vector() {
        let mut r = Rng::new(42);
        let got: Vec<u64> = (0..4).map(|_| r.next_u64()).collect();
        assert_eq!(got, REF_42);
    }

    const REF_42: [u64; 4] = [
        0x1578_0B2E_0C2E_C716,
        0x6104_D986_6D11_3A7E,
        0xAE17_5332_39E4_99A1,
        0xECB8_AD47_03B3_60A1,
    ];

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut c = Rng::new(8);
        assert_ne!(Rng::new(7).next_u64(), c.next_u64());
    }

    #[test]
    fn uniform_and_gauss_stats() {
        let mut r = Rng::new(1);
        let n = 20000;
        let (mut m, mut v) = (0.0, 0.0);
        for _ in 0..n {
            let u = r.uniform();
            assert!((0.0..1.0).contains(&u));
        }
        for _ in 0..n {
            let g = r.gauss();
            m += g;
            v += g * g;
        }
        m /= n as f64;
        v = v / n as f64 - m * m;
        assert!(m.abs() < 0.05, "moyenne {m}");
        assert!((v - 1.0).abs() < 0.05, "variance {v}");
    }
}
