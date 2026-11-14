//! Capteurs : vérité + bruit gaussien, puis quantification.
//! Un capteur sain varie d'un tick à l'autre (condition pour détecter un capteur figé).

use crate::params::Params;
use crate::prng::Rng;

/// Mesure brute vue par le firmware.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SensorReading {
    pub vbat_mv: u16,
    pub i_ma: u16,
    /// Dixièmes de degré Celsius.
    pub temp_dc: i16,
}

#[derive(Clone, Debug)]
pub struct Sensors {
    noise_mv: f64,
    noise_ma: f64,
    noise_c: f64,
}

impl Sensors {
    pub fn new(p: &Params) -> Sensors {
        Sensors {
            noise_mv: p.noise_mv,
            noise_ma: p.noise_ma,
            noise_c: p.noise_c,
        }
    }

    pub fn read(&self, rng: &mut Rng, v_mv: f64, i_ma: f64, temp_c: f64) -> SensorReading {
        let v = v_mv + self.noise_mv * rng.gauss();
        let i = i_ma + self.noise_ma * rng.gauss();
        let t = temp_c + self.noise_c * rng.gauss();
        SensorReading {
            vbat_mv: v.round().clamp(0.0, 65535.0) as u16,
            i_ma: i.round().clamp(0.0, 65535.0) as u16,
            temp_dc: (t * 10.0).round().clamp(-32768.0, 32767.0) as i16,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noisy_and_unbiased() {
        let p = Params::default();
        let s = Sensors::new(&p);
        let mut r = Rng::new(3);
        let n = 5000;
        let (mut sv, mut distinct, mut prev) = (0.0, 0, None);
        for _ in 0..n {
            let m = s.read(&mut r, 7200.0, 400.0, 10.0);
            sv += m.vbat_mv as f64;
            if prev != Some(m) {
                distinct += 1;
            }
            prev = Some(m);
        }
        assert!((sv / n as f64 - 7200.0).abs() < 1.0);
        assert!(distinct > n * 9 / 10, "capteur sain presque constant");
    }

    #[test]
    fn deterministic_per_seed() {
        let s = Sensors::new(&Params::default());
        let a = s.read(&mut Rng::new(9), 7000.0, 300.0, 5.0);
        let b = s.read(&mut Rng::new(9), 7000.0, 300.0, 5.0);
        assert_eq!(a, b);
    }
}
