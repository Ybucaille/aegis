//! Paramètres du simulateur = annexe A de `docs/SPEC.md`.
//! Les constantes thermiques (absentes de l'annexe) sont choisies pour satisfaire
//! les contraintes de calibration ; voir le compte rendu de T-003.

#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub tick_s: u32,
    pub orbit_period_s: u32,
    pub eclipse_s: u32,
    pub pass_s: u32,
    /// Début de la passe sol dans l'orbite (s, depuis le début de la phase soleil).
    pub pass_start_s: u32,
    pub t_run_s: u32,
    pub batt_wh: f64,
    pub v_base_mv: f64,
    pub v_slope_mv_per_pct: f64,
    pub soc_init_min_pct: f64,
    pub soc_init_max_pct: f64,
    pub soc_off_pct: f64,
    pub soc_on_pct: f64,
    pub solar_w: f64,
    pub obc_w: f64,
    pub rx_w: f64,
    pub tx_w: f64,
    pub payload_w: f64,
    pub heater_w: f64,
    pub watchdog_s: u32,
    pub boot_delay_s: u32,
    pub sel_min_ma: f64,
    pub sel_max_ma: f64,
    pub sel_margin_ma: f64,
    pub t_sel_s: u32,
    pub thermo_on_c: f64,
    pub thermo_off_c: f64,
    pub thermo_surv_on_c: f64,
    pub thermo_surv_off_c: f64,
    pub noise_mv: f64,
    pub noise_ma: f64,
    pub noise_c: f64,
    pub ram_size: usize,
    pub nvm_size: usize,
    /// Thermique : température d'environnement au soleil / en éclipse (hors réchauffeur).
    pub t_env_sun_c: f64,
    pub t_env_eclipse_c: f64,
    /// Constante de temps du nœud batterie (s).
    pub thermal_tau_s: f64,
    /// Élévation d'équilibre par watt de réchauffeur (K/W).
    pub thermal_r_k_per_w: f64,
    /// Surcharge : SoC initial (absent = tirage dans [min, max]).
    pub soc_init_pct: Option<f64>,
    /// Surcharge : phase orbitale initiale en s (absent = tirage dans [0, période)).
    pub phase_init_s: Option<u32>,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            tick_s: 1,
            orbit_period_s: 5700,
            eclipse_s: 2100,
            pass_s: 600,
            pass_start_s: 1500,
            t_run_s: 17_100,
            batt_wh: 10.0,
            v_base_mv: 6000.0,
            v_slope_mv_per_pct: 24.0,
            soc_init_min_pct: 60.0,
            soc_init_max_pct: 100.0,
            soc_off_pct: 5.0,
            soc_on_pct: 15.0,
            solar_w: 4.0,
            obc_w: 0.4,
            rx_w: 0.2,
            tx_w: 3.0,
            payload_w: 1.2,
            heater_w: 1.5,
            watchdog_s: 10,
            boot_delay_s: 5,
            sel_min_ma: 300.0,
            sel_max_ma: 800.0,
            sel_margin_ma: 150.0,
            t_sel_s: 30,
            thermo_on_c: 2.0,
            thermo_off_c: 8.0,
            thermo_surv_on_c: 0.0,
            thermo_surv_off_c: 5.0,
            noise_mv: 5.0,
            noise_ma: 5.0,
            noise_c: 0.2,
            ram_size: 1024,
            nvm_size: 256,
            t_env_sun_c: 15.0,
            t_env_eclipse_c: -20.0,
            thermal_tau_s: 1000.0,
            thermal_r_k_per_w: 40.0,
            soc_init_pct: None,
            phase_init_s: None,
        }
    }
}

impl Params {
    /// SoC initial effectif : surcharge ou tirage uniforme.
    pub fn draw_soc_init(&self, rng: &mut crate::prng::Rng) -> f64 {
        let drawn = rng.uniform_range(self.soc_init_min_pct, self.soc_init_max_pct);
        self.soc_init_pct.unwrap_or(drawn)
    }

    /// Phase initiale effective : surcharge ou tirage uniforme.
    pub fn draw_phase_init(&self, rng: &mut crate::prng::Rng) -> u32 {
        let drawn = rng.below(self.orbit_period_s as u64) as u32;
        self.phase_init_s.unwrap_or(drawn % self.orbit_period_s)
    }
}
