//! Orbite : période fixe, éclipse en fin d'orbite, une passe sol par orbite.
//! Phase 0 = début de la phase soleil ; éclipse sur [période - éclipse, période).

use crate::params::Params;

#[derive(Clone, Debug)]
pub struct Orbit {
    period: u32,
    eclipse: u32,
    pass_start: u32,
    pass: u32,
    phase: u32,
}

impl Orbit {
    /// `phase_init_s` : phase au tick 0 (déjà tirée ou surchargée, voir `Params::draw_phase_init`).
    pub fn new(p: &Params, phase_init_s: u32) -> Orbit {
        Orbit {
            period: p.orbit_period_s,
            eclipse: p.eclipse_s,
            pass_start: p.pass_start_s,
            pass: p.pass_s,
            phase: phase_init_s % p.orbit_period_s,
        }
    }

    pub fn phase(&self) -> u32 {
        self.phase
    }

    pub fn in_eclipse(&self) -> bool {
        self.phase >= self.period - self.eclipse
    }

    pub fn in_pass(&self) -> bool {
        self.phase >= self.pass_start && self.phase < self.pass_start + self.pass
    }

    /// Avance d'un tick de `dt` secondes.
    pub fn advance(&mut self, dt: u32) {
        self.phase = (self.phase + dt) % self.period;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eclipse_fraction_and_pass_length() {
        let p = Params::default();
        let mut o = Orbit::new(&p, 123);
        let (mut ecl, mut pass, mut rises) = (0u32, 0u32, 0u32);
        let mut prev = o.in_pass();
        for _ in 0..p.orbit_period_s {
            if o.in_eclipse() {
                ecl += 1;
            }
            if o.in_pass() {
                pass += 1;
                if !prev {
                    rises += 1;
                }
            }
            prev = o.in_pass();
            o.advance(1);
        }
        assert_eq!(ecl, 2100);
        assert_eq!(p.orbit_period_s, 5700);
        assert_eq!(pass, 600);
        assert!(rises <= 1);
    }

    #[test]
    fn phase_override_and_random() {
        let mut p = Params {
            phase_init_s: Some(3700),
            ..Params::default()
        };
        let mut r = crate::prng::Rng::new(1);
        assert_eq!(p.draw_phase_init(&mut r), 3700);
        assert!(Orbit::new(&p, 3700).in_eclipse());
        p.phase_init_s = None;
        let ph = p.draw_phase_init(&mut r);
        assert!(ph < 5700);
    }

    #[test]
    fn pass_does_not_overlap_eclipse() {
        let p = Params::default();
        assert!(p.pass_start_s + p.pass_s <= p.orbit_period_s - p.eclipse_s);
    }
}
