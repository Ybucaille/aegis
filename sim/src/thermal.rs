//! Thermique : un nœud (batterie), relaxation vers l'environnement + élévation due au réchauffeur.
//! `T_eq = T_env + R * P_heater` ; `dT/dt = (T_eq - T) / tau` (Euler exact par pas : exponentielle).

use crate::params::Params;

#[derive(Clone, Debug)]
pub struct Thermal {
    temp_c: f64,
    tau_s: f64,
    r_k_per_w: f64,
    env_sun: f64,
    env_ecl: f64,
}

impl Thermal {
    pub fn new(p: &Params, temp_init_c: f64) -> Thermal {
        Thermal {
            temp_c: temp_init_c,
            tau_s: p.thermal_tau_s,
            r_k_per_w: p.thermal_r_k_per_w,
            env_sun: p.t_env_sun_c,
            env_ecl: p.t_env_eclipse_c,
        }
    }

    pub fn temp_c(&self) -> f64 {
        self.temp_c
    }

    pub fn step(&mut self, in_eclipse: bool, heater_w: f64, dt_s: f64) {
        let env = if in_eclipse {
            self.env_ecl
        } else {
            self.env_sun
        };
        let t_eq = env + self.r_k_per_w * heater_w;
        let a = (-dt_s / self.tau_s).exp();
        self.temp_c = t_eq + (self.temp_c - t_eq) * a;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orbit::Orbit;
    use crate::power::Power;

    /// Part d'un équilibre soleil, simule `n` orbites, renvoie (T min, T au début d'éclipse).
    fn run_orbits(heater: impl Fn(f64) -> f64, n: u32) -> (f64, f64) {
        let p = Params::default();
        let mut o = Orbit::new(&p, 0);
        let mut th = Thermal::new(&p, p.t_env_sun_c);
        let mut tmin = f64::MAX;
        let mut t_ecl_start = 0.0;
        for _ in 0..n * p.orbit_period_s {
            if o.phase() == p.orbit_period_s - p.eclipse_s {
                t_ecl_start = th.temp_c();
            }
            let h = heater(th.temp_c());
            th.step(o.in_eclipse(), h, 1.0);
            tmin = tmin.min(th.temp_c());
            o.advance(1);
        }
        (tmin, t_ecl_start)
    }

    #[test]
    fn no_heater_below_minus_10_within_1500s_of_eclipse() {
        let p = Params::default();
        let mut o = Orbit::new(&p, 0);
        let mut th = Thermal::new(&p, p.t_env_sun_c);
        let mut t_in_ecl = 0u32;
        let mut crossed = None;
        for _ in 0..p.orbit_period_s * 2 {
            th.step(o.in_eclipse(), 0.0, 1.0);
            if o.in_eclipse() {
                t_in_ecl += 1;
                if th.temp_c() < -10.0 && crossed.is_none() {
                    crossed = Some(t_in_ecl);
                }
            } else {
                t_in_ecl = 0;
            }
            o.advance(1);
        }
        let c = crossed.expect("jamais sous -10");
        println!("sans réchauffeur : -10 °C franchi après {c} s d'éclipse");
        assert!(c < 1500);
    }

    #[test]
    fn permanent_heater_keeps_battery_warm() {
        let p = Params::default();
        let (tmin, _) = run_orbits(|_| p.heater_w, 3);
        println!("réchauffeur permanent : T min = {tmin:.1} °C");
        assert!(tmin >= 0.0);
    }

    /// Thermostat idéal ON < +2, OFF > +8, orbite en charges nominales : affiche le
    /// rapport cyclique et le bilan d'énergie (contrainte de calibration annexe A).
    #[test]
    fn ideal_thermostat_duty_and_energy_balance() {
        let p = Params::default();
        let mut o = Orbit::new(&p, 0);
        let mut th = Thermal::new(&p, p.t_env_sun_c);
        let mut heater_on = false;
        let mut pw = Power::new(&p, 80.0);
        let (mut on_ecl, mut on_tot, mut tmin) = (0u32, 0u32, f64::MAX);
        let (mut e_prod, mut e_cons) = (0.0f64, 0.0f64);
        // 1 orbite de mise en régime, puis 1 orbite mesurée.
        for i in 0..2 * p.orbit_period_s {
            let t = th.temp_c();
            if t < p.thermo_on_c {
                heater_on = true;
            } else if t > p.thermo_off_c {
                heater_on = false;
            }
            let h = if heater_on { p.heater_w } else { 0.0 };
            // charges nominales : OBC + RX + payload + balise 3 W pendant 1 s toutes les 10 s
            let tx = if i % 10 == 0 { p.tx_w } else { 0.0 };
            let load = p.obc_w + p.rx_w + p.payload_w + tx + h;
            let sol = if o.in_eclipse() { 0.0 } else { p.solar_w };
            let measured = i >= p.orbit_period_s;
            if measured {
                e_prod += sol / 3600.0;
                e_cons += load / 3600.0;
                if heater_on {
                    on_tot += 1;
                    if o.in_eclipse() {
                        on_ecl += 1;
                    }
                }
                tmin = tmin.min(t);
            }
            pw.step(sol, load, 1.0);
            th.step(o.in_eclipse(), h, 1.0);
            o.advance(1);
        }
        let duty_ecl = on_ecl as f64 / p.eclipse_s as f64;
        let duty_orbit = on_tot as f64 / p.orbit_period_s as f64;
        println!(
            "thermostat idéal : rapport cyclique éclipse = {:.1} %, orbite = {:.1} % ; T min {:.1} °C",
            100.0 * duty_ecl,
            100.0 * duty_orbit,
            tmin
        );
        println!(
            "bilan orbite nominal : produit {:.2} Wh, consommé {:.2} Wh, net {:+.2} Wh",
            e_prod,
            e_cons,
            e_prod - e_cons
        );
        assert!(tmin >= 0.0, "T min {tmin}");
        assert!(
            e_prod - e_cons > 0.0,
            "bilan négatif : à signaler au manager"
        );
    }
}
