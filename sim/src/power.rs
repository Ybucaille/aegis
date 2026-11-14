//! Énergie : bilan borné, tension linéaire, coupure matérielle de l'OBC avec hystérésis.

use crate::params::Params;

#[derive(Clone, Debug)]
pub struct Power {
    energy_wh: f64,
    cap_wh: f64,
    /// Alimentation OBC selon l'état de charge (hystérésis).
    obc_powered: bool,
    /// Coupure forcée de l'OBC par le limiteur matériel (F13), indépendante de la charge.
    forced_off: bool,
    soc_off: f64,
    soc_on: f64,
    v_base: f64,
    v_slope: f64,
}

impl Power {
    pub fn new(p: &Params, soc_init_pct: f64) -> Power {
        let soc = soc_init_pct.clamp(0.0, 100.0);
        Power {
            energy_wh: p.batt_wh * soc / 100.0,
            cap_wh: p.batt_wh,
            obc_powered: soc >= p.soc_off_pct,
            forced_off: false,
            soc_off: p.soc_off_pct,
            soc_on: p.soc_on_pct,
            v_base: p.v_base_mv,
            v_slope: p.v_slope_mv_per_pct,
        }
    }

    pub fn soc_pct(&self) -> f64 {
        100.0 * self.energy_wh / self.cap_wh
    }

    /// Force l'état de charge (injection F10, tests de l'oracle).
    pub fn set_soc_pct(&mut self, soc: f64) {
        self.energy_wh = self.cap_wh * soc.clamp(0.0, 100.0) / 100.0;
    }

    pub fn voltage_mv(&self) -> f64 {
        self.v_base + self.v_slope * self.soc_pct()
    }

    /// OBC alimenté ? Faux sous `SOC_OFF`, redevient vrai au-dessus de `SOC_ON`.
    pub fn obc_powered(&self) -> bool {
        self.obc_powered && !self.forced_off
    }

    /// Coupure (ou rétablissement) de l'OBC par le limiteur de courant matériel (F13).
    pub fn set_forced_off(&mut self, off: bool) {
        self.forced_off = off;
    }

    /// Intègre un tick. `solar_w` : production ; `loads_w` : somme des charges demandées.
    /// OBC coupé : les charges sont nulles (tout est derrière l'OBC / la coupure), le solaire charge.
    /// Renvoie la puissance de charge effectivement consommée (W).
    pub fn step(&mut self, solar_w: f64, loads_w: f64, dt_s: f64) -> f64 {
        let load = if self.obc_powered() { loads_w } else { 0.0 };
        self.energy_wh += (solar_w - load) * dt_s / 3600.0;
        self.energy_wh = self.energy_wh.clamp(0.0, self.cap_wh);
        let soc = self.soc_pct();
        if self.obc_powered && soc < self.soc_off {
            self.obc_powered = false;
        } else if !self.obc_powered && soc > self.soc_on {
            self.obc_powered = true;
        }
        load
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voltage_linear() {
        let p = Params::default();
        assert_eq!(Power::new(&p, 50.0).voltage_mv().round() as i32, 7200);
        assert_eq!(Power::new(&p, 5.0).voltage_mv().round() as i32, 6120);
        assert_eq!(Power::new(&p, 15.0).voltage_mv().round() as i32, 6360);
    }

    #[test]
    fn cutoff_below_5_and_restart_above_15() {
        let p = Params::default();
        let mut pw = Power::new(&p, 6.0);
        assert!(pw.obc_powered());
        // décharge à 3 W sans soleil jusqu'à < 5 %
        while pw.obc_powered() {
            pw.step(0.0, 3.0, 1.0);
            assert!(pw.soc_pct() > 0.0);
        }
        assert!(pw.soc_pct() < 5.0);
        // charge solaire, OBC éteint : pas de reprise avant > 15 %
        let mut ticks = 0;
        while !pw.obc_powered() {
            assert!(pw.soc_pct() <= 15.0 + 0.01);
            pw.step(4.0, 3.0, 1.0);
            ticks += 1;
            assert!(ticks < 10_000);
        }
        assert!(pw.soc_pct() > 15.0);
        // hystérésis : entre 5 et 15 en descendant, reste alimenté
        let mut pw = Power::new(&p, 10.0);
        assert!(pw.obc_powered());
        pw.step(0.0, 0.0, 1.0);
        assert!(pw.obc_powered());
        // à 10 % démarrage OBC éteint ? non : SoC init >= SOC_OFF => alimenté
    }

    #[test]
    fn energy_is_bounded() {
        let p = Params::default();
        let mut pw = Power::new(&p, 99.9);
        for _ in 0..10_000 {
            pw.step(4.0, 0.0, 1.0);
        }
        assert!((pw.soc_pct() - 100.0).abs() < 1e-9);
        for _ in 0..100_000 {
            pw.step(0.0, 5.0, 1.0);
        }
        assert!(pw.soc_pct() >= 0.0);
    }
}
