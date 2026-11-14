//! Tâche 3 `fdir` (critique) : détection, isolation, reprise (SPEC §4.4).
//!
//! | Détection | Réaction |
//! |---|---|
//! | `ILine > seuil(L)` sur `LU_SAMPLES` échantillons consécutifs | ligne coupée `LU_OFF_S` s puis réarmée ; `LU_RECUR_MAX` détections en `LU_WINDOW_S` s -> ligne isolée (Radio : jamais, coupure seulement) |
//! | valeur brute identique `STUCK_S` s alors que le redondant a varié (max - min sur la fenêtre) d'au moins `STUCK_SPAN_*` | capteur invalidé (`SENSOR_INVALID_MASK`, visible dans `sensor_fault_mask`) |
//! | capteur `Err` / hors plage | immédiat, chaque cycle, par `power` (bit dans `sensor_fault_mask`) |
//! | >= `STORM_SHORT_BOOTS` boots consécutifs sans atteindre `STABLE_BOOT_S` s (ce boot compris) | verrou `Survie` jusqu'à `STORM_CLEAR_S` s stables ou TC `ClearFdir` |
//! | vote TMR sans majorité (`TMR_ALARM`) ; état FDIR hors bornes | `sys_reset` (+ état assaini) |
//!
//! Choix d'implémentation (SPEC muette, à consigner) :
//! - « 3 récidives en 600 s » = 3 détections de latch-up sur la même ligne dans une fenêtre de
//!   600 s ouverte à la première ; la 3e isole.
//! - Seuils de courant = courant nominal max (puissance de la charge à 6000 mV) + 150 mA. Le sim ne
//!   reflète pas l'émission dans `ILineRadio` : seuil Radio calculé sur le récepteur seul.
//! - `FDIR_FLAG_CRITICAL` = latch-up Radio récurrent (3 détections dans la fenêtre) : la Radio ne
//!   pouvant être isolée, on bloque la remontée hors de `Survie` (balise 60 s, charge minimale).
//! - Un capteur n'est jamais invalidé pour figeage si son redondant l'est déjà (on ne retire pas
//!   le dernier capteur sur une présomption) ; `ClearFdir` lève aussi ces invalidations.
//! - Persistance (`SHORT_BOOTS`, `LINE_ISOLATED`) : `nvm::request_save` (T-011).
//!
//! Qui surveille le surveillant : tout l'état est en TMR (région `Fdir`), les seuils sont des
//! `const` (ROM), et le battement n'avance qu'en fin de tâche : si `fdir` ne s'exécute pas
//! jusqu'au bout, l'ordonnanceur ne recharge pas le watchdog.
//!
//! Témoin `unprotected` (R8) : UN seul point, la constante `PROTECTED` : la tâche se réduit au
//! battement, `on_boot` ne compte rien.

use crate::hal::{Hal, Line, SensorId};
use crate::mode;
use crate::nvm;
use crate::ram::{self, Ram};
use crate::sched;
use crate::tasks::power;

const PROTECTED: bool = !cfg!(feature = "unprotected");

// --- Seuils (ROM) ------------------------------------------------------------------------------

/// Échantillons consécutifs en surintensité avant coupure.
pub const LU_SAMPLES: u8 = 3;
/// Durée de coupure d'une ligne en latch-up (s). Le latch-up s'efface après 2 s de coupure.
pub const LU_OFF_S: u8 = 5;
/// Détections dans la fenêtre provoquant l'isolement.
pub const LU_RECUR_MAX: u8 = 3;
/// Fenêtre de récidive (s).
pub const LU_WINDOW_S: u16 = 600;
/// Marge au-dessus du courant nominal max (annexe A).
pub const LU_MARGIN_MA: i16 = 150;
/// Seuil de surintensité par ligne (index = `Line as usize`), mA : courant nominal max à 6000 mV
/// (Payload 1,2 W -> 200 mA ; récepteur 0,2 W -> 34 mA ; Heater 1,5 W -> 250 mA) + marge.
pub const LU_THRESHOLD_MA: [i16; 3] = [200 + LU_MARGIN_MA, 34 + LU_MARGIN_MA, 250 + LU_MARGIN_MA];
/// Plage plausible d'un capteur de courant de ligne (mA, bornes incluses) (C1, `docs/triage.md`).
/// Le plus fort courant physique du modèle est 250 + 800 = 1050 mA (Heater nominal + latch-up
/// maximal, annexe A) : la borne haute laisse un facteur ~2. Hors plage ou `Err` : capteur
/// invalide pour le cycle (SPEC §4.4), jamais pris pour une surintensité.
pub const ILINE_RANGE_MA: (i16, i16) = (-100, 2000);
/// Capteur de courant de chaque ligne (index = `Line as usize`).
pub const LINE_SENSOR: [SensorId; 3] = [
    SensorId::ILinePayload,
    SensorId::ILineRadio,
    SensorId::ILineHeater,
];

/// Durée de valeur brute identique avant invalidation (s).
pub const STUCK_S: u16 = 120;
/// Variation minimale du redondant (max - min sur la fenêtre) : 30 mV / 1 °C.
pub const STUCK_SPAN_MV: i16 = 30;
pub const STUCK_SPAN_DC: i16 = 10;

/// Boots consécutifs sans atteindre `STABLE_BOOT_S` (ce boot compris) déclenchant le verrou.
pub const STORM_SHORT_BOOTS: u8 = 3;
/// Uptime au-delà duquel un boot est « long » (s).
pub const STABLE_BOOT_S: u32 = 300;
/// Durée stable levant le verrou de tempête (1 orbite, s).
pub const STORM_CLEAR_S: u16 = 5700;

// --- Drapeaux publiés (bits de `FDIR_FLAGS` en plus de ceux de `ram`) --------------------------

/// Au moins une ligne isolée.
pub const FDIR_FLAG_LINE_ISOLATED: u16 = 1 << 3;
/// Au moins une ligne coupée pour latch-up en ce moment.
pub const FDIR_FLAG_LU_CUT: u16 = 1 << 4;
/// Au moins un capteur invalidé durablement (figé).
pub const FDIR_FLAG_SENSOR_INVALID: u16 = 1 << 5;
/// Bits recalculés à chaque cycle (les autres sont des verrous).
const DERIVED_FLAGS: u16 =
    ram::FDIR_FLAG_CRITICAL | FDIR_FLAG_LINE_ISOLATED | FDIR_FLAG_LU_CUT | FDIR_FLAG_SENSOR_INVALID;

/// Bits de `SENSOR_INVALID_MASK` que le FDIR peut poser.
const STUCK_MASK: u16 = (1 << SensorId::VBattA as u16)
    | (1 << SensorId::VBattB as u16)
    | (1 << SensorId::TempBattA as u16)
    | (1 << SensorId::TempBattB as u16);

// --- Interface ----------------------------------------------------------------------------------

/// TC `ClearFdir` (SPEC §5) : lève isolations, verrou de tempête et invalidations de capteurs,
/// remet à zéro les fenêtres de récidive et le compteur de boots courts. Une coupure de latch-up
/// en cours va à son terme (elle protège la ligne). Persistance demandée.
pub fn clear_fdir(ram: &mut Ram) {
    ram::LINE_ISOLATED.set(ram, 0);
    let f = ram::FDIR_FLAGS.get(ram) & !(ram::FDIR_FLAG_STORM_LOCK | DERIVED_FLAGS);
    ram::FDIR_FLAGS.set(ram, f);
    ram::STORM_STABLE_S.set(ram, 0);
    ram::SHORT_BOOTS.set(ram, 0);
    for l in 0..3 {
        ram::LU_OVER[l].set(ram, 0);
        ram::LU_RECUR[l].set(ram, 0);
        ram::LU_WIN_LEFT[l].set(ram, 0);
    }
    ram::SENSOR_INVALID_MASK.set(ram, 0);
    for s in 0..4 {
        ram::STUCK_CNT[s].set(ram, 0);
    }
    persist(ram);
}

/// Au boot, APRÈS la relecture NVM (qui restaure `SHORT_BOOTS` et `LINE_ISOLATED`) : ce boot
/// compte comme court jusqu'à preuve du contraire ; verrou si le seuil est atteint.
pub fn on_boot(ram: &mut Ram) {
    if !PROTECTED {
        return;
    }
    let n = ram::SHORT_BOOTS.get(ram).saturating_add(1);
    ram::SHORT_BOOTS.set(ram, n);
    if n >= STORM_SHORT_BOOTS {
        let f = ram::FDIR_FLAGS.get(ram) | ram::FDIR_FLAG_STORM_LOCK;
        ram::FDIR_FLAGS.set(ram, f);
        ram::STORM_STABLE_S.set(ram, 0);
    }
    persist(ram);
}

/// Demande d'écriture du bloc persistant (`SHORT_BOOTS`, `LINE_ISOLATED`), servie par
/// `nvm::service` au plus tard au cycle suivant.
fn persist(ram: &mut Ram) {
    nvm::request_save(ram);
}

/// Ligne coupée pour latch-up en ce moment (lu par `mode::commanded_lines`). Témoin
/// `unprotected` : jamais (le FDIR n'existe pas, son état n'a aucun effet).
pub fn line_cut(ram: &mut Ram, l: Line) -> bool {
    PROTECTED && ram::LU_OFF_LEFT[l as usize].get(ram) > 0
}

// --- Tâche --------------------------------------------------------------------------------------

pub fn run<H: Hal>(ram: &mut Ram, hal: &mut H) {
    if PROTECTED {
        check_consistency(ram, hal);
        latchup(ram, hal);
        stuck_sensors(ram, hal);
        storm(ram);
        publish_flags(ram);
        mode::apply_lines(ram, hal);
    }
    let hb = ram::FDIR_HEARTBEAT.get(ram).wrapping_add(1);
    ram::FDIR_HEARTBEAT.set(ram, hb);
    sched::set_flag(ram, sched::FLAG_FDIR);
}

/// Incohérence d'état : vote TMR sans majorité (signalé par `tmr`), ou état FDIR hors de ses
/// bornes (impossible sans corruption de deux copies) -> état assaini puis `sys_reset` (effectif
/// en fin de cycle).
fn check_consistency<H: Hal>(ram: &mut Ram, hal: &mut H) {
    let mut bad = false;
    if ram::TMR_ALARM.get(ram) != 0 {
        ram::TMR_ALARM.set(ram, 0);
        bad = true;
    }
    for l in 0..3 {
        bad |= clamp_u8(ram, &ram::LU_OVER[l], LU_SAMPLES);
        bad |= clamp_u8(ram, &ram::LU_OFF_LEFT[l], LU_OFF_S);
        bad |= clamp_u8(ram, &ram::LU_RECUR[l], LU_RECUR_MAX);
        bad |= clamp_u16(ram, &ram::LU_WIN_LEFT[l], LU_WINDOW_S);
    }
    for s in 0..4 {
        bad |= clamp_u16(ram, &ram::STUCK_CNT[s], STUCK_S + 1);
        let (lo, hi) = (ram::STUCK_MIN[s].get(ram), ram::STUCK_MAX[s].get(ram));
        if lo > hi {
            ram::STUCK_CNT[s].set(ram, 0);
            ram::STUCK_MIN[s].set(ram, hi);
            bad = true;
        }
    }
    bad |= clamp_u16(ram, &ram::STORM_STABLE_S, STORM_CLEAR_S);
    let inv = ram::SENSOR_INVALID_MASK.get(ram);
    if inv & !STUCK_MASK != 0 {
        ram::SENSOR_INVALID_MASK.set(ram, inv & STUCK_MASK);
        bad = true;
    }
    if bad {
        hal.sys_reset();
    }
}

fn clamp_u8(ram: &mut Ram, s: &ram::Slot<u8>, max: u8) -> bool {
    let v = s.get(ram);
    if v > max {
        s.set(ram, max);
        true
    } else {
        false
    }
}

fn clamp_u16(ram: &mut Ram, s: &ram::Slot<u16>, max: u16) -> bool {
    let v = s.get(ram);
    if v > max {
        s.set(ram, max);
        true
    } else {
        false
    }
}

/// Latch-up : surintensité sur `LU_SAMPLES` cycles -> coupure `LU_OFF_S` s ; récidives -> isolement.
fn latchup<H: Hal>(ram: &mut Ram, hal: &mut H) {
    for line in Line::ALL {
        let l = line as usize;
        // Fenêtre de récidive.
        let win = ram::LU_WIN_LEFT[l].get(ram);
        if win > 0 {
            ram::LU_WIN_LEFT[l].set(ram, win - 1);
            if win == 1 {
                ram::LU_RECUR[l].set(ram, 0);
            }
        }
        // Coupure en cours : on décompte, pas d'échantillonnage (ligne hors tension).
        let off = ram::LU_OFF_LEFT[l].get(ram);
        if off > 0 {
            ram::LU_OFF_LEFT[l].set(ram, off - 1);
            ram::LU_OVER[l].set(ram, 0);
            continue;
        }
        let over = match hal.sensor(LINE_SENSOR[l]) {
            Ok(i) if i >= ILINE_RANGE_MA.0 && i <= ILINE_RANGE_MA.1 => i > LU_THRESHOLD_MA[l],
            _ => {
                // Capteur invalide ce cycle (C1) : pas une surintensité (`LU_OVER` remis à 0
                // ci-dessous), défaut signalé dans `sensor_fault_mask` (SPEC §4.4). `power`
                // réécrit le masque à chaque cycle avant `fdir` : un OU suffit, sans état.
                let m = ram::SENSOR_FAULT_MASK.get(ram) | (1 << LINE_SENSOR[l] as u16);
                ram::SENSOR_FAULT_MASK.set(ram, m);
                false
            }
        };
        let n = if over {
            ram::LU_OVER[l].get(ram).saturating_add(1)
        } else {
            0
        };
        if n < LU_SAMPLES {
            ram::LU_OVER[l].set(ram, n);
            continue;
        }
        // Détection.
        ram::LU_OVER[l].set(ram, 0);
        ram::LU_OFF_LEFT[l].set(ram, LU_OFF_S);
        let c = ram::CNT_LATCHUP.get(ram).saturating_add(1);
        ram::CNT_LATCHUP.set(ram, c);
        let recur = if ram::LU_WIN_LEFT[l].get(ram) == 0 {
            ram::LU_WIN_LEFT[l].set(ram, LU_WINDOW_S);
            1
        } else {
            ram::LU_RECUR[l]
                .get(ram)
                .saturating_add(1)
                .min(LU_RECUR_MAX)
        };
        ram::LU_RECUR[l].set(ram, recur);
        if recur >= LU_RECUR_MAX && line != Line::Radio {
            let iso = ram::LINE_ISOLATED.get(ram) | line.mask() as u8;
            ram::LINE_ISOLATED.set(ram, iso);
            persist(ram);
        }
    }
}

/// Lecture d'un capteur surveillé : `Some` si `Ok` et dans sa plage plausible.
fn plausible<H: Hal>(hal: &mut H, idx: usize) -> Option<i16> {
    let id = ram::STUCK_SENSORS[idx];
    let range = if idx < 2 {
        power::VBATT_RANGE_MV
    } else {
        power::TBATT_RANGE_DC
    };
    match hal.sensor(id) {
        Ok(v) if v >= range.0 && v <= range.1 => Some(v),
        _ => None,
    }
}

/// Capteur figé : valeur brute identique pendant `STUCK_S` s alors que le redondant a varié.
fn stuck_sensors<H: Hal>(ram: &mut Ram, hal: &mut H) {
    let mut vals = [None; 4];
    for (i, v) in vals.iter_mut().enumerate() {
        *v = plausible(hal, i);
    }
    for i in 0..4 {
        let p = i ^ 1; // redondant : A <-> B
        let bit = 1u16 << (ram::STUCK_SENSORS[i] as u16);
        let pbit = 1u16 << (ram::STUCK_SENSORS[p] as u16);
        let inv = ram::SENSOR_INVALID_MASK.get(ram);
        let Some(v) = vals[i] else {
            ram::STUCK_CNT[i].set(ram, 0);
            continue;
        };
        if inv & bit != 0 {
            continue;
        }
        let cnt = ram::STUCK_CNT[i].get(ram);
        if cnt == 0 || v != ram::STUCK_LAST[i].get(ram) {
            // Nouvelle référence : la fenêtre du redondant repart de sa valeur courante.
            ram::STUCK_LAST[i].set(ram, v);
            ram::STUCK_CNT[i].set(ram, 1);
            let pv = vals[p].unwrap_or(0);
            ram::STUCK_MIN[i].set(ram, pv);
            ram::STUCK_MAX[i].set(ram, pv);
            if vals[p].is_none() {
                // Redondant illisible : pas de référence, la fenêtre ne s'ouvre pas.
                ram::STUCK_CNT[i].set(ram, 0);
            }
            continue;
        }
        let cnt = (cnt + 1).min(STUCK_S + 1);
        ram::STUCK_CNT[i].set(ram, cnt);
        if let Some(pv) = vals[p] {
            if pv < ram::STUCK_MIN[i].get(ram) {
                ram::STUCK_MIN[i].set(ram, pv);
            }
            if pv > ram::STUCK_MAX[i].get(ram) {
                ram::STUCK_MAX[i].set(ram, pv);
            }
        }
        // `cnt` échantillons identiques couvrent `cnt - 1` secondes.
        if cnt > STUCK_S && inv & pbit == 0 {
            let span = ram::STUCK_MAX[i].get(ram) as i32 - ram::STUCK_MIN[i].get(ram) as i32;
            let need = if i < 2 { STUCK_SPAN_MV } else { STUCK_SPAN_DC };
            if span >= need as i32 {
                ram::SENSOR_INVALID_MASK.set(ram, inv | bit);
                ram::STUCK_CNT[i].set(ram, 0);
            }
        }
    }
}

/// Tempête de resets : boot long -> compteur de boots courts remis à zéro ; sous verrou, levée
/// après `STORM_CLEAR_S` s stables. Sous verrou le compteur n'est PAS remis à zéro à 300 s : un
/// reset pendant le verrou le ré-enclenche au boot suivant.
fn storm(ram: &mut Ram) {
    let flags = ram::FDIR_FLAGS.get(ram);
    if flags & ram::FDIR_FLAG_STORM_LOCK != 0 {
        let s = ram::STORM_STABLE_S.get(ram).saturating_add(1);
        if s >= STORM_CLEAR_S {
            ram::FDIR_FLAGS.set(ram, flags & !ram::FDIR_FLAG_STORM_LOCK);
            ram::STORM_STABLE_S.set(ram, 0);
            ram::SHORT_BOOTS.set(ram, 0);
            ram::BOOT_STABLE.set(ram, 1);
            persist(ram);
        } else {
            ram::STORM_STABLE_S.set(ram, s);
        }
    } else if ram::BOOT_STABLE.get(ram) == 0 && ram::UPTIME.get(ram) >= STABLE_BOOT_S {
        ram::BOOT_STABLE.set(ram, 1);
        ram::SHORT_BOOTS.set(ram, 0);
        persist(ram);
    }
}

/// Drapeaux dérivés, recalculés à chaque cycle (un drapeau corrompu se corrige seul).
fn publish_flags(ram: &mut Ram) {
    let mut d = 0u16;
    if ram::LU_RECUR[Line::Radio as usize].get(ram) >= LU_RECUR_MAX {
        d |= ram::FDIR_FLAG_CRITICAL;
    }
    if ram::LINE_ISOLATED.get(ram) & !(Line::Radio.mask() as u8) != 0 {
        d |= FDIR_FLAG_LINE_ISOLATED;
    }
    if (0..3).any(|l| ram::LU_OFF_LEFT[l].get(ram) > 0) {
        d |= FDIR_FLAG_LU_CUT;
    }
    if ram::SENSOR_INVALID_MASK.get(ram) != 0 {
        d |= FDIR_FLAG_SENSOR_INVALID;
    }
    let f = (ram::FDIR_FLAGS.get(ram) & !DERIVED_FLAGS) | d;
    ram::FDIR_FLAGS.set(ram, f);
}

#[cfg(test)]
mod tests {
    // Témoin `unprotected` : la plupart des tests (et leurs aides) n'existent qu'avec le FDIR.
    #![cfg_attr(feature = "unprotected", allow(dead_code, unused_imports))]
    extern crate std;
    use super::*;
    use crate::hal::{HalError, Reg};
    use crate::proto::{self, Beacon, FrameType};
    use crate::testhal::MockHal;
    use crate::{Firmware, Mode, StepReport};
    use std::vec::Vec;

    fn booted() -> (Firmware, MockHal) {
        let mut fw = Firmware::new();
        let mut hal = MockHal::nominal();
        fw.boot(&mut hal);
        (fw, hal)
    }

    /// Satellite en `Nominal` (tension haute, 200 cycles).
    fn nominal() -> (Firmware, MockHal) {
        let (mut fw, mut hal) = booted();
        for _ in 0..200 {
            fw.step(&mut hal);
        }
        assert_eq!(mode::current(fw.ram_mut()), Mode::Nominal);
        (fw, hal)
    }

    /// Latch-up simulé comme dans le sim (F4) : surintensité tant que la ligne est alimentée ;
    /// s'efface après 2 s de coupure.
    struct Latch {
        line: Line,
        nominal_ma: i16,
        extra_ma: i16,
        active: bool,
        off_s: u32,
    }

    impl Latch {
        fn new(line: Line, nominal_ma: i16, extra_ma: i16) -> Self {
            Latch {
                line,
                nominal_ma,
                extra_ma,
                active: false,
                off_s: 0,
            }
        }
        /// Met à jour le capteur de courant d'après l'état du registre (avant le `step`).
        fn drive(&mut self, hal: &mut MockHal) {
            let on = hal.line_on(self.line.mask());
            let i = if on {
                self.off_s = 0;
                self.nominal_ma + if self.active { self.extra_ma } else { 0 }
            } else {
                self.off_s += 1;
                if self.off_s >= 2 {
                    self.active = false;
                }
                0
            };
            hal.set(LINE_SENSOR[self.line as usize], Ok(i));
        }
    }

    /// Fait tourner `n` cycles avec le latch-up piloté ; renvoie l'état de la ligne par cycle.
    fn run_latch(fw: &mut Firmware, hal: &mut MockHal, lu: &mut Latch, n: u32) -> Vec<bool> {
        (0..n)
            .map(|_| {
                lu.drive(hal);
                fw.step(hal);
                hal.line_on(lu.line.mask())
            })
            .collect()
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn latchup_cut_within_5_cycles_then_rearmed() {
        let (mut fw, mut hal) = nominal();
        let mut lu = Latch::new(Line::Payload, 160, 300);
        lu.active = true;
        let on = run_latch(&mut fw, &mut hal, &mut lu, 30);
        let cut = on.iter().position(|&x| !x).expect("jamais coupée") + 1;
        assert!(cut <= 5, "coupée au cycle {cut}, exigé <= 5 (T_SEL = 30 s)");
        assert_eq!(cut, LU_SAMPLES as usize);
        // Coupée exactement LU_OFF_S cycles, puis réarmée (le latch-up s'est effacé).
        let off = on[cut - 1..].iter().take_while(|&&x| !x).count();
        assert_eq!(off, LU_OFF_S as usize);
        assert!(on[cut - 1 + off..].iter().all(|&x| x));
        let ram = fw.ram_mut();
        assert_eq!(ram::CNT_LATCHUP.get(ram), 1);
        assert_eq!(ram::LINE_ISOLATED.get(ram), 0);
        assert_eq!(mode::current(ram), Mode::Nominal);
        // La balise publie le compteur.
        for _ in 0..10 {
            fw.step(&mut hal);
        }
        assert_eq!(hal.beacons().last().unwrap().cnt_latchup, 1);
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn three_recurrences_isolate_payload_and_cap_eco() {
        let (mut fw, mut hal) = nominal();
        let mut lu = Latch::new(Line::Payload, 160, 500);
        for k in 0..LU_RECUR_MAX {
            assert_eq!(mode::current(fw.ram_mut()), Mode::Nominal, "récidive {k}");
            lu.active = true;
            run_latch(&mut fw, &mut hal, &mut lu, 20);
        }
        let ram = fw.ram_mut();
        assert_eq!(ram::CNT_LATCHUP.get(ram), 3);
        assert!(mode::line_isolated(ram, Line::Payload));
        assert_eq!(mode::current(ram), Mode::Eco);
        assert_ne!(ram::FDIR_FLAGS.get(ram) & FDIR_FLAG_LINE_ISOLATED, 0);
        // Plafonné à Eco, Payload coupée, même avec une batterie pleine.
        let on = run_latch(&mut fw, &mut hal, &mut lu, 1000);
        assert!(on.iter().all(|&x| !x));
        assert_eq!(mode::current(fw.ram_mut()), Mode::Eco);
        assert_ne!(
            hal.beacons().last().unwrap().fdir_flags & FDIR_FLAG_LINE_ISOLATED,
            0
        );
        // ClearFdir lève l'isolement : retour en Nominal après le maintien de 60 s.
        clear_fdir(fw.ram_mut());
        let r: Vec<StepReport> = (0..70).map(|_| fw.step(&mut hal)).collect();
        assert_eq!(r.last().unwrap().mode, Mode::Nominal);
        assert!(hal.line_on(Line::Payload.mask()));
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn recurrences_outside_window_do_not_isolate() {
        let (mut fw, mut hal) = nominal();
        let mut lu = Latch::new(Line::Heater, 220, 400);
        hal.set_temp(-100); // réchauffeur commandé
        for _ in 0..2 {
            lu.active = true;
            run_latch(&mut fw, &mut hal, &mut lu, 20);
        }
        // Fenêtre de 600 s ouverte à la 1re détection : on la laisse expirer.
        run_latch(&mut fw, &mut hal, &mut lu, LU_WINDOW_S as u32);
        lu.active = true;
        run_latch(&mut fw, &mut hal, &mut lu, 20);
        let ram = fw.ram_mut();
        assert_eq!(ram::CNT_LATCHUP.get(ram), 3);
        assert!(!mode::line_isolated(ram, Line::Heater));
        assert!(hal.line_on(Line::Heater.mask()));
        // Deux de plus dans la nouvelle fenêtre : isolée.
        for _ in 0..2 {
            lu.active = true;
            run_latch(&mut fw, &mut hal, &mut lu, 20);
        }
        assert!(mode::line_isolated(fw.ram_mut(), Line::Heater));
        assert!(!hal.line_on(Line::Heater.mask()));
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn radio_is_never_isolated_only_cycled() {
        let (mut fw, mut hal) = nominal();
        let mut lu = Latch::new(Line::Radio, 30, 300);
        for _ in 0..10 {
            lu.active = true;
            let on = run_latch(&mut fw, &mut hal, &mut lu, 15);
            let off = on.iter().filter(|&&x| !x).count();
            assert_eq!(off, LU_OFF_S as usize);
            assert!(*on.last().unwrap(), "Radio non réarmée");
        }
        let ram = fw.ram_mut();
        assert_eq!(ram::CNT_LATCHUP.get(ram), 10);
        assert_eq!(ram::LINE_ISOLATED.get(ram), 0);
        assert!(!mode::line_isolated(ram, Line::Radio));
        // Même un bit d'isolement Radio forcé n'a aucun effet.
        ram::LINE_ISOLATED.set(ram, Line::Radio.mask() as u8);
        fw.step(&mut hal);
        assert!(hal.line_on(Line::Radio.mask()));
        // Latch-up Radio récurrent : alarme critique publiée.
        assert_ne!(
            ram::FDIR_FLAGS.get(fw.ram_mut()) & ram::FDIR_FLAG_CRITICAL,
            0
        );
    }

    /// C1 (`docs/triage.md`) : une lecture de courant de ligne hors de `ILINE_RANGE_MA` ou `Err`
    /// n'est jamais un latch-up (ni coupure, ni compteur, ni isolement) ; le capteur est signalé
    /// dans `sensor_fault_mask` à chaque cycle.
    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn c1_iline_out_of_range_is_not_latchup() {
        let values: [Result<i16, HalError>; 4] =
            [Ok(30000), Ok(i16::MAX), Ok(-30000), Err(HalError::Io)];
        for line in Line::ALL {
            let l = line as usize;
            let bit = 1u16 << LINE_SENSOR[l] as u16;
            for v in values {
                let (mut fw, mut hal) = nominal();
                if line == Line::Heater {
                    hal.set_temp(-100); // réchauffeur commandé (batterie froide)
                }
                hal.set(LINE_SENSOR[l], v);
                for c in 0..2000 {
                    fw.step(&mut hal);
                    let ctx = std::format!("{line:?} {v:?} cycle {c}");
                    let ram = fw.ram_mut();
                    assert_eq!(ram::CNT_LATCHUP.get(ram), 0, "{ctx}");
                    assert_eq!(ram::LINE_ISOLATED.get(ram), 0, "{ctx}");
                    for k in 0..3 {
                        assert_eq!(ram::LU_OFF_LEFT[k].get(ram), 0, "{ctx}");
                    }
                    let f = ram::FDIR_FLAGS.get(ram);
                    assert_eq!(f & ram::FDIR_FLAG_CRITICAL, 0, "{ctx}");
                    assert_eq!(f & FDIR_FLAG_LINE_ISOLATED, 0, "{ctx}");
                    assert_ne!(ram::SENSOR_FAULT_MASK.get(ram) & bit, 0, "{ctx}");
                    assert!(hal.line_on(line.mask()), "ligne non commandée : {ctx}");
                }
                if line != Line::Heater {
                    assert_eq!(mode::current(fw.ram_mut()), Mode::Nominal);
                }
                assert_ne!(hal.beacons().last().unwrap().sensor_fault_mask & bit, 0);
                // Capteur revenu dans la plage : plus de bit au cycle suivant.
                hal.set(LINE_SENSOR[l], Ok(0));
                fw.step(&mut hal);
                assert_eq!(ram::SENSOR_FAULT_MASK.get(fw.ram_mut()) & bit, 0);
            }
        }
    }

    /// C1 : la borne haute de plausibilité ne masque pas un latch-up réel jusqu'à 2000 mA ;
    /// au-delà (2001 mA), la lecture est invalide et ignorée.
    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn c1_real_latchup_still_detected_up_to_range() {
        // (ligne, courant nominal simulé)
        let lines = [(Line::Payload, 160), (Line::Radio, 30), (Line::Heater, 220)];
        for (line, nominal_ma) in lines {
            let extras = [ILINE_RANGE_MA.1 - nominal_ma, 800];
            for extra in extras {
                let (mut fw, mut hal) = nominal();
                if line == Line::Heater {
                    hal.set_temp(-100);
                }
                let mut lu = Latch::new(line, nominal_ma, extra);
                // Ligne alimentée et lue nominale avant l'apparition du latch-up.
                let warm = run_latch(&mut fw, &mut hal, &mut lu, 5);
                assert!(warm.iter().skip(1).all(|&x| x), "{line:?} non commandée");
                lu.active = true;
                let on = run_latch(&mut fw, &mut hal, &mut lu, 30);
                let cut = on.iter().position(|&x| !x);
                assert_eq!(
                    cut,
                    Some(LU_SAMPLES as usize - 1),
                    "{line:?} +{extra} mA non coupée à temps"
                );
                let off = on[cut.unwrap()..].iter().take_while(|&&x| !x).count();
                assert_eq!(off, LU_OFF_S as usize, "{line:?} +{extra} mA");
                assert!(*on.last().unwrap(), "{line:?} non réarmée");
                let ram = fw.ram_mut();
                assert_eq!(ram::CNT_LATCHUP.get(ram), 1, "{line:?} +{extra} mA");
                assert_eq!(
                    ram::SENSOR_FAULT_MASK.get(ram) & (1 << LINE_SENSOR[line as usize] as u16),
                    0
                );
            }
            // 2001 mA : hors plage -> ignorée, aucune coupure.
            let (mut fw, mut hal) = nominal();
            if line == Line::Heater {
                hal.set_temp(-100);
            }
            let mut lu = Latch::new(line, nominal_ma, ILINE_RANGE_MA.1 + 1 - nominal_ma);
            run_latch(&mut fw, &mut hal, &mut lu, 5);
            lu.active = true;
            let on = run_latch(&mut fw, &mut hal, &mut lu, 30);
            assert!(on.iter().all(|&x| x), "{line:?} coupée à 2001 mA");
            assert_eq!(ram::CNT_LATCHUP.get(fw.ram_mut()), 0, "{line:?} 2001 mA");
        }
    }

    /// Capteur figé alors que le redondant varie : invalidé à 120 s, tension validée = capteur
    /// sain ; puis le sain tombe aussi (`Err`) -> plus aucune tension -> `Survie`.
    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn stuck_voltage_sensor_invalidated_at_120s() {
        let (mut fw, mut hal) = nominal();
        let bit_a = 1u16 << SensorId::VBattA as u16;
        // Un cycle où A bouge (dans le témoin à bruit nul, A et B sont constants depuis le
        // boot), puis A figé à partir du cycle 1.
        hal.set(SensorId::VBattA, Ok(7801));
        fw.step(&mut hal);
        hal.set(SensorId::VBattA, Ok(7800)); // figé
        let mut invalid_at = None;
        for k in 1..=200u32 {
            let b = 7790 + 40 * (k % 2) as i16; // redondant : varie de 40 mV
            hal.set(SensorId::VBattB, Ok(b));
            fw.step(&mut hal);
            let ram = fw.ram_mut();
            if invalid_at.is_none() && ram::SENSOR_INVALID_MASK.get(ram) & bit_a != 0 {
                invalid_at = Some(k);
            }
            if invalid_at.is_some_and(|t| k > t) {
                // Tension validée = capteur sain seul, bit publié.
                assert_eq!(ram::VBATT_MV.get(ram), b as u16);
                assert_ne!(ram::SENSOR_FAULT_MASK.get(ram) & bit_a, 0);
            }
        }
        // 1er échantillon figé au cycle 1 : 120 s plus tard, cycle 121.
        assert_eq!(invalid_at, Some(STUCK_S as u32 + 1));
        assert_eq!(mode::current(fw.ram_mut()), Mode::Nominal);
        let last = *hal.beacons().last().unwrap();
        assert_ne!(last.sensor_fault_mask & bit_a, 0);
        assert_ne!(last.fdir_flags & FDIR_FLAG_SENSOR_INVALID, 0);
        // Le sain tombe : aucun capteur de tension valide -> Survie immédiat.
        hal.set(SensorId::VBattB, Err(HalError::Io));
        assert_eq!(fw.step(&mut hal).mode, Mode::Survie);
        assert!(!ram::VBATT_VALID.get(fw.ram_mut()));
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn stuck_detection_needs_redundant_variation() {
        // Redondant qui varie de 29 mV seulement : rien. Les deux constants : rien.
        let (mut fw, mut hal) = nominal();
        for k in 0..400u32 {
            hal.set(SensorId::VBattB, Ok(7790 + 29 * (k % 2) as i16));
            fw.step(&mut hal);
        }
        assert_eq!(ram::SENSOR_INVALID_MASK.get(fw.ram_mut()), 0);
        // Température : A figée, B varie de 1,5 °C -> A invalidée, thermostat sur B.
        let bit_ta = 1u16 << SensorId::TempBattA as u16;
        hal.set(SensorId::TempBattA, Ok(200));
        for k in 0..(STUCK_S as u32 + 5) {
            hal.set(SensorId::TempBattB, Ok(195 + 15 * (k % 2) as i16));
            fw.step(&mut hal);
        }
        let ram = fw.ram_mut();
        assert_eq!(ram::SENSOR_INVALID_MASK.get(ram), bit_ta);
        // Jamais les deux par figeage : B figé à son tour alors que A (invalide) varie.
        hal.set(SensorId::TempBattB, Ok(10));
        for k in 0..400u32 {
            hal.set(SensorId::TempBattA, Ok(100 + 50 * (k % 2) as i16));
            fw.step(&mut hal);
        }
        let ram = fw.ram_mut();
        assert_eq!(ram::SENSOR_INVALID_MASK.get(ram), bit_ta);
        assert_eq!(ram::TBATT_DC.get(ram), 10);
        assert!(
            hal.line_on(Line::Heater.mask()),
            "thermostat sur le capteur sain"
        );
        // ClearFdir lève l'invalidation.
        clear_fdir(fw.ram_mut());
        assert_eq!(ram::SENSOR_INVALID_MASK.get(fw.ram_mut()), 0);
    }

    /// Reboot simulé : RAM perdue, compteur de boots courts restauré comme le ferait la NVM.
    fn reboot(fw: &mut Firmware, hal: &mut MockHal) {
        let nvm_short = ram::SHORT_BOOTS.get(fw.ram_mut());
        let ram = fw.ram_mut();
        crate::sched::boot_erase(ram, hal);
        ram::SHORT_BOOTS.set(ram, nvm_short);
        crate::sched::boot_finish(ram, hal);
    }

    fn storm_locked(fw: &mut Firmware) -> bool {
        ram::FDIR_FLAGS.get(fw.ram_mut()) & ram::FDIR_FLAG_STORM_LOCK != 0
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn three_short_boots_lock_survie_until_clear_fdir() {
        let (mut fw, mut hal) = booted();
        assert_eq!(ram::SHORT_BOOTS.get(fw.ram_mut()), 1);
        for _ in 0..100 {
            fw.step(&mut hal);
        }
        reboot(&mut fw, &mut hal);
        assert!(!storm_locked(&mut fw));
        for _ in 0..299 {
            fw.step(&mut hal);
        }
        reboot(&mut fw, &mut hal);
        // 3e boot sans qu'aucun n'ait atteint 300 s : verrou.
        assert_eq!(ram::SHORT_BOOTS.get(fw.ram_mut()), 3);
        assert!(storm_locked(&mut fw));
        assert_ne!(ram::NVM_PENDING.get(fw.ram_mut()), 0);
        // Batterie pleine, mais Survie tenue, même au-delà de 300 s.
        for _ in 0..1000 {
            assert_eq!(fw.step(&mut hal).mode, Mode::Survie);
        }
        assert!(storm_locked(&mut fw));
        assert_ne!(
            hal.beacons().last().unwrap().fdir_flags & ram::FDIR_FLAG_STORM_LOCK,
            0
        );
        // Sous verrou, le compteur n'est pas remis à zéro : un reset ré-enclenche le verrou.
        reboot(&mut fw, &mut hal);
        assert!(storm_locked(&mut fw));
        // ClearFdir : verrou levé, compteur à zéro, remontée normale (uptime >= 120 + 60 s).
        for _ in 0..130 {
            fw.step(&mut hal);
        }
        clear_fdir(fw.ram_mut());
        assert!(!storm_locked(&mut fw));
        assert_eq!(ram::SHORT_BOOTS.get(fw.ram_mut()), 0);
        let r: Vec<StepReport> = (0..130).map(|_| fw.step(&mut hal)).collect();
        assert_eq!(r[59].mode, Mode::Eco);
        assert_eq!(r[119].mode, Mode::Nominal);
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn storm_lock_lifts_after_one_stable_orbit_and_long_boot_resets_counter() {
        let (mut fw, mut hal) = booted();
        ram::SHORT_BOOTS.set(fw.ram_mut(), 2);
        reboot(&mut fw, &mut hal);
        assert!(storm_locked(&mut fw));
        for k in 1..=STORM_CLEAR_S as u32 {
            fw.step(&mut hal);
            assert_eq!(storm_locked(&mut fw), k < STORM_CLEAR_S as u32, "cycle {k}");
        }
        assert_eq!(ram::SHORT_BOOTS.get(fw.ram_mut()), 0);
        let r: Vec<StepReport> = (0..130).map(|_| fw.step(&mut hal)).collect();
        assert_eq!(r.last().unwrap().mode, Mode::Nominal);
        // Boot long (>= 300 s) : compteur remis à zéro, persistance demandée.
        reboot(&mut fw, &mut hal);
        assert_eq!(ram::SHORT_BOOTS.get(fw.ram_mut()), 1);
        for _ in 0..STABLE_BOOT_S {
            fw.step(&mut hal);
        }
        assert_eq!(ram::SHORT_BOOTS.get(fw.ram_mut()), 0);
        ram::NVM_PENDING.set(fw.ram_mut(), 0);
        reboot(&mut fw, &mut hal);
        assert_eq!(ram::SHORT_BOOTS.get(fw.ram_mut()), 1);
        assert!(!storm_locked(&mut fw));
    }

    #[test]
    fn frozen_heartbeat_stops_watchdog_kicks() {
        let (mut fw, mut hal) = nominal();
        let before = hal.kicks.len();
        for _ in 0..30 {
            // Cellule forcée (les 3 copies) : le battement ne peut plus avancer.
            ram::FDIR_HEARTBEAT.set(fw.ram_mut(), 0x1234);
            fw.step(&mut hal);
        }
        assert!(
            hal.kicks.len() - before <= 1,
            "battement gelé mais watchdog rechargé"
        );
        // Libéré : les rechargements reprennent.
        fw.step(&mut hal);
        assert!(fw.step(&mut hal).kicked);
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn every_register_altered_is_rewritten_next_cycle() {
        let (mut fw, mut hal) = nominal();
        for r in Reg::ALL {
            for bit in 0..16 {
                let good = hal.regs[r as usize];
                hal.regs[r as usize] ^= 1 << bit;
                let fix0 = ram::CNT_REG_FIX.get(fw.ram_mut());
                fw.step(&mut hal);
                assert_eq!(hal.regs[r as usize], good, "{r:?} bit {bit}");
                assert_eq!(ram::CNT_REG_FIX.get(fw.ram_mut()), fix0 + 1);
            }
        }
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn tmr_no_majority_or_out_of_bounds_state_requests_reset() {
        let (mut fw, mut hal) = nominal();
        // Trois copies d'un octet deux à deux différentes (octet de poids fort de l'uptime,
        // lu en début de cycle : réaction dans le même cycle ; vote bit à bit -> 0, juste).
        let ram = fw.ram_mut();
        let (o, s) = (ram::UPTIME.off(), ram::UPTIME.tmr_stride());
        ram[o] = 0x01;
        ram[o + s] = 0x02;
        ram[o + 2 * s] = 0x04;
        fw.step(&mut hal);
        assert_eq!(hal.resets, 1);
        assert_eq!(ram::TMR_ALARM.get(fw.ram_mut()), 0);
        fw.step(&mut hal);
        assert_eq!(hal.resets, 1, "alarme traitée une seule fois");
        // État hors bornes (deux copies touchées) : assaini + reset.
        ram::LU_OFF_LEFT[Line::Radio as usize].set(fw.ram_mut(), 200);
        fw.step(&mut hal);
        assert_eq!(hal.resets, 2);
        assert!(ram::LU_OFF_LEFT[Line::Radio as usize].get(fw.ram_mut()) <= LU_OFF_S);
        // La Radio revient en au plus 5 s.
        for _ in 0..LU_OFF_S {
            fw.step(&mut hal);
        }
        assert!(hal.line_on(Line::Radio.mask()));
    }

    /// Témoin : FDIR neutralisé (latch-up jamais coupé), battement et watchdog intacts.
    #[cfg(feature = "unprotected")]
    #[test]
    fn unprotected_fdir_and_refresh_are_neutralized() {
        let (mut fw, mut hal) = nominal();
        let mut lu = Latch::new(Line::Payload, 160, 500);
        lu.active = true;
        let on = run_latch(&mut fw, &mut hal, &mut lu, 60);
        assert!(on.iter().all(|&x| x), "latch-up coupé sans FDIR");
        assert_eq!(ram::CNT_LATCHUP.get(fw.ram_mut()), 0);
        assert!(hal.kicks.len() >= 260);
        hal.regs[Reg::WdgCtrl as usize] = 0;
        fw.step(&mut hal);
        assert_eq!(
            hal.regs[Reg::WdgCtrl as usize],
            0,
            "registre rafraîchi sans protection"
        );
        assert_eq!(ram::SHORT_BOOTS.get(fw.ram_mut()), 0);
    }

    // --- Test de masse : tout bit des régions Fdir et Sched -----------------------------------

    const MASS_CYCLES: u32 = 300;

    /// Scénario FDIR actif : latch-up Payload à 10 s, VBattA figé avec VBattB qui varie
    /// (A figé à 20 s : invalidation à 140 s), latch-up Radio à 200 s.
    struct Scenario {
        payload: Latch,
        radio: Latch,
    }

    impl Scenario {
        fn new() -> Self {
            Scenario {
                payload: Latch::new(Line::Payload, 160, 400),
                radio: Latch::new(Line::Radio, 30, 300),
            }
        }
        fn feed(&mut self, hal: &mut MockHal, k: u32) {
            if k == 10 {
                self.payload.active = true;
            }
            if k == 200 {
                self.radio.active = true;
            }
            self.payload.drive(hal);
            self.radio.drive(hal);
            let a = if k < 20 { 7800 + (k % 2) as i16 } else { 7800 };
            hal.set(SensorId::VBattA, Ok(a));
            hal.set(SensorId::VBattB, Ok(7790 + 40 * (k % 2) as i16));
        }
    }

    fn normalize(f: &[u8]) -> Vec<u8> {
        let fr = proto::decode_frame(f).expect("trame émise invalide");
        if fr.ftype != FrameType::Beacon {
            return f.to_vec();
        }
        let mut b = Beacon::decode(fr.payload).expect("balise invalide");
        b.cnt_tmr_fix = 0;
        b.cnt_ecc_corr = 0;
        b.encode().to_vec()
    }

    struct Trace {
        reports: Vec<StepReport>,
        frames: Vec<Vec<u8>>,
        kicks: Vec<u8>,
        regs: [u16; 4],
        resets: usize,
    }

    fn mass_run(ram0: &ram::Ram, hal0: &MockHal, flip: Option<usize>) -> Trace {
        let mut fw = Firmware::new();
        *fw.ram_mut() = *ram0;
        let mut hal = hal0.clone();
        let (s0, k0, r0) = (hal.sent.len(), hal.kicks.len(), hal.resets);
        if let Some(bit) = flip {
            fw.ram_mut()[bit / 8] ^= 1 << (bit % 8);
        }
        let mut sc = Scenario::new();
        let reports = (0..MASS_CYCLES)
            .map(|k| {
                sc.feed(&mut hal, k);
                fw.step(&mut hal)
            })
            .collect();
        Trace {
            reports,
            frames: hal.sent[s0..].iter().map(|f| normalize(f)).collect(),
            kicks: hal.kicks[k0..].to_vec(),
            regs: hal.regs,
            resets: hal.resets - r0,
        }
    }

    /// Tout bit des régions `Fdir` et `Sched` basculé une fois : sur 300 cycles, comportement
    /// identique au témoin, ou reset demandé, ou watchdog non rechargé ; jamais un état
    /// divergent silencieux.
    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn every_bit_of_fdir_and_sched_regions_is_identical_or_loud() {
        let (mut fw, hal0) = nominal();
        let ram0 = *fw.ram_mut();
        let w = mass_run(&ram0, &hal0, None);
        // Le témoin exerce bien le FDIR.
        assert!(w.reports.iter().all(|r| r.kicked));
        assert_eq!(w.resets, 0);
        let (mut identical, mut loud) = (0usize, 0usize);
        for r in ram::RAM_MAP
            .iter()
            .filter(|r| matches!(r.kind, ram::RegionKind::Fdir | ram::RegionKind::Sched))
        {
            for bit in r.start * 8..(r.start + r.len) * 8 {
                let t = mass_run(&ram0, &hal0, Some(bit));
                let same = t.reports == w.reports
                    && t.kicks == w.kicks
                    && t.frames == w.frames
                    && t.regs == w.regs;
                if same {
                    identical += 1;
                } else {
                    assert!(
                        t.resets > 0 || t.kicks.len() < w.kicks.len(),
                        "région {} octet {} bit {} : divergence silencieuse",
                        r.name,
                        bit / 8,
                        bit % 8
                    );
                    loud += 1;
                }
            }
        }
        std::println!(
            "{} basculements Fdir+Sched x {MASS_CYCLES} cycles : {identical} identiques, {loud} \
             reset/watchdog",
            identical + loud
        );
        assert_eq!(identical + loud, (ram::FDIR_LEN + ram::SCHED_LEN) * 8);
    }

    /// Le témoin du test de masse exerce réellement les trois détections.
    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn mass_scenario_exercises_fdir() {
        let (mut fw, mut hal) = nominal();
        let mut sc = Scenario::new();
        let mut invalid_at = None;
        for k in 0..MASS_CYCLES {
            sc.feed(&mut hal, k);
            fw.step(&mut hal);
            if invalid_at.is_none() && ram::SENSOR_INVALID_MASK.get(fw.ram_mut()) != 0 {
                invalid_at = Some(k);
            }
        }
        assert_eq!(invalid_at, Some(20 + STUCK_S as u32));
        let ram = fw.ram_mut();
        assert_eq!(ram::CNT_LATCHUP.get(ram), 2);
        assert_eq!(ram::LU_RECUR[Line::Radio as usize].get(ram), 1);
        assert_eq!(
            ram::SENSOR_INVALID_MASK.get(ram),
            1 << SensorId::VBattA as u16
        );
    }
}
