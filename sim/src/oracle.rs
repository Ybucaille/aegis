//! Oracle de survie (SPEC §8, S1-S6, E0-E8). Écrit AVANT l'injecteur : la mesure précède ce qu'elle mesure.
//!
//! Sources autorisées, et seulement celles-là : la vérité physique du simulateur (énergie, température,
//! lignes détruites, redémarrages), ce que le sol a reçu (balises, ACK, trames acceptées) et
//! `StepReport.executed_tc`. Aucune lecture de la RAM du firmware.
//!
//! Interprétations fixées dans `docs/decisions.md` (R3) :
//! - S2 : « dernière passe » = dernière passe ENTIÈREMENT contenue dans `[0, T_RUN]` ; aucune passe
//!   complète dans le run -> S2 faux (pas d'interprétation favorable).
//! - S6 : tout redémarrage de l'OBC compte, quelle qu'en soit la cause (coupure matérielle comprise).
//! - S5 : comparaison octet à octet avec le registre des trames réellement émises, sans repasser par
//!   `firmware::proto` (seuls les décalages d'en-tête de SPEC §5 sont lus : SEQ = octets 3..7, opcode =
//!   octet 8).
//!
//! Choix de l'oracle non littéralement dans la SPEC (à citer au rapport) :
//! - E3 `DEAD` est traité comme un échec même si S1-S6 sont vrais (« sans interprétation favorable ») :
//!   OBC alimenté et plus aucun cycle firmware depuis plus de [`DEAD_AFTER_S`] à la fin du run.
//! - `OK_NOMINAL` exige en fin de run : lignes Payload et Radio réellement alimentées (interrupteur en
//!   vigueur, ligne non détruite), batterie et lignes non détruites. L'isolement de la ligne Heater n'est
//!   pas observable de l'extérieur (thermostat) : non vérifié.
//! - E8, mode : « incohérent depuis plus de 60 s » = la dernière balise ET toutes les balises reçues sans
//!   interruption depuis plus de 60 s annoncent un mode incohérent avec la ligne Payload réelle au moment
//!   de leur réception (Payload doit être alimentée si et seulement si le mode annoncé est `Nominal`).
//! - E8, capteur : le bit `sensor_fault_mask` pertinent est celui d'un capteur de la grandeur concernée
//!   (VBattA/B pour la tension, TempBattA/B/Obc pour la température), bit n = `SensorId` n.

use crate::params::Params;
use firmware::proto::{opcode, Ack, AckResult, Beacon};
use firmware::{Line, Mode, ResetCause};
use std::collections::HashSet;

/// OBC alimenté sans cycle firmware depuis plus de N s en fin de run : `DEAD`.
pub const DEAD_AFTER_S: u32 = 60;
/// S4 : plage thermique admise (°C) et durée consécutive maximale hors plage (s).
pub const TEMP_MIN_C: f64 = -10.0;
pub const TEMP_MAX_C: f64 = 50.0;
pub const MAX_OUT_OF_RANGE_S: u32 = 300;
/// S3 : état de charge minimal à `T_RUN` (%).
pub const MIN_FINAL_SOC_PCT: f64 = 20.0;
/// S6 : nombre maximal de resets pendant la dernière orbite.
pub const MAX_RESETS_LAST_ORBIT: u32 = 2;
/// E8 : seuils de mensonge.
pub const LIE_VOLT_MV: f64 = 200.0;
pub const LIE_TEMP_C: f64 = 5.0;
pub const LIE_MODE_S: u32 = 60;
const MASK_VOLT: u16 = 0b0_0000_0011;
const MASK_TEMP: u16 = 0b1_1100_0000;
/// Décalages de l'en-tête de trame (SPEC §5) : `SYNC(2) TYPE(1) SEQ(4) LEN(1) payload...`.
const SEQ_OFF: usize = 3;
const PAYLOAD_OFF: usize = 8;

/// Catégories d'échec, dans l'ordre de priorité de SPEC §8.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    /// E0 : panique ou incohérence du simulateur ; posé par la campagne, jamais par `evaluate`.
    Harness,
    VitalLoss,
    Thermal,
    Dead,
    BootLoop,
    Power,
    CommsLost,
    CmdIntegrity,
    SilentLie,
}

impl Category {
    pub const fn code(self) -> &'static str {
        match self {
            Category::Harness => "E0",
            Category::VitalLoss => "E1",
            Category::Thermal => "E2",
            Category::Dead => "E3",
            Category::BootLoop => "E4",
            Category::Power => "E5",
            Category::CommsLost => "E6",
            Category::CmdIntegrity => "E7",
            Category::SilentLie => "E8",
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Category::Harness => "HARNESS",
            Category::VitalLoss => "VITAL_LOSS",
            Category::Thermal => "THERMAL",
            Category::Dead => "DEAD",
            Category::BootLoop => "BOOT_LOOP",
            Category::Power => "POWER",
            Category::CommsLost => "COMMS_LOST",
            Category::CmdIntegrity => "CMD_INTEGRITY",
            Category::SilentLie => "SILENT_LIE",
        }
    }
}

/// Verdict d'un run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Verdict {
    OkNominal,
    OkDegraded,
    /// Échec ; la catégorie est la principale (première dans l'ordre de SPEC §8.1).
    Fail(Category),
}

impl Verdict {
    pub fn survived(self) -> bool {
        !matches!(self, Verdict::Fail(_))
    }
    pub fn name(self) -> &'static str {
        match self {
            Verdict::OkNominal => "OK_NOMINAL",
            Verdict::OkDegraded => "OK_DEGRADED",
            Verdict::Fail(c) => c.name(),
        }
    }
}

/// Vérité physique que seul le simulateur connaît (alimentée par l'injecteur, T-014).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Truth {
    /// Bit `Line as u8` : ligne détruite (non récupérable).
    pub destroyed_lines: u8,
    pub battery_destroyed: bool,
}

impl Truth {
    pub fn destroy_line(&mut self, l: Line) {
        self.destroyed_lines |= 1 << (l as u8);
    }
    pub fn is_destroyed(&self, l: Line) -> bool {
        self.destroyed_lines & (1 << (l as u8)) != 0
    }
}

/// Résultat complet de l'évaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub verdict: Verdict,
    /// Toutes les catégories applicables, dans l'ordre de priorité.
    pub categories: Vec<Category>,
    /// S1..S6 (index 0..5).
    pub criteria: [bool; 6],
    pub dead: bool,
    /// Descriptions des violations S5 (déboguage, rapport).
    pub s5_violations: Vec<String>,
}

/// État du monde en fin de run, fourni par `World`.
#[derive(Clone, Copy, Debug)]
pub struct EndState {
    /// Nombre de ticks exécutés (= `T_RUN`).
    pub t_end: u32,
    pub soc_pct: f64,
    pub obc_powered: bool,
    /// Bits `LoadSwitches` en vigueur (bit n = ligne n), avant prise en compte des lignes détruites.
    pub switches: u8,
}

/// Observation de fin de tick.
#[derive(Clone, Copy, Debug)]
pub struct TickObs {
    pub temp_c: f64,
    /// Le firmware a terminé un cycle ce tick.
    pub fw_cycle: bool,
    /// La passe continue au tick suivant.
    pub next_in_pass: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct PassStat {
    beacons: u32,
    pings: u32,
}

#[derive(Clone, Debug)]
struct Emitted {
    bytes: Vec<u8>,
    executed: bool,
}

#[derive(Clone, Debug)]
struct Delivered {
    bytes: Vec<u8>,
    consumed: bool,
}

#[derive(Clone, Copy, Debug)]
struct BeaconRec {
    t: u32,
    mode: u8,
    lie: bool,
}

/// Enregistreur d'observations + évaluation.
#[derive(Clone, Debug)]
pub struct Oracle {
    pass_start_s: u32,
    orbit_period_s: u32,
    // passes
    cur_pass: Option<(bool, PassStat)>, // (tronquée au début, compteurs)
    last_complete: Option<PassStat>,
    last_next_in_pass: bool,
    // thermique
    out_run: u32,
    max_out_run: u32,
    // vie du firmware
    last_cycle_t: Option<u32>,
    // S5
    up_emitted: Vec<Emitted>,
    up_delivered: Vec<Delivered>,
    down_emitted: HashSet<Vec<u8>>,
    s5_violations: Vec<String>,
    // balises
    last_beacon: Option<BeaconRec>,
    incons_since: Option<u32>,
}

impl Oracle {
    pub fn new(p: &Params) -> Oracle {
        Oracle {
            pass_start_s: p.pass_start_s,
            orbit_period_s: p.orbit_period_s,
            cur_pass: None,
            last_complete: None,
            last_next_in_pass: false,
            out_run: 0,
            max_out_run: 0,
            last_cycle_t: None,
            up_emitted: Vec::new(),
            up_delivered: Vec::new(),
            down_emitted: HashSet::new(),
            s5_violations: Vec::new(),
            last_beacon: None,
            incons_since: None,
        }
    }

    /// Début de tick : ouvre ou ferme les passes (`in_pass` = état en début de tick).
    pub fn begin_tick(&mut self, t: u32, phase_s: u32, in_pass: bool) {
        match (&self.cur_pass, in_pass) {
            (None, true) => {
                let truncated = t == 0 && phase_s != self.pass_start_s;
                self.cur_pass = Some((truncated, PassStat::default()));
            }
            (Some((trunc, stat)), false) => {
                if !*trunc {
                    self.last_complete = Some(*stat);
                }
                self.cur_pass = None;
            }
            _ => {}
        }
    }

    /// Fin de tick : vérité thermique, activité du firmware.
    pub fn end_tick(&mut self, t: u32, obs: &TickObs) {
        if obs.temp_c < TEMP_MIN_C || obs.temp_c > TEMP_MAX_C {
            self.out_run += 1;
            self.max_out_run = self.max_out_run.max(self.out_run);
        } else {
            self.out_run = 0;
        }
        if obs.fw_cycle {
            self.last_cycle_t = Some(t);
        }
        self.last_next_in_pass = obs.next_in_pass;
    }

    // --- S5 : registre des trames ---

    /// Trame montante émise par le sol.
    pub fn on_up_emitted(&mut self, bytes: &[u8]) {
        self.up_emitted.push(Emitted {
            bytes: bytes.to_vec(),
            executed: false,
        });
    }

    /// Début du lot de trames lues par le firmware pendant le pas courant : vide le lot précédent.
    /// Une TC exécutée n'est rapprochée que des trames de CE lot (S5, T-029).
    pub fn begin_step_reads(&mut self) {
        self.up_delivered.clear();
    }

    /// Trame montante réellement lue par le firmware dans le pas courant (après canal, injection éventuelle).
    pub fn on_up_delivered(&mut self, bytes: &[u8]) {
        self.up_delivered.push(Delivered {
            bytes: bytes.to_vec(),
            consumed: false,
        });
    }

    /// `StepReport.executed_tc` : une TC a été exécutée.
    pub fn on_tc_executed(&mut self, t: u32, seq: u32, op: u8) {
        let seq_b = seq.to_be_bytes();
        // Seules les trames lues dans ce pas sont candidates (`begin_step_reads`) : une trame plus
        // ancienne, rejetée par le firmware, ne peut pas se voir attribuer cette exécution.
        // La plus ancienne trame du lot non consommée portant ce SEQ et cet opcode (FIFO).
        let idx = self.up_delivered.iter().position(|d| {
            !d.consumed
                && d.bytes.len() > PAYLOAD_OFF
                && d.bytes[SEQ_OFF..SEQ_OFF + 4] == seq_b
                && d.bytes[PAYLOAD_OFF] == op
        });
        let Some(i) = idx else {
            self.violate(format!(
                "t={t}: TC seq={seq} op={op:#04x} exécutée sans trame lue dans ce pas portant ce SEQ et cet opcode (inventée ou rejeu)"
            ));
            return;
        };
        self.up_delivered[i].consumed = true;
        let bytes = self.up_delivered[i].bytes.clone();
        let same: Vec<usize> = (0..self.up_emitted.len())
            .filter(|&k| self.up_emitted[k].bytes == bytes)
            .collect();
        if same.is_empty() {
            self.violate(format!(
                "t={t}: TC seq={seq} exécutée depuis une trame qui n'est pas intacte (ne correspond à aucune trame émise par le sol)"
            ));
            return;
        }
        match same.iter().find(|&&k| !self.up_emitted[k].executed) {
            Some(&k) => self.up_emitted[k].executed = true,
            None => self.violate(format!(
                "t={t}: TC seq={seq} exécutée plus d'une fois (rejeu)"
            )),
        }
    }

    /// Trame émise par le satellite (avant canal).
    pub fn on_down_emitted(&mut self, bytes: &[u8]) {
        self.down_emitted.insert(bytes.to_vec());
    }

    /// Trame que le sol a ACCEPTÉE (valide à ses yeux) ; doit être une trame réellement émise.
    pub fn on_down_accepted(&mut self, t: u32, bytes: &[u8]) {
        if !self.down_emitted.contains(bytes) {
            self.violate(format!(
                "t={t}: TM acceptée par le sol mais absente du registre des trames émises (corrompue ou inventée)"
            ));
        }
    }

    fn violate(&mut self, msg: String) {
        self.s5_violations.push(msg);
    }

    // --- vue sol ---

    /// Balise valide reçue ; `true_*` = vérité physique à cet instant.
    pub fn on_beacon(
        &mut self,
        t: u32,
        b: &Beacon,
        true_v_mv: f64,
        true_temp_c: f64,
        payload_on: bool,
    ) {
        if let Some((_, s)) = self.cur_pass.as_mut() {
            s.beacons += 1;
        }
        let volt_lie = (f64::from(b.vbatt_mv) - true_v_mv).abs() > LIE_VOLT_MV
            && b.sensor_fault_mask & MASK_VOLT == 0;
        let temp_lie = (f64::from(b.temp_batt_dc) / 10.0 - true_temp_c).abs() > LIE_TEMP_C
            && b.sensor_fault_mask & MASK_TEMP == 0;
        let announced_nominal = b.mode == Mode::Nominal as u8;
        let incons = announced_nominal != payload_on;
        if incons {
            self.incons_since.get_or_insert(t);
        } else {
            self.incons_since = None;
        }
        let mode_lie = incons && self.incons_since.is_some_and(|s| t - s > LIE_MODE_S);
        self.last_beacon = Some(BeaconRec {
            t,
            mode: b.mode,
            lie: volt_lie || temp_lie || mode_lie,
        });
    }

    /// ACK valide reçu par le sol.
    pub fn on_ack(&mut self, a: &Ack) {
        if a.opcode == opcode::PING && a.result == AckResult::Ok {
            if let Some((_, s)) = self.cur_pass.as_mut() {
                s.pings += 1;
            }
        }
    }

    /// Date de la dernière balise reçue.
    pub fn last_beacon_t(&self) -> Option<u32> {
        self.last_beacon.map(|b| b.t)
    }

    /// Évalue S1-S6 et déduit verdict et catégories.
    pub fn evaluate(&self, truth: &Truth, resets: &[(u32, ResetCause)], end: &EndState) -> Outcome {
        let s1 = !truth.is_destroyed(Line::Radio) && !truth.battery_destroyed;
        // S2 : dernière passe complète ; une passe ouverte est complète si elle finit exactement avec le run.
        let last_pass = match &self.cur_pass {
            Some((false, stat)) if !self.last_next_in_pass => Some(*stat),
            _ => self.last_complete,
        };
        let s2 = last_pass.is_some_and(|p| p.beacons >= 1 && p.pings >= 1);
        let s3 = end.obc_powered && end.soc_pct >= MIN_FINAL_SOC_PCT;
        let s4 = self.max_out_run <= MAX_OUT_OF_RANGE_S;
        let s5 = self.s5_violations.is_empty();
        let from = end.t_end.saturating_sub(self.orbit_period_s);
        let n_resets = resets.iter().filter(|(t, _)| *t >= from).count() as u32;
        let s6 = n_resets <= MAX_RESETS_LAST_ORBIT;
        let criteria = [s1, s2, s3, s4, s5, s6];

        let dead = end.obc_powered
            && self
                .last_cycle_t
                .is_none_or(|c| end.t_end.saturating_sub(c) > DEAD_AFTER_S);

        let mut cats = Vec::new();
        if !s1 {
            cats.push(Category::VitalLoss);
        }
        if !s4 {
            cats.push(Category::Thermal);
        }
        if dead {
            cats.push(Category::Dead);
        }
        if !s6 {
            cats.push(Category::BootLoop);
        }
        if !s3 {
            cats.push(Category::Power);
        }
        if !s2 && !dead {
            // E6 : « firmware actif mais S2 faux ».
            cats.push(Category::CommsLost);
        }
        if !s5 {
            cats.push(Category::CmdIntegrity);
        }
        let survives = criteria.iter().all(|&c| c) && !dead;
        if survives && self.last_beacon.is_some_and(|b| b.lie) {
            cats.push(Category::SilentLie);
        }
        let verdict = if let Some(&c) = cats.first() {
            Verdict::Fail(c)
        } else {
            let payload_on = self.line_alive(Line::Payload, truth, end);
            let radio_on = self.line_alive(Line::Radio, truth, end);
            let nominal = self
                .last_beacon
                .is_some_and(|b| b.mode == Mode::Nominal as u8)
                && payload_on
                && radio_on
                && truth.destroyed_lines == 0;
            if nominal {
                Verdict::OkNominal
            } else {
                Verdict::OkDegraded
            }
        };
        Outcome {
            verdict,
            categories: cats,
            criteria,
            dead,
            s5_violations: self.s5_violations.clone(),
        }
    }

    fn line_alive(&self, l: Line, truth: &Truth, end: &EndState) -> bool {
        end.obc_powered && end.switches & (1 << (l as u8)) != 0 && !truth.is_destroyed(l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{SimConfig, World};
    use firmware::proto;
    use firmware::Hal;
    use ground::{Ground, Policy};

    fn cfg(seed: u64) -> SimConfig {
        SimConfig {
            seed,
            ..SimConfig::default()
        }
    }

    /// Vole `n` ticks ; `before(t, &mut World)` est appelé avant chaque tick.
    fn fly(cfg: SimConfig, mut before: impl FnMut(u32, &mut World)) -> Outcome {
        let n = cfg.duration_s;
        let mut w = World::new(cfg);
        for t in 0..n {
            before(t, &mut w);
            w.step();
        }
        w.finish().outcome
    }

    #[test]
    fn s1_radio_line_destroyed_is_vital_loss() {
        let o = fly(cfg(1), |t, w| {
            if t == 100 {
                w.truth_mut().destroy_line(Line::Radio);
            }
        });
        assert_eq!(o.verdict, Verdict::Fail(Category::VitalLoss));
        assert!(!o.criteria[0]);
    }

    #[test]
    fn s1_battery_destroyed_is_vital_loss() {
        let o = fly(cfg(1), |t, w| {
            if t == 100 {
                w.truth_mut().battery_destroyed = true;
            }
        });
        assert_eq!(o.verdict, Verdict::Fail(Category::VitalLoss));
    }

    #[test]
    fn payload_line_destroyed_is_not_a_vital_loss() {
        // Ligne non vitale : S1 reste vrai. (Le drapeau n'a pas encore d'effet physique avant T-014 :
        // le firmware annonce Nominal avec Payload « morte » -> SILENT_LIE, et c'est bien le bon verdict.)
        let o = fly(cfg(1), |t, w| {
            if t == 0 {
                w.truth_mut().destroy_line(Line::Payload);
            }
        });
        assert!(o.criteria[0]);
        assert!(!o.categories.contains(&Category::VitalLoss));
        assert_ne!(o.verdict, Verdict::OkNominal);
    }

    #[test]
    fn s2_no_ack_in_last_pass_is_comms_lost() {
        let mut c = cfg(1);
        c.params.phase_init_s = Some(0);
        let o = fly(c, |t, w| {
            if t == 0 {
                // Sol muet : aucune TC, donc aucun ACK (les balises arrivent quand même).
                w.replace_ground(Ground::new(Policy::Manual));
            }
        });
        assert!(!o.criteria[1]);
        assert_eq!(o.verdict, Verdict::Fail(Category::CommsLost));
    }

    #[test]
    fn s2_pass_truncated_by_t_run_is_ignored() {
        // Phase 0 : passes complètes [1500,2100) et [7200,7800) -> run de 7201 ticks : la 2e est
        // tronquée (1 tick, aucun ACK possible). Une implémentation qui regarderait la dernière passe
        // même tronquée conclurait à tort COMMS_LOST.
        let mut c = cfg(1);
        c.params.phase_init_s = Some(0);
        c.duration_s = 7201;
        let o = fly(c, |_, _| {});
        assert!(o.criteria[1], "dernière passe complète = [1500,2100)");
        // Et sans aucune passe complète, S2 est faux (pas d'interprétation favorable).
        let mut c = cfg(1);
        c.params.phase_init_s = Some(0);
        c.duration_s = 1700;
        let o = fly(c, |_, _| {});
        assert!(!o.criteria[1]);
    }

    #[test]
    fn s3_final_soc_19_is_power() {
        let n = SimConfig::default().duration_s;
        let o = fly(cfg(1), |t, w| {
            if t == n - 1 {
                w.set_soc_pct(19.0);
            }
        });
        assert!(!o.criteria[2]);
        assert_eq!(o.verdict, Verdict::Fail(Category::Power));
    }

    #[test]
    fn s4_301s_out_of_range_fails_300s_does_not() {
        let hot = |dur: u32| {
            fly(cfg(1), move |t, w| {
                if t < dur {
                    w.set_temp_c(60.0);
                } else if t == dur {
                    w.set_temp_c(10.0);
                }
            })
        };
        let o = hot(301);
        assert!(!o.criteria[3]);
        assert_eq!(o.verdict, Verdict::Fail(Category::Thermal));
        let o = hot(300);
        assert!(o.criteria[3], "300 s exactement : pas d'échec");
        assert!(!o.categories.contains(&Category::Thermal));
    }

    fn frame(seq: u32, op: u8) -> Vec<u8> {
        let mut f = vec![0u8; PAYLOAD_OFF + 3];
        f[SEQ_OFF..SEQ_OFF + 4].copy_from_slice(&seq.to_be_bytes());
        f[PAYLOAD_OFF] = op;
        f
    }

    fn violated(o: &Oracle) -> bool {
        !o.s5_violations.is_empty()
    }

    #[test]
    fn s5_rejected_corrupt_frame_does_not_poison_later_seq() {
        let mut o = Oracle::new(&Params::default());
        let good = frame(17, opcode::REBOOT);
        let corrupt = frame(17, opcode::REBOOT);
        let mut corrupt = corrupt;
        *corrupt.last_mut().unwrap() ^= 1; // différente de la trame émise
        o.on_up_emitted(&good);
        // t=10 : trame corrompue lue puis rejetée (aucune exécution)
        o.begin_step_reads();
        o.on_up_delivered(&corrupt);
        // t=20 : la vraie trame, lue et exécutée une fois
        o.begin_step_reads();
        o.on_up_delivered(&good);
        o.on_tc_executed(20, 17, opcode::REBOOT);
        assert!(!violated(&o), "{:?}", o.s5_violations);
    }

    #[test]
    fn s5_corrupt_frame_really_executed_is_violation() {
        let mut o = Oracle::new(&Params::default());
        let good = frame(17, opcode::REBOOT);
        let mut corrupt = good.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        o.on_up_emitted(&good);
        o.begin_step_reads();
        o.on_up_delivered(&corrupt);
        o.on_tc_executed(10, 17, opcode::REBOOT);
        assert!(violated(&o));
    }

    #[test]
    fn s5_tc_executed_without_frame_read_in_this_step_is_violation() {
        let mut o = Oracle::new(&Params::default());
        let good = frame(17, opcode::REBOOT);
        o.on_up_emitted(&good);
        o.begin_step_reads();
        o.on_up_delivered(&good);
        // pas suivant : rien de lu, mais une exécution déclarée (rejeu/invention)
        o.begin_step_reads();
        o.on_tc_executed(11, 17, opcode::REBOOT);
        assert!(violated(&o));
    }

    #[test]
    fn s5_replayed_frame_executed_twice_is_violation() {
        let mut o = Oracle::new(&Params::default());
        let good = frame(17, opcode::REBOOT);
        o.on_up_emitted(&good);
        for t in [10, 20] {
            o.begin_step_reads();
            o.on_up_delivered(&good);
            o.on_tc_executed(t, 17, opcode::REBOOT);
        }
        assert!(violated(&o));
    }

    #[test]
    fn s5_tc_executed_twice_is_cmd_integrity() {
        let mut w = World::new(cfg(1));
        let n = w.config().duration_s;
        let mut dup = None;
        for _ in 0..n {
            w.step();
            if dup.is_none() {
                dup = w.oracle_first_executed();
                if let Some((t, seq, op)) = dup {
                    w.oracle_mut().on_tc_executed(t, seq, op); // 2e exécution fabriquée
                }
            }
        }
        let o = w.finish().outcome;
        assert!(dup.is_some());
        assert!(!o.criteria[4], "{:?}", o.s5_violations);
        assert_eq!(o.verdict, Verdict::Fail(Category::CmdIntegrity));
    }

    #[test]
    fn s5_tc_never_sent_by_ground_is_cmd_integrity() {
        let mut w = World::new(cfg(1));
        for _ in 0..w.config().duration_s {
            w.step();
        }
        w.oracle_mut().on_tc_executed(5, 424_242, opcode::REBOOT);
        assert_eq!(
            w.finish().outcome.verdict,
            Verdict::Fail(Category::CmdIntegrity)
        );
    }

    #[test]
    fn s5_altered_downlink_accepted_by_ground_is_cmd_integrity() {
        // Trame descendante altérée (balise avec une tension fausse, CRC recalculé) acceptée par un
        // sol factice : le sol l'accepte, l'oracle compare au registre des trames émises.
        let mut w = World::new(cfg(1));
        let n = w.config().duration_s;
        let mut done = false;
        for _ in 0..n {
            w.step();
            if !done {
                if let Some((_, b)) = w.ground().last_beacon {
                    let mut forged = b;
                    forged.vbatt_mv = forged.vbatt_mv.wrapping_add(900);
                    let mut buf = [0u8; firmware::MAX_FRAME];
                    let len = proto::encode_beacon(9999, &forged, &mut buf);
                    w.ground_receive(&buf[..len]);
                    done = true;
                }
            }
        }
        assert!(done);
        let o = w.finish().outcome;
        assert!(!o.criteria[4]);
        assert_eq!(o.verdict, Verdict::Fail(Category::CmdIntegrity));
    }

    #[test]
    fn s6_three_resets_in_last_orbit_is_boot_loop_two_is_not() {
        let n = SimConfig::default().duration_s;
        let resets = |k: u32| {
            fly(cfg(1), move |t, w| {
                // k resets commandés, espacés de 100 s, dans la dernière orbite
                for i in 0..k {
                    if t == n - 3000 + i * 100 {
                        w.hw_mut().sys_reset();
                    }
                }
            })
        };
        let o3 = resets(3);
        assert!(!o3.criteria[5]);
        assert_eq!(o3.verdict, Verdict::Fail(Category::BootLoop));
        let o2 = resets(2);
        assert!(o2.criteria[5], "2 resets : toléré");
    }

    #[test]
    fn s6_counts_resets_whatever_the_cause_but_only_in_last_orbit() {
        let n = SimConfig::default().duration_s;
        // 3 resets, tous dans la 1re orbite : sans effet sur S6.
        let o = fly(cfg(1), |t, w| {
            if [100, 200, 300].contains(&t) {
                w.hw_mut().sys_reset();
            }
        });
        assert!(o.criteria[5]);
        let _ = n;
    }

    #[test]
    fn silent_lie_voltage_201mv_without_flag() {
        let mk = |delta: u16, mask: u16| {
            let mut o = Oracle::new(&Params::default());
            o.begin_tick(0, 1500, true);
            let b = Beacon {
                mode: Mode::Nominal as u8,
                vbatt_mv: (7000 + delta),
                temp_batt_dc: 100,
                sensor_fault_mask: mask,
                ..Beacon::default()
            };
            o.on_beacon(1, &b, 7000.0, 10.0, true);
            o.last_beacon.unwrap().lie
        };
        assert!(mk(201, 0));
        assert!(!mk(200, 0), "200 mV pile : pas un mensonge");
        assert!(!mk(201, 0b01), "drapeau levé : l'écart est signalé");
    }

    #[test]
    fn silent_lie_temperature_and_mode() {
        let mut o = Oracle::new(&Params::default());
        let b = |dc: i16, mode: Mode| Beacon {
            mode: mode as u8,
            vbatt_mv: 7000,
            temp_batt_dc: dc,
            ..Beacon::default()
        };
        o.on_beacon(1, &b(160, Mode::Nominal), 7000.0, 10.0, true);
        assert!(o.last_beacon.unwrap().lie, "6 °C d'écart sans drapeau");
        // Mode incohérent (Nominal annoncé, Payload coupée) : mensonge seulement après 60 s.
        let mut o = Oracle::new(&Params::default());
        o.on_beacon(10, &b(100, Mode::Nominal), 7000.0, 10.0, false);
        assert!(!o.last_beacon.unwrap().lie);
        o.on_beacon(70, &b(100, Mode::Nominal), 7000.0, 10.0, false);
        assert!(!o.last_beacon.unwrap().lie, "60 s pile");
        o.on_beacon(71, &b(100, Mode::Nominal), 7000.0, 10.0, false);
        assert!(o.last_beacon.unwrap().lie);
        o.on_beacon(72, &b(100, Mode::Eco), 7000.0, 10.0, false);
        assert!(!o.last_beacon.unwrap().lie, "cohérent à nouveau");
    }

    #[test]
    fn silent_lie_makes_a_surviving_run_fail() {
        let mut w = World::new(cfg(1));
        for _ in 0..w.config().duration_s {
            w.step();
        }
        // Dernière balise falsifiée par l'oracle lui-même (hors canal) : tension fausse sans drapeau.
        let t = w.oracle_mut().last_beacon_t().unwrap();
        let b = Beacon {
            mode: Mode::Nominal as u8,
            vbatt_mv: 6000,
            temp_batt_dc: 100,
            ..Beacon::default()
        };
        w.oracle_mut().on_beacon(t + 1, &b, 6201.0, 10.0, true);
        assert_eq!(
            w.finish().outcome.verdict,
            Verdict::Fail(Category::SilentLie)
        );
    }

    #[test]
    fn nominal_flight_seeds_1_to_20_is_ok_nominal() {
        for seed in 1..=20u64 {
            let o = fly(cfg(seed), |_, _| {});
            assert_eq!(o.verdict, Verdict::OkNominal, "graine {seed}: {o:?}");
            assert_eq!(o.criteria, [true; 6], "graine {seed}");
        }
    }

    #[test]
    fn category_order_follows_spec() {
        use Category::*;
        let order = [
            Harness,
            VitalLoss,
            Thermal,
            Dead,
            BootLoop,
            Power,
            CommsLost,
            CmdIntegrity,
            SilentLie,
        ];
        for (i, c) in order.iter().enumerate() {
            assert_eq!(c.code(), format!("E{i}"));
        }
        assert!(order.windows(2).all(|w| w[0] < w[1]));
    }
}
