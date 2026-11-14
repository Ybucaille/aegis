//! Monde simulé : physique + matériel + firmware réel. Séquence d'un tick : SPEC §3.1.
//! Le sol (`ground::Ground`, politique Auto) est branché sur le canal radio (étapes 5 et 6).

use crate::fault::{flip_frame_bits, truncate_frame, Fault, StuckKind, OBC_CUT_S, SEL_CLEAR_OFF_S};
use crate::hw::{Hw, SensorFault};
use crate::metrics;
pub use crate::metrics::{InjectionRecord, RunResult};
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

/// Vrai si le binaire est compilé avec les protections du firmware (pas de feature `unprotected`).
pub const PROTECTED_BUILD: bool = cfg!(not(feature = "unprotected"));

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
    /// Doit correspondre au binaire compilé (`PROTECTED_BUILD`) : `World::try_new` refuse sinon.
    pub protections: bool,
}

impl SimConfig {
    /// Erreur explicite si la configuration demande autre chose que ce que le binaire contient.
    pub fn check_protections(&self) -> Result<(), String> {
        if self.protections == PROTECTED_BUILD {
            Ok(())
        } else {
            Err(format!(
                "SimConfig.protections={} mais le binaire est compilé {} (feature cargo `unprotected` \
                 = {}) : recompiler avec/sans `--features sim/unprotected`, pas de bascule silencieuse",
                self.protections,
                if PROTECTED_BUILD { "AVEC protections" } else { "SANS protections" },
                !PROTECTED_BUILD
            ))
        }
    }
}

impl Default for SimConfig {
    fn default() -> Self {
        let params = Params::default();
        SimConfig {
            seed: 1,
            duration_s: params.t_run_s,
            faults: Vec::new(),
            params,
            protections: PROTECTED_BUILD,
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
    fw_disabled: bool,
    /// Pannes du run triées par date (copie de `cfg.faults`), et index de la prochaine à appliquer.
    fault_events: Vec<FaultEvent>,
    next_fault: usize,
    inj: Injector,
    injections: Vec<InjectionRecord>,
}

/// Latch-up actif sur une ligne.
#[derive(Clone, Copy, Debug)]
struct Sel {
    extra_ma: u16,
    /// Ticks cumulés ligne alimentée avec la surintensité (ne se remet à zéro qu'à l'effacement).
    on_ticks: u32,
    /// Ticks consécutifs ligne coupée.
    off_ticks: u32,
    rec: usize,
}

/// État d'injection : pannes latentes ou en attente d'une trame.
#[derive(Debug, Default)]
struct Injector {
    sel: [Option<Sel>; 3],
    /// Latch-up tiré mais ligne pas encore alimentée : (ligne, surintensité, enregistrement).
    pending_sel: Vec<(Line, u16, usize)>,
    pending_up: Option<(Fault, usize)>,
    pending_down: Option<(Vec<u32>, usize)>,
    /// Trames montantes émises par le sol (source des rejeux).
    up_history: Vec<Vec<u8>>,
    /// Ticks restants de coupure OBC (F13).
    obc_cut: u32,
}

impl World {
    /// Comme `try_new`, mais panique si la configuration contredit le binaire.
    pub fn new(cfg: SimConfig) -> World {
        World::try_new(cfg).unwrap_or_else(|e| panic!("{e}"))
    }

    /// Crée le monde ; erreur explicite si `cfg.protections` diverge du binaire compilé.
    pub fn try_new(cfg: SimConfig) -> Result<World, String> {
        cfg.check_protections()?;
        Ok(World::build(cfg))
    }

    fn build(cfg: SimConfig) -> World {
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
        let cfg_faults = cfg.faults.clone();
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
            fw_disabled: false,
            fault_events: {
                let mut v = cfg_faults;
                v.sort_by_key(|e| e.t);
                v
            },
            next_fault: 0,
            inj: Injector::default(),
            injections: Vec::new(),
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

    /// Vérité physique en lecture (lignes détruites...).
    pub fn truth(&self) -> &Truth {
        &self.truth
    }

    /// Enregistrements des injections effectivement tirées dans le run (ordre d'application).
    pub fn injections(&self) -> &[InjectionRecord] {
        &self.injections
    }

    /// Surintensité (mA) d'un latch-up actif sur la ligne, s'il y en a un.
    pub fn sel_extra_ma(&self, l: Line) -> Option<u16> {
        self.inj.sel[l as usize].map(|s| s.extra_ma)
    }

    /// Banc d'essai : coupe (`false`) le firmware, le watchdog n'étant alors plus exercé. Sert aux
    /// tests d'effet physique (la mesure ne doit pas dépendre de la réaction du firmware).
    pub fn set_firmware_enabled(&mut self, on: bool) {
        self.fw_disabled = !on;
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
        let destroyed = self.truth.destroyed_lines;
        let sel = self.inj.sel;
        let line_ma = |line: Line, w: f64| -> f64 {
            if sw & line.mask() != 0 && destroyed & (1 << (line as u8)) == 0 {
                w / v * 1_000_000.0 + sel[line as usize].map_or(0.0, |s| s.extra_ma as f64)
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

        // 1. pannes dues à ce tick, puis latch-up latents devenus actifs
        while self.next_fault < self.fault_events.len()
            && self.fault_events[self.next_fault].t <= self.t
        {
            let f = self.fault_events[self.next_fault].fault.clone();
            self.next_fault += 1;
            self.inject(&f);
        }
        self.sel_activate();

        // 2. capteurs (la physique est intégrée en fin de tick, une fois les charges connues)
        if self.power.obc_powered() {
            self.sample_sensors(in_eclipse);
        }

        // 3. firmware
        let mut report = None;
        let fw_on = !self.fw_disabled;
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
                let f = self.shape_downlink(f);
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
            for f in self.shape_uplink(&buf[..n]) {
                self.hw.push_uplink(&f);
            }
        }
        self.summary.tc_sent = self.ground.view().tc_sent;

        // physique : charges effectives (OBC alimenté, lignes selon LoadSwitches en vigueur)
        let sw = self.hw.reg(Reg::LoadSwitches);
        let powered = self.power.obc_powered();
        self.sel_tick(powered, sw);
        let p = &self.cfg.params;
        let destroyed = self.truth.destroyed_lines;
        let on = |l: Line| sw & l.mask() != 0 && destroyed & (1 << (l as u8)) == 0;
        let sel_w: f64 = Line::ALL
            .iter()
            .filter(|l| on(**l))
            .filter_map(|l| self.inj.sel[*l as usize])
            .map(|s| s.extra_ma as f64 * self.power.voltage_mv() / 1_000_000.0)
            .sum();
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
        load_w += sel_w;
        let solar_w = if in_eclipse { 0.0 } else { p.solar_w };
        let tick_s = p.tick_s;
        let dt = tick_s as f64;
        let was_powered = powered;
        if self.inj.obc_cut > 0 {
            // Coupure du limiteur (F13) : rétablie ici, après la capture de `was_powered`, pour que
            // le redémarrage à froid soit détecté comme une remise sous tension.
            self.inj.obc_cut -= 1;
            if self.inj.obc_cut == 0 {
                self.power.set_forced_off(false);
            }
        }
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

    // ----------------------------------------------------------------------------------------
    // Injection de pannes (SPEC §7)
    // ----------------------------------------------------------------------------------------

    fn flip_ram_bit(&mut self, lin: usize) {
        let ram = self.fw.ram_mut();
        ram[lin / 8] ^= 1 << (lin % 8);
    }

    fn flip_reg(&mut self, r: Reg, bit: u8) -> String {
        let before = self.hw.reg(r);
        let after = before ^ (1 << bit);
        self.hw.set_reg(r, after);
        format!("reg={r:?} bit={bit} {before:#06x}->{after:#06x}")
    }

    /// Applique la panne maintenant (appelée au début du tick `t_inj`) et consigne ce qui a été
    /// réellement touché dans `injections()`.
    pub fn inject(&mut self, f: &Fault) {
        let rec = self.injections.len();
        self.injections.push(InjectionRecord {
            class: f.class().label(),
            fault: f.name(),
            params: f.params_text(),
            seed: self.cfg.seed,
            t_inj: self.t,
            applied_t: None,
            touched: String::new(),
            frame_before: None,
            frame_after: None,
        });
        let t = self.t;
        let mut touched: Option<String> = None;
        match f {
            Fault::RamSeu { byte, bit }
            | Fault::FdirHit { byte, bit }
            | Fault::SchedHit { byte, bit } => {
                let lin = *byte as usize * 8 + *bit as usize;
                self.flip_ram_bit(lin);
                let name = firmware::region_of(*byte as usize).map_or("?", |r| r.name);
                self.note_ram_region(name);
                touched = Some(format!("ram byte={byte} bit={bit} region={name}"));
            }
            Fault::RamMbu { byte, bit, n } => {
                let start = *byte as usize * 8 + *bit as usize;
                for k in 0..*n as usize {
                    self.flip_ram_bit(start + k);
                }
                let name = firmware::region_of(*byte as usize).map_or("?", |r| r.name);
                self.note_ram_region(name);
                touched = Some(format!("ram byte={byte} bit={bit} n={n} region={name}"));
            }
            Fault::RegSeu { reg, bit } | Fault::WdgRegHit { reg, bit } => {
                touched = Some(self.flip_reg(*reg, *bit));
            }
            Fault::LatchUp { line, extra_ma } => {
                self.inj.pending_sel.push((*line, *extra_ma, rec));
                self.injections[rec].touched =
                    format!("line={line:?} extra_ma={extra_ma} latent jusqu'à alimentation");
            }
            Fault::SensorStuck { sensor, kind } => {
                let sf = match kind {
                    StuckKind::Frozen => SensorFault::Frozen(self.hw.sensor_raw(*sensor)),
                    StuckKind::Err => SensorFault::Err,
                    StuckKind::OutOfRange(v) => SensorFault::Value(*v),
                };
                self.hw.set_sensor_fault(*sensor, Some(sf));
                touched = Some(format!("sensor={sensor:?} {sf:?}"));
            }
            Fault::TcCorrupt { .. } | Fault::TcReplay { .. } | Fault::TcTruncate { .. } => {
                self.inj.pending_up = Some((f.clone(), rec));
                self.injections[rec].touched = "en attente de la prochaine trame montante".into();
            }
            Fault::TmCorrupt { bits } => {
                self.inj.pending_down = Some((bits.clone(), rec));
                self.injections[rec].touched =
                    "en attente de la prochaine trame descendante reçue".into();
            }
            Fault::WdgSpurious => {
                if self.power.obc_powered() && self.cpu != Cpu::Off {
                    self.do_reset(ResetCause::Watchdog);
                    touched = Some("reset watchdog intempestif".into());
                } else {
                    touched = Some("OBC hors tension : sans effet".into());
                }
            }
            Fault::CpuHang => {
                self.hung = true;
                touched = Some("processeur figé jusqu'au prochain reset".into());
            }
            Fault::BattDeplete { soc_x10 } => {
                let soc = *soc_x10 as f64 / 10.0;
                self.power.set_soc_pct(soc);
                touched = Some(format!("soc forcé à {soc:.1} %"));
            }
            Fault::NvmCorrupt { bits } => {
                let nvm = self.hw.nvm_mut();
                for b in bits {
                    nvm[*b as usize / 8] ^= 1 << (*b % 8);
                }
                touched = Some(format!("nvm bits={bits:?}"));
            }
            Fault::ObcLatchUp => {
                self.power.set_forced_off(true);
                self.inj.obc_cut = OBC_CUT_S + 1; // +1 : le tick d'injection compte déjà
                self.cpu = Cpu::Off;
                self.hung = false;
                touched = Some(format!("OBC coupé {OBC_CUT_S} s par le limiteur"));
            }
            Fault::WdgDead => {
                self.hw.set_wdg_dead(true);
                touched = Some("watchdog mort".into());
            }
        }
        if let Some(txt) = touched {
            let r = &mut self.injections[rec];
            r.touched = txt;
            r.applied_t = Some(t);
        }
    }

    fn line_on(&self, l: Line) -> bool {
        self.power.obc_powered()
            && self.hw.reg(Reg::LoadSwitches) & l.mask() != 0
            && !self.truth.is_destroyed(l)
    }

    /// Un latch-up latent devient actif dès que sa ligne est alimentée.
    fn sel_activate(&mut self) {
        let mut i = 0;
        while i < self.inj.pending_sel.len() {
            let (l, extra, rec) = self.inj.pending_sel[i];
            if self.line_on(l) {
                self.inj.pending_sel.remove(i);
                self.inj.sel[l as usize] = Some(Sel {
                    extra_ma: extra,
                    on_ticks: 0,
                    off_ticks: 0,
                    rec,
                });
                let t = self.t;
                let r = &mut self.injections[rec];
                r.applied_t = Some(t);
                r.touched = format!("line={l:?} extra_ma={extra} actif");
            } else {
                i += 1;
            }
        }
    }

    /// Chronomètres de latch-up : destruction à `T_SEL` de ligne alimentée cumulée, effacement
    /// après `SEL_CLEAR_OFF_S` de coupure.
    fn sel_tick(&mut self, powered: bool, sw: u16) {
        let t_sel = self.cfg.params.t_sel_s;
        for l in Line::ALL {
            let i = l as usize;
            let on = powered && sw & l.mask() != 0 && !self.truth.is_destroyed(l);
            let Some(mut s) = self.inj.sel[i] else {
                continue;
            };
            if on {
                s.on_ticks += 1;
                s.off_ticks = 0;
                if s.on_ticks >= t_sel {
                    self.truth.destroy_line(l);
                    self.hw.set_line_dead(l);
                    self.inj.sel[i] = None;
                    let t = self.t;
                    self.injections[s.rec]
                        .touched
                        .push_str(&format!(" ; ligne détruite t={t}"));
                    continue;
                }
            } else {
                s.off_ticks += 1;
                if s.off_ticks >= SEL_CLEAR_OFF_S {
                    self.inj.sel[i] = None;
                    let t = self.t;
                    self.injections[s.rec]
                        .touched
                        .push_str(&format!(" ; effacé t={t}"));
                    continue;
                }
            }
            self.inj.sel[i] = Some(s);
        }
    }

    /// Canal montant : applique la panne de trame en attente et mémorise la trame émise.
    fn shape_uplink(&mut self, orig: &[u8]) -> Vec<Vec<u8>> {
        let mut out = vec![orig.to_vec()];
        if let Some((fault, rec)) = self.inj.pending_up.take() {
            let t = self.t;
            let r = &mut self.injections[rec];
            r.applied_t = Some(t);
            r.frame_before = Some(orig.to_vec());
            match fault {
                Fault::TcCorrupt { bits } => {
                    let (g, pos) = flip_frame_bits(orig, &bits);
                    r.touched = format!("bits={pos:?} sur {} bits", orig.len() * 8);
                    r.frame_after = Some(g.clone());
                    out = vec![g];
                }
                Fault::TcTruncate { frac } => {
                    let g = truncate_frame(orig, frac);
                    r.touched = format!("trame tronquée {} -> {} octets", orig.len(), g.len());
                    r.frame_after = Some(g.clone());
                    out = vec![g];
                }
                Fault::TcReplay { pick } => {
                    let h = &self.inj.up_history;
                    let old = if h.is_empty() {
                        orig.to_vec()
                    } else {
                        h[pick as usize % h.len()].clone()
                    };
                    r.touched = format!("rejeu d'une trame de {} octets", old.len());
                    r.frame_before = Some(old.clone());
                    r.frame_after = Some(old.clone());
                    out.push(old);
                }
                _ => {}
            }
        }
        if self.inj.up_history.len() < 64 {
            self.inj.up_history.push(orig.to_vec());
        }
        out
    }

    /// Canal descendant : altère la première trame reçue par le sol après `t_inj` (F12).
    fn shape_downlink(&mut self, f: Vec<u8>) -> Vec<u8> {
        let Some((bits, rec)) = self.inj.pending_down.take() else {
            return f;
        };
        let (g, pos) = flip_frame_bits(&f, &bits);
        let t = self.t;
        let r = &mut self.injections[rec];
        r.applied_t = Some(t);
        r.touched = format!("bits={pos:?} sur {} bits", f.len() * 8);
        r.frame_before = Some(f);
        r.frame_after = Some(g.clone());
        g
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
            injections: self.injections,
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
