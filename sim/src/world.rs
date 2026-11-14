//! Monde simulé : physique + matériel + firmware réel. Séquence d'un tick : SPEC §3.1.
//! Le sol (`ground::Ground`, politique Auto) est branché sur le canal radio (étapes 5 et 6).

use crate::hw::Hw;
use crate::metrics;
pub use crate::metrics::RunResult;
use crate::oracle::{EndState, Oracle, TickObs, Truth};
use crate::orbit::Orbit;
use crate::params::Params;
use crate::power::Power;
use crate::prng::{splitmix64, Rng};
use crate::sensors::Sensors;
use crate::thermal::Thermal;
use firmware::proto::Command;
use firmware::{Firmware, Line, Mode, Reg, ResetCause, SensorId};
use ground::{Ground, GroundView, Policy};
use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Délai entre un reset et le `boot` du firmware (annexe A : 5 s).
pub const BOOT_DELAY: u32 = 5;
/// Température initiale du nœud batterie (°C). Choix du simulateur, indépendant de la graine.
pub const TEMP_INIT_C: f64 = 10.0;

/// Panne à injecter. Placeholder : les vraies pannes arrivent en T-014 ; `faults` est ignoré ici.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    Placeholder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaultEvent {
    pub t: u32,
    pub fault: Fault,
}

#[derive(Clone, Debug)]
pub struct SimConfig {
    pub seed: u64,
    pub duration_s: u32,
    pub faults: Vec<FaultEvent>,
    pub params: Params,
    /// Non exploité en T-006 (le firmware n'a pas encore de feature `unprotected`).
    pub protections: bool,
}

impl Default for SimConfig {
    fn default() -> Self {
        let params = Params::default();
        SimConfig {
            seed: 1,
            duration_s: params.t_run_s,
            faults: Vec::new(),
            params,
            protections: true,
        }
    }
}

/// État du processeur de bord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cpu {
    Running,
    /// Reset effectué ; `boot` dans N ticks.
    Booting(u32),
    /// OBC non alimenté (coupure sous `SOC_OFF`).
    Off,
}

/// Vue de l'état après un tick (dashboard, tests, déterminisme).
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// Indice du tick qui vient d'être exécuté.
    pub t: u32,
    pub phase_s: u32,
    pub in_eclipse: bool,
    pub in_pass: bool,
    pub soc_pct: f64,
    pub vbatt_mv: f64,
    pub temp_c: f64,
    pub solar_w: f64,
    pub load_w: f64,
    pub obc_powered: bool,
    pub cpu: Cpu,
    pub hung: bool,
    /// Bits de `LoadSwitches` en vigueur (3 lignes).
    pub lines: u8,
    pub mode: Option<Mode>,
    pub kicked: bool,
    pub boot_count: u32,
    pub resets_by_cause: [u32; 6],
    pub last_reset: Option<ResetCause>,
    pub tx_frames: u32,
    pub rx_frames: u32,
    pub tx_lost: u32,
    /// Vue sol : balises valides reçues, ACK reçus, dernière balise (date, mode annoncé, boot_count).
    pub beacons_rx: u32,
    pub acks_rx: u32,
    pub last_beacon: Option<(u32, u8, u16)>,
    /// Trames rejetées par le sol (CRC, format...).
    pub ground_rejects: u32,
    pub tc_sent: u32,
}

/// Trame émise par le satellite et reçue par le sol pendant une passe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownFrame {
    pub t: u32,
    pub bytes: Vec<u8>,
}

/// Résumé de la vue sol (comparable, contrairement à `GroundView`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct GroundSummary {
    /// (t, mode, boot_count) de chaque balise valide reçue (historique complet).
    pub beacons: Vec<(u32, u8, u16)>,
    /// (t, tc_seq, opcode, résultat) de chaque ACK reçu.
    pub acks: Vec<(u32, u32, u8, u8)>,
    pub rejects: u32,
    pub tc_sent: u32,
}

pub struct World {
    cfg: SimConfig,
    fw: Firmware,
    hw: Hw,
    orbit: Orbit,
    power: Power,
    thermal: Thermal,
    sensors: Sensors,
    srng: Rng,
    t: u32,
    cpu: Cpu,
    hung: bool,
    boot_count: u32,
    resets_by_cause: [u32; 6],
    reset_log: Vec<(u32, ResetCause)>,
    last_report: Option<(Mode, bool)>,
    downlink: VecDeque<DownFrame>,
    tx_frames: u32,
    tx_lost: u32,
    min_soc: f64,
    temp_min: f64,
    temp_max: f64,
    ticks_off: u32,
    executed_tcs: Vec<(u32, u32, u8)>,
    last_snapshot: Option<Snapshot>,
    ground: Ground,
    summary: GroundSummary,
    oracle: Oracle,
    truth: Truth,
    ram_region: Option<String>,
    #[cfg(test)]
    fw_disabled: bool,
}

impl World {
    pub fn new(cfg: SimConfig) -> World {
        let mut rng = Rng::new(cfg.seed);
        let soc = cfg.params.draw_soc_init(&mut rng);
        let phase = cfg.params.draw_phase_init(&mut rng);
        let srng = Rng::new(splitmix64(cfg.seed ^ 0x5E45_0125_u64));
        let orbit = Orbit::new(&cfg.params, phase);
        let power = Power::new(&cfg.params, soc);
        let thermal = Thermal::new(&cfg.params, TEMP_INIT_C);
        let sensors = Sensors::new(&cfg.params);
        let mut hw = Hw::new();
        hw.set_link_up(orbit.in_pass());
        let mut fw = Firmware::new();
        fw.boot(&mut hw);
        let min_soc = power.soc_pct();
        let oracle = Oracle::new(&cfg.params);
        let cfg_soc_off = cfg.params.soc_off_pct;
        World {
            cfg,
            fw,
            hw,
            orbit,
            power,
            thermal,
            sensors,
            srng,
            t: 0,
            cpu: if soc >= cfg_soc_off {
                Cpu::Running
            } else {
                Cpu::Off
            },
            hung: false,
            boot_count: 1,
            resets_by_cause: [0; 6],
            reset_log: Vec::new(),
            last_report: None,
            downlink: VecDeque::new(),
            tx_frames: 0,
            tx_lost: 0,
            min_soc,
            temp_min: TEMP_INIT_C,
            temp_max: TEMP_INIT_C,
            ticks_off: 0,
            executed_tcs: Vec::new(),
            last_snapshot: None,
            ground: Ground::new(Policy::Auto),
            summary: GroundSummary::default(),
            oracle,
            truth: Truth::default(),
            ram_region: None,
            #[cfg(test)]
            fw_disabled: false,
        }
    }

    /// Vue de la station sol.
    pub fn ground(&self) -> &GroundView {
        self.ground.view()
    }

    /// TC manuelle (dashboard) : mise en file, émise à la prochaine passe.
    pub fn queue_cmd(&mut self, c: Command) {
        self.ground.queue(c);
    }

    /// Vérité physique du simulateur (lignes détruites...), alimentée par l'injecteur et lue par l'oracle.
    pub fn truth_mut(&mut self) -> &mut Truth {
        &mut self.truth
    }

    /// Mémorise la région RAM touchée par la panne (métrique §8.2).
    pub fn note_ram_region(&mut self, region: &str) {
        self.ram_region = Some(region.to_string());
    }

    #[cfg(test)]
    pub(crate) fn oracle_mut(&mut self) -> &mut Oracle {
        &mut self.oracle
    }

    #[cfg(test)]
    pub(crate) fn oracle_first_executed(&self) -> Option<(u32, u32, u8)> {
        self.executed_tcs.first().copied()
    }

    #[cfg(test)]
    pub(crate) fn replace_ground(&mut self, g: Ground) {
        self.ground = g;
    }

    #[cfg(test)]
    pub(crate) fn set_soc_pct(&mut self, soc: f64) {
        self.power.set_soc_pct(soc);
    }

    #[cfg(test)]
    pub(crate) fn set_temp_c(&mut self, c: f64) {
        self.thermal.set_temp_c(c);
    }

    pub fn hw(&self) -> &Hw {
        &self.hw
    }
    pub fn hw_mut(&mut self) -> &mut Hw {
        &mut self.hw
    }
    pub fn firmware_mut(&mut self) -> &mut Firmware {
        &mut self.fw
    }
    pub fn cpu(&self) -> Cpu {
        self.cpu
    }
    /// Fige le processeur (le sim cesse d'appeler `step`) jusqu'au prochain reset (F9, T-014).
    pub fn set_hung(&mut self, hung: bool) {
        self.hung = hung;
    }
    pub fn time(&self) -> u32 {
        self.t
    }
    pub fn config(&self) -> &SimConfig {
        &self.cfg
    }

    /// Dépose une trame montante (sol -> satellite). Remise au firmware pendant la passe seulement.
    pub fn push_uplink(&mut self, frame: &[u8]) -> bool {
        self.hw.push_uplink(frame)
    }

    /// Récupère les trames descendantes reçues par le sol (émises pendant une passe).
    pub fn drain_downlink(&mut self) -> Vec<DownFrame> {
        self.downlink.drain(..).collect()
    }

    fn cause_index(c: ResetCause) -> usize {
        c as usize
    }

    fn record_reset(&mut self, cause: ResetCause) {
        self.resets_by_cause[Self::cause_index(cause)] += 1;
        self.reset_log.push((self.t, cause));
    }

    /// Reset du processeur : registres par défaut, RAM effacée au `boot`, NVM conservée.
    fn do_reset(&mut self, cause: ResetCause) {
        self.record_reset(cause);
        self.hw.hw_reset(cause);
        self.hung = false;
        self.start_boot_delay();
    }

    fn start_boot_delay(&mut self) {
        if BOOT_DELAY == 0 {
            self.finish_boot();
        } else {
            self.cpu = Cpu::Booting(BOOT_DELAY);
        }
    }

    fn finish_boot(&mut self) {
        self.fw.boot(&mut self.hw);
        self.cpu = Cpu::Running;
        self.boot_count += 1;
    }

    /// Valeurs vraies des 9 capteurs puis bruit, quantification (SPEC §6).
    fn sample_sensors(&mut self, in_eclipse: bool) {
        let p = &self.cfg.params;
        let v = self.power.voltage_mv();
        let t_c = self.thermal.temp_c();
        let sw = self.hw.reg(Reg::LoadSwitches);
        let line_ma = |line: Line, w: f64| -> f64 {
            if sw & line.mask() != 0 {
                w / v * 1_000_000.0
            } else {
                0.0
            }
        };
        let solar_ma = if in_eclipse {
            0.0
        } else {
            p.solar_w / v * 1_000_000.0
        };
        let truth = |id: SensorId| -> (f64, f64, f64) {
            match id {
                SensorId::VBattA | SensorId::VBattB => (v, 0.0, 0.0),
                SensorId::ISolar => (0.0, solar_ma, 0.0),
                SensorId::ILinePayload => (0.0, line_ma(Line::Payload, p.payload_w), 0.0),
                SensorId::ILineRadio => (0.0, line_ma(Line::Radio, p.rx_w), 0.0),
                SensorId::ILineHeater => (0.0, line_ma(Line::Heater, p.heater_w), 0.0),
                // Simplification : la sonde OBC lit la température du nœud batterie unique.
                SensorId::TempBattA | SensorId::TempBattB | SensorId::TempObc => (0.0, 0.0, t_c),
            }
        };
        let mut out = [0i16; 9];
        for id in SensorId::ALL {
            let (tv, ti, tt) = truth(id);
            let r = self.sensors.read(&mut self.srng, tv, ti, tt);
            out[id as usize] = match id {
                SensorId::VBattA | SensorId::VBattB => r.vbat_mv.min(i16::MAX as u16) as i16,
                SensorId::ISolar
                | SensorId::ILinePayload
                | SensorId::ILineRadio
                | SensorId::ILineHeater => r.i_ma.min(i16::MAX as u16) as i16,
                _ => r.temp_dc,
            };
        }
        self.hw.set_sensors(out);
    }

    /// Un tick. Étapes 1 (pannes) et 6 (sol) : non branchées en T-006.
    pub fn step(&mut self) -> Snapshot {
        let in_eclipse = self.orbit.in_eclipse();
        let in_pass = self.orbit.in_pass();
        let mut tx_this_tick = 0u32;
        self.oracle.begin_tick(self.t, self.orbit.phase(), in_pass);

        // 2. capteurs (la physique est intégrée en fin de tick, une fois les charges connues)
        if self.power.obc_powered() {
            self.sample_sensors(in_eclipse);
        }

        // 3. firmware
        let mut report = None;
        #[cfg(test)]
        let fw_on = !self.fw_disabled;
        #[cfg(not(test))]
        let fw_on = true;
        if fw_on && self.power.obc_powered() && self.cpu == Cpu::Running && !self.hung {
            let fw = &mut self.fw;
            let hw = &mut self.hw;
            match catch_unwind(AssertUnwindSafe(|| fw.step(hw))) {
                Ok(r) => report = Some(r),
                Err(_) => self.hw.set_pending_reset(ResetCause::Panic),
            }
        }
        for f in self.hw.drain_delivered() {
            self.oracle.on_up_delivered(&f);
        }
        let fw_cycle = report.is_some();
        if let Some(r) = report {
            self.last_report = Some((r.mode, r.kicked));
            if let Some((seq, op)) = r.executed_tc {
                self.executed_tcs.push((self.t, seq, op));
                self.oracle.on_tc_executed(self.t, seq, op);
            }
        }

        // 4. watchdog et resets en attente
        match self.cpu {
            Cpu::Running if self.power.obc_powered() => {
                if let Some(c) = self.hw.take_pending_reset() {
                    self.do_reset(c);
                } else if fw_on && self.hw.wdg_tick() {
                    self.do_reset(ResetCause::Watchdog);
                }
            }
            Cpu::Booting(n) => {
                if n <= 1 {
                    self.finish_boot();
                } else {
                    self.cpu = Cpu::Booting(n - 1);
                }
            }
            _ => {}
        }

        // 5. canal radio : descente seulement pendant la passe, sans perte
        let tx_power = self.hw.tx_this_tick();
        for f in self.hw.drain_tx() {
            tx_this_tick += 1;
            self.tx_frames += 1;
            self.oracle.on_down_emitted(&f);
            if in_pass {
                self.ground_receive(&f);
                self.downlink.push_back(DownFrame {
                    t: self.t,
                    bytes: f,
                });
            } else {
                self.tx_lost += 1;
            }
        }

        // 6. sol : opérateur automatique, une TC au plus par tick, seulement en passe
        if let Some((buf, n)) = self.ground.poll_tx(self.t, in_pass) {
            self.oracle.on_up_emitted(&buf[..n]);
            self.hw.push_uplink(&buf[..n]);
        }
        self.summary.tc_sent = self.ground.view().tc_sent;

        // physique : charges effectives (OBC alimenté, lignes selon LoadSwitches en vigueur)
        let p = &self.cfg.params;
        let sw = self.hw.reg(Reg::LoadSwitches);
        let on = |l: Line| sw & l.mask() != 0;
        let powered = self.power.obc_powered();
        let mut load_w = p.obc_w;
        let mut heater_w = 0.0;
        if on(Line::Payload) {
            load_w += p.payload_w;
        }
        if on(Line::Radio) {
            load_w += p.rx_w;
        }
        if on(Line::Heater) {
            load_w += p.heater_w;
            heater_w = p.heater_w;
        }
        if tx_power {
            load_w += p.tx_w;
        }
        let solar_w = if in_eclipse { 0.0 } else { p.solar_w };
        let tick_s = p.tick_s;
        let dt = tick_s as f64;
        let was_powered = powered;
        let drawn = self.power.step(solar_w, load_w, dt);
        let now_powered = self.power.obc_powered();
        self.thermal
            .step(in_eclipse, if was_powered { heater_w } else { 0.0 }, dt);
        if was_powered && !now_powered {
            // Coupure matérielle : l'état du processeur est perdu, la NVM reste.
            self.cpu = Cpu::Off;
            self.hung = false;
        } else if !was_powered && now_powered {
            // Redémarrage à froid.
            self.record_reset(ResetCause::Brownout);
            self.hw.hw_reset(ResetCause::Brownout);
            self.start_boot_delay();
        }
        if !now_powered {
            self.ticks_off += 1;
        }

        // 7. mesures brutes
        self.min_soc = self.min_soc.min(self.power.soc_pct());
        self.temp_min = self.temp_min.min(self.thermal.temp_c());
        self.temp_max = self.temp_max.max(self.thermal.temp_c());

        self.orbit.advance(tick_s);
        self.hw.set_link_up(self.orbit.in_pass());
        self.oracle.end_tick(
            self.t,
            &TickObs {
                temp_c: self.thermal.temp_c(),
                fw_cycle,
                next_in_pass: self.orbit.in_pass(),
            },
        );

        let snap = Snapshot {
            t: self.t,
            phase_s: self.orbit.phase(),
            in_eclipse,
            in_pass,
            soc_pct: self.power.soc_pct(),
            vbatt_mv: self.power.voltage_mv(),
            temp_c: self.thermal.temp_c(),
            solar_w,
            load_w: drawn,
            obc_powered: now_powered,
            cpu: self.cpu,
            hung: self.hung,
            lines: (sw & 0b111) as u8,
            mode: self.last_report.map(|r| r.0),
            kicked: self.last_report.map(|r| r.1).unwrap_or(false),
            boot_count: self.boot_count,
            resets_by_cause: self.resets_by_cause,
            last_reset: self.reset_log.last().map(|r| r.1),
            tx_frames: tx_this_tick,
            rx_frames: self.hw.rx_delivered(),
            tx_lost: self.tx_lost,
            beacons_rx: self.summary.beacons.len() as u32,
            acks_rx: self.summary.acks.len() as u32,
            last_beacon: self.summary.beacons.last().copied(),
            ground_rejects: self.summary.rejects,
            tc_sent: self.summary.tc_sent,
        };
        self.t += 1;
        self.last_snapshot = Some(snap.clone());
        snap
    }

    /// Ligne réellement alimentée : interrupteur en vigueur, OBC alimenté, ligne non détruite.
    fn line_live(&self, l: Line) -> bool {
        self.power.obc_powered()
            && self.hw.reg(Reg::LoadSwitches) & l.mask() != 0
            && !self.truth.is_destroyed(l)
    }

    /// Passe la trame au sol et journalise ce qu'elle a produit (au plus 1 trame par tick).
    /// Canal : point d'insertion de l'injecteur (T-014) ; l'oracle compare ce que le sol accepte
    /// au registre des trames réellement émises.
    pub(crate) fn ground_receive(&mut self, bytes: &[u8]) {
        let before = self.ground.view().frames_ok;
        self.ground.on_frame(self.t, bytes);
        let v = self.ground.view();
        if v.frames_ok > before {
            self.oracle.on_down_accepted(self.t, bytes);
            if let Some((t, b)) = v.last_beacon {
                if t == self.t && self.summary.beacons.last().map(|x| x.0) != Some(t) {
                    self.summary.beacons.push((t, b.mode, b.boot_count));
                    self.oracle.on_beacon(
                        t,
                        &b,
                        self.power.voltage_mv(),
                        self.thermal.temp_c(),
                        self.line_live(Line::Payload),
                    );
                }
            }
            if let Some((t, a)) = v.acks.back() {
                if *t == self.t {
                    self.summary
                        .acks
                        .push((*t, a.tc_seq, a.opcode, a.result as u8));
                    self.oracle.on_ack(a);
                }
            }
        }
        self.summary.rejects = self.ground.view().rejects.total();
    }

    /// Fin du run : métriques brutes et verdict de l'oracle.
    pub fn finish(self) -> RunResult {
        let end = EndState {
            t_end: self.t,
            soc_pct: self.power.soc_pct(),
            obc_powered: self.power.obc_powered(),
            switches: (self.hw.reg(Reg::LoadSwitches) & 0b111) as u8,
        };
        let outcome = self.oracle.evaluate(&self.truth, &self.reset_log, &end);
        let inject_t = self.cfg.faults.iter().map(|f| f.t).min();
        let (delay_to_beacon_s, delay_to_nominal_s) = match inject_t {
            Some(t0) => metrics::delays(t0, &self.summary.beacons),
            None => (None, None),
        };
        let last_beacon = self.ground.view().last_beacon.map(|(_, b)| b);
        RunResult {
            seed: self.cfg.seed,
            duration_s: self.t,
            final_snapshot: self.last_snapshot,
            resets_by_cause: self.resets_by_cause,
            reset_log: self.reset_log,
            boot_count: self.boot_count,
            min_soc_pct: self.min_soc,
            temp_min_c: self.temp_min,
            temp_max_c: self.temp_max,
            ticks_obc_off: self.ticks_off,
            tx_frames: self.tx_frames,
            tx_lost: self.tx_lost,
            rx_frames: self.hw.rx_delivered(),
            nvm_writes: self.hw.nvm_writes(),
            executed_tcs: self.executed_tcs,
            ground: self.summary,
            verdict: outcome.verdict,
            outcome,
            inject_t,
            delay_to_beacon_s,
            delay_to_nominal_s,
            last_beacon,
            ram_region: self.ram_region,
        }
    }
}

/// Exécute un run complet de `cfg.duration_s` ticks.
pub fn run(cfg: SimConfig) -> RunResult {
    let n = cfg.duration_s;
    let mut w = World::new(cfg);
    for _ in 0..n {
        w.step();
    }
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use firmware::Hal;

    fn world(phase: u32, soc: f64) -> World {
        let mut cfg = SimConfig::default();
        cfg.params.phase_init_s = Some(phase);
        cfg.params.soc_init_pct = Some(soc);
        World::new(cfg)
    }

    /// Monde sans firmware (`fw_disabled`) : le test pilote lui-même le matériel et lit la
    /// physique, sans que la réécriture de `LoadSwitches` par le firmware réel (SPEC §4.1),
    /// sa balise du 1er cycle ou sa lecture de la file montante ne faussent la mesure.
    fn bench(phase: u32, soc: f64) -> World {
        let mut w = world(phase, soc);
        w.fw_disabled = true;
        w
    }

    #[test]
    fn watchdog_not_kicked_resets_after_10s_then_boots_after_5s() {
        let mut w = world(0, 80.0);
        w.set_hung(true);
        let mut reset_at = None;
        let mut boot_at = None;
        for i in 0..40u32 {
            let s = w.step();
            if reset_at.is_none() && s.resets_by_cause[ResetCause::Watchdog as usize] == 1 {
                reset_at = Some(i);
            }
            if reset_at.is_some() && boot_at.is_none() && s.boot_count == 2 {
                boot_at = Some(i);
            }
        }
        let r = reset_at.expect("pas de reset watchdog");
        assert_eq!(r, 9, "reset à la 10e seconde sans rechargement");
        assert_eq!(boot_at.unwrap() - r, BOOT_DELAY);
        assert_eq!(w.hw_mut().reset_cause(), ResetCause::Watchdog);
        assert!(!w.hung, "le blocage cesse au reset");
        assert_eq!(w.cpu(), Cpu::Running);
    }

    #[test]
    fn nvm_kept_ram_cleared_on_reset() {
        let mut w = world(0, 80.0);
        w.hw_mut().nvm_write(7, &[0xAB, 0xCD]).unwrap();
        w.firmware_mut().ram_mut()[100] = 0xEE;
        w.hw_mut().sys_reset();
        w.step();
        for _ in 0..BOOT_DELAY {
            w.step();
        }
        assert_eq!(w.cpu(), Cpu::Running);
        assert_eq!(w.firmware_mut().ram_mut()[100], 0);
        let mut b = [0u8; 2];
        w.hw_mut().nvm_read(7, &mut b).unwrap();
        assert_eq!(b, [0xAB, 0xCD]);
        assert_eq!(w.hw_mut().reset_cause(), ResetCause::Commanded);
    }

    #[test]
    fn cut_line_reads_zero_current_and_costs_nothing() {
        let mut w = bench(0, 80.0);
        w.hw_mut()
            .reg_write(Reg::LoadSwitches, Line::Radio.mask() | Line::Payload.mask());
        let on = w.step();
        let i_on = w.hw_mut().sensor(SensorId::ILinePayload).unwrap();
        assert!((140..200).contains(&i_on), "i_on = {i_on}");
        assert!((on.load_w - (0.4 + 0.2 + 1.2)).abs() < 1e-9);
        w.hw_mut().reg_write(Reg::LoadSwitches, Line::Radio.mask());
        let off = w.step();
        let i_off = w.hw_mut().sensor(SensorId::ILinePayload).unwrap();
        assert!(i_off < 30, "i_off = {i_off}");
        assert!((off.load_w - 0.6).abs() < 1e-9);
    }

    #[test]
    fn transmit_costs_3w_for_one_second() {
        let mut w = bench(0, 80.0);
        let base = w.step().load_w;
        w.hw_mut().radio_send(&[1, 2, 3]).unwrap();
        let tx = w.step();
        assert!((tx.load_w - base - 3.0).abs() < 1e-9);
        assert_eq!(tx.tx_frames, 1);
        assert!((w.step().load_w - base).abs() < 1e-9);
    }

    #[test]
    fn uplink_delivered_only_during_pass() {
        let mut w = bench(1490, 80.0);
        assert!(w.push_uplink(&[9, 9, 9]));
        let mut buf = [0u8; firmware::MAX_FRAME];
        let mut got_at = None;
        for i in 0..20u32 {
            if w.hw_mut().radio_recv(&mut buf).is_some() && got_at.is_none() {
                got_at = Some(i);
            }
            w.step();
        }
        assert_eq!(
            got_at,
            Some(10),
            "remise au premier tick de passe (phase 1500)"
        );
        // hors passe : jamais remise
        let mut w = bench(2200, 80.0);
        assert!(w.push_uplink(&[1]));
        for _ in 0..1000 {
            assert!(w.hw_mut().radio_recv(&mut buf).is_none());
            w.step();
        }
        // émission hors passe : perdue
        w.hw_mut().radio_send(&[5]).unwrap();
        w.step();
        assert!(w.drain_downlink().is_empty());
        assert_eq!(w.finish().tx_lost, 1);
    }

    #[test]
    fn downlink_delivered_in_pass() {
        let mut w = bench(1500, 80.0);
        w.hw_mut().radio_send(&[7, 7]).unwrap();
        w.step();
        let d = w.drain_downlink();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].bytes, vec![7, 7]);
    }

    #[test]
    fn same_seed_same_final_snapshot() {
        let mk = |seed| SimConfig {
            seed,
            duration_s: 6000,
            ..SimConfig::default()
        };
        let a = run(mk(42));
        let b = run(mk(42));
        assert_eq!(a, b);
        assert!(a.final_snapshot.is_some());
        let c = run(mk(43));
        assert_ne!(a.final_snapshot, c.final_snapshot);
    }

    #[test]
    fn brownout_cuts_obc_and_cold_restarts() {
        // SoC proche de SOC_OFF, éclipse, charges lourdes : coupure puis reprise au soleil.
        let mut w = world(4000, 5.5);
        w.hw_mut().reg_write(Reg::LoadSwitches, 0b111);
        let mut went_off = false;
        let mut restarted = false;
        for _ in 0..12_000 {
            let s = w.step();
            if s.cpu == Cpu::Off {
                went_off = true;
            }
            if went_off && s.obc_powered && s.resets_by_cause[ResetCause::Brownout as usize] == 1 {
                restarted = true;
                break;
            }
        }
        assert!(went_off && restarted);
        // la NVM survit à la coupure, RAM effacée, cause = Brownout
        assert_eq!(w.hw_mut().reset_cause(), ResetCause::Brownout);
    }
}
