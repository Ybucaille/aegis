//! Exécutif cyclique (SPEC §4.1) : ordre fixe = priorité, pas de préemption, pas d'attente.
//!
//! Un cycle = un tick :
//! 1. `begin_cycle` : drapeaux d'exécution remis à 0, uptime + 1 ;
//! 2. tâches critiques `health`, `power`, `fdir`, `thermal` ; puis non critiques `comms_rx`,
//!    `telemetry`, `scrub`, et `payload` en `Nominal` seulement ;
//! 3. `finish_cycle` : `wdg_kick` SEULEMENT si les 4 drapeaux critiques sont posés ET si le
//!    battement du FDIR a avancé d'exactement 1 depuis le cycle précédent. Clés alternées
//!    `0xA5` / `0x5A` ; la clé suivante ne change que si le rechargement a été émis.
//!
//! Invariant : aucune tâche critique ne lit un résultat d'une tâche non critique du même cycle
//! (le plafond écrit par `comms_rx` n'est lu par `power` qu'au cycle suivant).

use crate::hal::{Hal, Reg};
use crate::mode;
use crate::ram::{self, Ram};
use crate::tasks;
use crate::{
    Mode, StepReport, RADIO_CFG_VALUE, WDG_CTRL_VALUE, WDG_KEY_A, WDG_KEY_B, WDG_TIMEOUT_S,
};

pub const FLAG_HEALTH: u8 = 1 << 0;
pub const FLAG_POWER: u8 = 1 << 1;
pub const FLAG_FDIR: u8 = 1 << 2;
pub const FLAG_THERMAL: u8 = 1 << 3;
pub const FLAG_COMMS_RX: u8 = 1 << 4;
pub const FLAG_TELEMETRY: u8 = 1 << 5;
pub const FLAG_SCRUB: u8 = 1 << 6;
pub const FLAG_PAYLOAD: u8 = 1 << 7;
/// Drapeaux exigés pour recharger le watchdog.
pub const CRITICAL_FLAGS: u8 = FLAG_HEALTH | FLAG_POWER | FLAG_FDIR | FLAG_THERMAL;

pub fn set_flag(ram: &mut Ram, flag: u8) {
    let f = ram::EXEC_FLAGS.get(ram) | flag;
    ram::EXEC_FLAGS.set(ram, f);
}

/// Démarrage : RAM effacée, valeurs par défaut, registres écrits. NVM : T-011.
pub fn boot<H: Hal>(ram: &mut Ram, hal: &mut H) {
    *ram = [0; crate::hal::RAM_SIZE];
    let cause = hal.reset_cause();
    ram::RESET_CAUSE.set(ram, cause as u8);
    ram::MODE.set(ram, Mode::Survie as u8);
    ram::CEILING.set(ram, Mode::Nominal as u8);
    ram::WDG_KEY.set(ram, WDG_KEY_A);
    ram::BEACON_ELAPSED.set(ram, u16::MAX);
    hal.reg_write(Reg::WdgTimeout, WDG_TIMEOUT_S);
    hal.reg_write(Reg::WdgCtrl, WDG_CTRL_VALUE);
    hal.reg_write(Reg::RadioCfg, RADIO_CFG_VALUE);
    mode::apply_lines(ram, hal);
}

pub fn begin_cycle(ram: &mut Ram) {
    ram::EXEC_FLAGS.set(ram, 0);
    let up = ram::UPTIME.get(ram).saturating_add(1);
    ram::UPTIME.set(ram, up);
}

/// Fin de cycle : décision et émission du rechargement du watchdog. Renvoie `true` si émis.
pub fn finish_cycle<H: Hal>(ram: &mut Ram, hal: &mut H) -> bool {
    let flags_ok = ram::EXEC_FLAGS.get(ram) & CRITICAL_FLAGS == CRITICAL_FLAGS;
    let hb = ram::FDIR_HEARTBEAT.get(ram);
    let hb_ok = hb == ram::HB_SEEN.get(ram).wrapping_add(1);
    ram::HB_SEEN.set(ram, hb);
    if !(flags_ok && hb_ok) {
        return false;
    }
    // Clé corrompue (ni A ni B) : on repart sur A ; au pire un rechargement est refusé par le
    // matériel, la clé suivante (B) est alors acceptée.
    let key = if ram::WDG_KEY.get(ram) == WDG_KEY_B {
        WDG_KEY_B
    } else {
        WDG_KEY_A
    };
    hal.wdg_kick(key);
    ram::WDG_KEY.set(
        ram,
        if key == WDG_KEY_A {
            WDG_KEY_B
        } else {
            WDG_KEY_A
        },
    );
    true
}

/// Un cycle complet.
pub fn step<H: Hal>(ram: &mut Ram, hal: &mut H) -> StepReport {
    begin_cycle(ram);
    tasks::health::run(ram, hal);
    tasks::power::run(ram, hal);
    tasks::fdir::run(ram, hal);
    tasks::thermal::run(ram, hal);
    let executed_tc = tasks::comms_rx::run(ram, hal);
    tasks::telemetry::run(ram, hal);
    tasks::scrub::run(ram, hal);
    if mode::current(ram) == Mode::Nominal {
        tasks::payload::run(ram, hal);
    }
    let kicked = finish_cycle(ram, hal);
    StepReport {
        executed_tc,
        mode: mode::current(ram),
        kicked,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::hal::{HalError, Line, SensorId, MAX_FRAME, RAM_SIZE};
    use crate::proto::{self, AckResult, Command};
    use crate::testhal::{MockHal, TestRng};
    use crate::Firmware;
    use std::println;
    use std::vec::Vec;

    fn booted() -> (Firmware, MockHal) {
        let mut fw = Firmware::new();
        let mut hal = MockHal::nominal();
        fw.boot(&mut hal);
        (fw, hal)
    }

    fn run(fw: &mut Firmware, hal: &mut MockHal, n: u32) -> Vec<StepReport> {
        (0..n).map(|_| fw.step(hal)).collect()
    }

    /// Cycle (1-based) où le mode devient `m` pour la première fois.
    fn first_in(reports: &[StepReport], m: Mode) -> Option<usize> {
        reports.iter().position(|r| r.mode == m).map(|i| i + 1)
    }

    #[test]
    fn firmware_is_exactly_ram() {
        assert_eq!(core::mem::size_of::<Firmware>(), RAM_SIZE);
    }

    #[test]
    fn boot_gives_survie_and_writes_registers() {
        let (mut fw, mut hal) = booted();
        assert_eq!(mode::current(fw.ram_mut()), Mode::Survie);
        assert_eq!(hal.regs[Reg::WdgTimeout as usize], WDG_TIMEOUT_S);
        assert_eq!(hal.regs[Reg::WdgCtrl as usize], WDG_CTRL_VALUE);
        assert_eq!(hal.regs[Reg::RadioCfg as usize], RADIO_CFG_VALUE);
        assert_eq!(hal.regs[Reg::LoadSwitches as usize], Line::Radio.mask());
        let r = fw.step(&mut hal);
        assert_eq!(r.mode, Mode::Survie);
        assert!(r.kicked);
        // Première balise dès le premier cycle, en Survie, cause de reset publiée.
        let b = hal.beacons();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].mode, Mode::Survie as u8);
        assert_eq!(b[0].uptime_s, 1);
        assert_eq!(b[0].reset_cause, crate::ResetCause::PowerOn as u8);
    }

    #[test]
    fn boot_erases_ram_and_reads_reset_cause() {
        let mut fw = Firmware::new();
        fw.ram_mut().fill(0xEE);
        let mut hal = MockHal::nominal();
        hal.reset_cause = crate::ResetCause::Watchdog;
        fw.boot(&mut hal);
        let ram = fw.ram_mut();
        assert_eq!(ram::RESET_CAUSE.get(ram), crate::ResetCause::Watchdog as u8);
        assert_eq!(ram::UPTIME.get(ram), 0);
        assert_eq!(ram::LAST_TC_SEQ.get(ram), 0);
        // Hors des cellules initialisées, tout est à zéro.
        let init = [
            ram::RESET_CAUSE.off(),
            ram::MODE.off(),
            ram::CEILING.off(),
            ram::WDG_KEY.off(),
            ram::BEACON_ELAPSED.off(),
            ram::BEACON_ELAPSED.off() + 1,
        ];
        for (i, &b) in ram.iter().enumerate() {
            if !init.contains(&i) {
                assert_eq!(b, 0, "octet {i} non effacé");
            }
        }
    }

    #[test]
    fn scenario_survie_eco_nominal_respects_durations() {
        let (mut fw, mut hal) = booted();
        // Tension basse 100 s, puis 7100 mV (>= 7080) jusqu'à 400 s, puis 7600 mV.
        hal.set_vbatt(7000);
        let mut r = run(&mut fw, &mut hal, 100);
        hal.set_vbatt(7100);
        r.extend(run(&mut fw, &mut hal, 300));
        hal.set_vbatt(7600);
        r.extend(run(&mut fw, &mut hal, 200));
        // Montée Eco : 60 s de maintien à partir du cycle 101 -> cycle 160 (uptime >= 120 ok).
        assert_eq!(first_in(&r, Mode::Eco), Some(160));
        // Nominal : 60 s >= 7560 à partir du cycle 401 -> cycle 460.
        assert_eq!(first_in(&r, Mode::Nominal), Some(460));
        assert!(r[459..].iter().all(|x| x.mode == Mode::Nominal));
        assert!(hal.line_on(Line::Payload.mask()));

        // Ici c'est l'uptime qui limite : tension haute dès le boot.
        let (mut fw, mut hal) = booted();
        let r = run(&mut fw, &mut hal, 300);
        assert_eq!(first_in(&r, Mode::Eco), Some(120));
        assert_eq!(first_in(&r, Mode::Nominal), Some(180));
        // Payload alimentée seulement en Nominal.
        assert!(hal.line_on(Line::Payload.mask()));
    }

    #[test]
    fn critical_voltage_10s_gives_survie() {
        let (mut fw, mut hal) = booted();
        run(&mut fw, &mut hal, 200);
        assert_eq!(mode::current(fw.ram_mut()), Mode::Nominal);
        hal.set_vbatt(6700);
        let r = run(&mut fw, &mut hal, 20);
        assert_eq!(first_in(&r, Mode::Survie), Some(10));
        assert!(r[..9].iter().all(|x| x.mode != Mode::Survie));
        assert!(!hal.line_on(Line::Payload.mask()));
        assert!(hal.line_on(Line::Radio.mask()));
    }

    #[test]
    fn ab_disagreement_gives_low_value_and_flag() {
        let (mut fw, mut hal) = booted();
        hal.set(SensorId::VBattA, Ok(7800));
        hal.set(SensorId::VBattB, Ok(7400));
        fw.step(&mut hal);
        let b = *hal.beacons().last().unwrap();
        assert_eq!(b.vbatt_mv, 7400);
        assert_ne!(b.sensor_fault_mask & ram::SFM_VBATT_DISAGREE, 0);
        // Accord : moyenne, pas de drapeau.
        hal.set(SensorId::VBattB, Ok(7700));
        fw.step(&mut hal);
        let ram = fw.ram_mut();
        assert_eq!(ram::VBATT_MV.get(ram), 7750);
        assert_eq!(ram::SENSOR_FAULT_MASK.get(ram), 0);
    }

    #[test]
    fn single_or_no_voltage_sensor() {
        let (mut fw, mut hal) = booted();
        run(&mut fw, &mut hal, 200);
        hal.set(SensorId::VBattA, Err(HalError::Io));
        fw.step(&mut hal);
        {
            let ram = fw.ram_mut();
            assert_eq!(ram::VBATT_MV.get(ram), 7800);
            assert_ne!(ram::SENSOR_FAULT_MASK.get(ram) & 1, 0);
            assert_eq!(mode::current(ram), Mode::Nominal);
        }
        // Hors plage = invalide aussi ; les deux invalides -> Survie immédiat.
        hal.set(SensorId::VBattB, Ok(-3));
        let r = fw.step(&mut hal);
        assert_eq!(r.mode, Mode::Survie);
        assert!(!ram::VBATT_VALID.get(fw.ram_mut()));
        assert!(
            r.kicked,
            "la perte des capteurs ne doit pas couper le watchdog"
        );
    }

    #[test]
    fn thermostat_hysteresis_per_mode() {
        let (mut fw, mut hal) = booted();
        let heater = Line::Heater.mask();
        // Survie : ON sous 0 °C, OFF au-dessus de +5 °C.
        hal.set_temp(10);
        fw.step(&mut hal);
        assert!(!hal.line_on(heater));
        hal.set_temp(-1);
        fw.step(&mut hal);
        assert!(hal.line_on(heater));
        hal.set_temp(50);
        fw.step(&mut hal);
        assert!(hal.line_on(heater), "hystérésis");
        hal.set_temp(51);
        fw.step(&mut hal);
        assert!(!hal.line_on(heater));
        // Nominal : ON sous +2 °C.
        hal.set_temp(200);
        run(&mut fw, &mut hal, 200);
        assert_eq!(mode::current(fw.ram_mut()), Mode::Nominal);
        hal.set_temp(10);
        fw.step(&mut hal);
        assert!(hal.line_on(heater));
        hal.set_temp(81);
        fw.step(&mut hal);
        assert!(!hal.line_on(heater));
        // Désaccord de température : la plus froide + drapeau.
        hal.set(SensorId::TempBattA, Ok(100));
        hal.set(SensorId::TempBattB, Ok(-10));
        fw.step(&mut hal);
        let ram = fw.ram_mut();
        assert_eq!(ram::TBATT_DC.get(ram), -10);
        assert_ne!(ram::SENSOR_FAULT_MASK.get(ram) & ram::SFM_TEMP_DISAGREE, 0);
        assert!(hal.line_on(heater));
    }

    #[test]
    fn no_temperature_turns_heater_off() {
        let (mut fw, mut hal) = booted();
        hal.set_temp(-30);
        fw.step(&mut hal);
        assert!(hal.line_on(Line::Heater.mask()));
        hal.set(SensorId::TempBattA, Err(HalError::NotPowered));
        hal.set(SensorId::TempBattB, Ok(i16::MAX));
        fw.step(&mut hal);
        assert!(!hal.line_on(Line::Heater.mask()));
    }

    #[test]
    fn ping_is_acknowledged() {
        let (mut fw, mut hal) = booted();
        hal.push_tc(1, &Command::Ping);
        let r = fw.step(&mut hal);
        assert_eq!(r.executed_tc, Some((1, proto::opcode::PING)));
        let acks = hal.acks();
        assert_eq!(acks.len(), 1);
        assert_eq!(acks[0].tc_seq, 1);
        assert_eq!(acks[0].opcode, proto::opcode::PING);
        assert_eq!(acks[0].result, AckResult::Ok);
        assert_eq!(ram::LAST_TC_SEQ.get(fw.ram_mut()), 1);
        // La balise suivante publie last_tc_seq.
        run(&mut fw, &mut hal, 60);
        assert_eq!(hal.beacons().last().unwrap().last_tc_seq, 1);
        // Pas d'ACK en double.
        assert_eq!(hal.acks().len(), 1);
    }

    #[test]
    fn set_mode_sets_ceiling() {
        let (mut fw, mut hal) = booted();
        run(&mut fw, &mut hal, 200);
        assert_eq!(mode::current(fw.ram_mut()), Mode::Nominal);
        hal.push_tc(5, &Command::SetMode(Mode::Eco as u8));
        let r = fw.step(&mut hal);
        assert_eq!(r.executed_tc, Some((5, proto::opcode::SET_MODE)));
        assert_eq!(hal.acks()[0].result, AckResult::Ok);
        // Le plafond est lu par `power` au cycle suivant.
        assert_eq!(fw.step(&mut hal).mode, Mode::Eco);
        hal.push_tc(6, &Command::SetMode(7));
        fw.step(&mut hal);
        assert_eq!(hal.acks()[1].result, AckResult::BadArg);
        assert_eq!(mode::ceiling(fw.ram_mut()), Mode::Eco);
        hal.push_tc(7, &Command::SetMode(Mode::Nominal as u8));
        let r = run(&mut fw, &mut hal, 100);
        // Le maintien V >= 7560 compte depuis l'entrée en Eco (1 cycle avant cette série),
        // même plafond bloquant : 60 s au total -> cycle 59 de la série.
        assert_eq!(first_in(&r, Mode::Nominal), Some(59));
        // Opcodes hors périmètre T-005 : acceptés, acquittés Refused.
        hal.push_tc(8, &Command::Reboot);
        fw.step(&mut hal);
        assert_eq!(hal.acks()[3].result, AckResult::Refused);
        assert_eq!(hal.resets, 0);
    }

    fn rejected(fw: &mut Firmware) -> u16 {
        ram::CNT_TC_REJECTED.get(fw.ram_mut())
    }

    #[test]
    fn replayed_corrupted_truncated_frames_are_silently_rejected() {
        let (mut fw, mut hal) = booted();
        let ping = hal.push_tc(10, &Command::Ping);
        fw.step(&mut hal);
        assert_eq!(hal.acks().len(), 1);
        let sent_before = hal.sent.len();

        // Rejeu exact.
        hal.rx.push_back(ping.clone());
        let r = fw.step(&mut hal);
        assert_eq!(r.executed_tc, None);
        assert_eq!(rejected(&mut fw), 1);
        // SEQ plus ancien.
        hal.push_tc(9, &Command::Ping);
        fw.step(&mut hal);
        assert_eq!(rejected(&mut fw), 2);
        // Hors fenêtre.
        hal.push_tc(10 + 1025, &Command::Ping);
        fw.step(&mut hal);
        assert_eq!(rejected(&mut fw), 3);

        // Corruption : chaque bit de la trame, un par un.
        let fresh = {
            let mut out = [0u8; MAX_FRAME];
            let n = proto::encode_tc(11, &Command::Ping, &mut out);
            out[..n].to_vec()
        };
        let mut expected = 3;
        for bit in 0..fresh.len() * 8 {
            let mut f = fresh.clone();
            f[bit / 8] ^= 1 << (bit % 8);
            hal.rx.push_back(f);
            let r = fw.step(&mut hal);
            expected += 1;
            assert_eq!(r.executed_tc, None, "bit {bit} accepté");
            assert_eq!(rejected(&mut fw), expected);
        }
        // Troncature à toute longueur.
        for n in 0..fresh.len() {
            hal.rx.push_back(fresh[..n].to_vec());
            assert_eq!(fw.step(&mut hal).executed_tc, None);
            expected += 1;
            assert_eq!(rejected(&mut fw), expected);
        }
        // Mauvais type : un ACK valide renvoyé vers le bord.
        let mut out = [0u8; MAX_FRAME];
        let ack = proto::Ack {
            tc_seq: 11,
            opcode: 1,
            result: AckResult::Ok,
        };
        let n = proto::encode_ack(11, &ack, &mut out);
        hal.rx.push_back(out[..n].to_vec());
        fw.step(&mut hal);
        expected += 1;
        assert_eq!(rejected(&mut fw), expected);

        // Aucun ACK émis pour toutes ces trames ; last_tc_seq inchangé.
        assert_eq!(hal.acks().len(), 1);
        assert!(hal.sent[sent_before..]
            .iter()
            .all(|f| proto::decode_frame(f).unwrap().ftype != proto::FrameType::Ack));
        assert_eq!(ram::LAST_TC_SEQ.get(fw.ram_mut()), 10);
        // La TC intacte passe toujours.
        hal.rx.push_back(fresh);
        assert_eq!(fw.step(&mut hal).executed_tc, Some((11, 1)));
    }

    #[test]
    fn at_most_two_frames_and_one_tc_per_tick() {
        let (mut fw, mut hal) = booted();
        hal.rx.push_back([0u8; 3].to_vec());
        hal.rx.push_back([0u8; 3].to_vec());
        hal.rx.push_back([0u8; 3].to_vec());
        fw.step(&mut hal);
        assert_eq!(hal.rx.len(), 1);
        assert_eq!(rejected(&mut fw), 2);
        hal.rx.clear();
        hal.push_tc(1, &Command::Ping);
        hal.push_tc(2, &Command::Ping);
        assert_eq!(fw.step(&mut hal).executed_tc, Some((1, 1)));
        assert_eq!(fw.step(&mut hal).executed_tc, Some((2, 1)));
    }

    #[test]
    fn watchdog_kicked_each_cycle_with_alternating_keys() {
        let (mut fw, mut hal) = booted();
        let r = run(&mut fw, &mut hal, 1000);
        assert!(r.iter().all(|x| x.kicked));
        assert_eq!(hal.kicks.len(), 1000);
        assert_eq!(hal.kicks[0], WDG_KEY_A);
        for (i, k) in hal.kicks.iter().enumerate() {
            assert_eq!(*k, if i % 2 == 0 { WDG_KEY_A } else { WDG_KEY_B });
        }
    }

    #[test]
    fn no_kick_if_a_critical_flag_is_missing() {
        for missing in [FLAG_HEALTH, FLAG_POWER, FLAG_FDIR, FLAG_THERMAL] {
            let (mut fw, mut hal) = booted();
            run(&mut fw, &mut hal, 5);
            let kicks = hal.kicks.len();
            let ram = fw.ram_mut();
            // Cycle reconstitué à la main : toutes les tâches sauf une ont posé leur drapeau.
            begin_cycle(ram);
            tasks::health::run(ram, &mut hal);
            tasks::power::run(ram, &mut hal);
            tasks::fdir::run(ram, &mut hal);
            tasks::thermal::run(ram, &mut hal);
            let f = ram::EXEC_FLAGS.get(ram) & !missing;
            ram::EXEC_FLAGS.set(ram, f);
            assert!(!finish_cycle(ram, &mut hal), "drapeau {missing:#x}");
            assert_eq!(hal.kicks.len(), kicks);
            // Les tâches non critiques ne suffisent pas.
            begin_cycle(ram);
            ram::EXEC_FLAGS.set(ram, !CRITICAL_FLAGS | (CRITICAL_FLAGS & !missing));
            let hb = ram::HB_SEEN.get(ram).wrapping_add(1);
            ram::FDIR_HEARTBEAT.set(ram, hb);
            assert!(!finish_cycle(ram, &mut hal));
            // Reprise : la clé n'a pas été consommée, l'alternance est conservée.
            let r = fw.step(&mut hal);
            assert!(r.kicked);
            let n = hal.kicks.len();
            assert_ne!(hal.kicks[n - 1], hal.kicks[n - 2]);
        }
    }

    #[test]
    fn no_kick_if_fdir_heartbeat_frozen_or_jumps() {
        let (mut fw, mut hal) = booted();
        run(&mut fw, &mut hal, 5);
        let ram = fw.ram_mut();
        // Battement gelé : toutes les tâches tournent, mais le battement ne bouge pas.
        begin_cycle(ram);
        ram::EXEC_FLAGS.set(ram, 0xFF);
        assert!(!finish_cycle(ram, &mut hal));
        // Saut corrompu (bit basculé dans le battement).
        let hb = ram::FDIR_HEARTBEAT.get(ram);
        ram::FDIR_HEARTBEAT.set(ram, hb ^ 0x0100);
        let r = fw.step(&mut hal);
        assert!(!r.kicked);
        // Le cycle suivant se recale tout seul.
        assert!(fw.step(&mut hal).kicked);
    }

    #[test]
    fn health_rewrites_altered_registers() {
        let (mut fw, mut hal) = booted();
        run(&mut fw, &mut hal, 3);
        assert_eq!(ram::CNT_REG_FIX.get(fw.ram_mut()), 0);
        hal.regs[Reg::WdgCtrl as usize] ^= 1;
        hal.regs[Reg::RadioCfg as usize] = 0;
        hal.regs[Reg::LoadSwitches as usize] = 0; // radio coupée par un SEU
        fw.step(&mut hal);
        assert_eq!(hal.regs[Reg::WdgCtrl as usize], WDG_CTRL_VALUE);
        assert_eq!(hal.regs[Reg::RadioCfg as usize], RADIO_CFG_VALUE);
        assert!(hal.line_on(Line::Radio.mask()));
        assert_eq!(ram::CNT_REG_FIX.get(fw.ram_mut()), 3);
    }

    #[test]
    fn beacon_period_follows_mode() {
        let (mut fw, mut hal) = booted();
        let count = |hal: &MockHal| hal.beacons().len();
        // Survie, 119 cycles : balises aux cycles 1 et 61 (Eco au cycle 120).
        run(&mut fw, &mut hal, 119);
        assert_eq!(count(&hal), 2);
        // Eco 60 s puis Nominal.
        run(&mut fw, &mut hal, 61);
        assert_eq!(mode::current(fw.ram_mut()), Mode::Nominal);
        let before = count(&hal);
        run(&mut fw, &mut hal, 100);
        assert_eq!(count(&hal) - before, 10);
        let b = *hal.beacons().last().unwrap();
        assert_eq!(b.mode, Mode::Nominal as u8);
        assert_eq!(
            b.line_state as u16 & Line::Payload.mask(),
            Line::Payload.mask()
        );
    }

    /// R13 : un compteur de balise corrompu ne fait pas taire la radio.
    #[test]
    fn corrupted_beacon_counter_does_not_silence_radio() {
        for v in [u16::MAX, 0x8000, 61, 60, 59] {
            let (mut fw, mut hal) = booted();
            fw.step(&mut hal);
            let before = hal.beacons().len();
            ram::BEACON_ELAPSED.set(fw.ram_mut(), v);
            run(&mut fw, &mut hal, 60);
            assert!(hal.beacons().len() > before, "compteur {v:#x}");
        }
    }

    #[test]
    fn beacon_retried_after_send_failure() {
        let (mut fw, mut hal) = booted();
        hal.send_result = Err(HalError::Busy);
        run(&mut fw, &mut hal, 5);
        assert!(hal.sent.is_empty());
        hal.send_result = Ok(());
        fw.step(&mut hal);
        assert_eq!(hal.beacons().len(), 1);
    }

    #[test]
    fn nominal_20000_cycles_without_panic() {
        let seed = 0x5EED_0005;
        println!("graine bruit capteurs : {seed:#x}");
        let (mut fw, mut hal) = booted();
        hal.noise = 5;
        hal.rng = TestRng::new(seed);
        let mut seq = 0u32;
        let mut acked = 0;
        for k in 0..20_000u32 {
            if k % 60 == 0 {
                seq += 1;
                hal.push_tc(seq, &Command::Ping);
            }
            let r = fw.step(&mut hal);
            assert!(r.kicked, "cycle {k}");
            if r.executed_tc.is_some() {
                acked += 1;
            }
        }
        assert_eq!(acked, seq);
        assert_eq!(hal.acks().len() as u32, seq);
        assert_eq!(mode::current(fw.ram_mut()), Mode::Nominal);
        assert_eq!(ram::UPTIME.get(fw.ram_mut()), 20_000);
        assert_eq!(ram::CNT_TC_REJECTED.get(fw.ram_mut()), 0);
        assert_eq!(hal.resets, 0);
    }

    /// Corruption volontaire massive : RAM entièrement aléatoire, puis bits basculés à chaque
    /// cycle. Exigence : jamais de `panic`, et le watchdog continue d'être rechargé la plupart
    /// du temps avec une clé valide.
    #[test]
    fn random_ram_never_panics() {
        let seed = 0xA361_5005;
        println!("graine corruption RAM : {seed:#x}");
        let mut rng = TestRng::new(seed);
        for run_i in 0..200 {
            let mut fw = Firmware::new();
            let mut hal = MockHal::nominal();
            fw.boot(&mut hal);
            for b in fw.ram_mut().iter_mut() {
                *b = rng.next_u64() as u8;
            }
            for _ in 0..300 {
                if rng.below(4) == 0 {
                    let bit = rng.below((RAM_SIZE * 8) as u64) as usize;
                    fw.ram_mut()[bit / 8] ^= 1 << (bit % 8);
                }
                if rng.below(10) == 0 {
                    let s = SensorId::ALL[rng.below(9) as usize];
                    hal.set(s, Ok(rng.next_u64() as i16));
                }
                if rng.below(20) == 0 {
                    let len = rng.below(MAX_FRAME as u64 + 1) as usize;
                    let f: Vec<u8> = (0..len).map(|_| rng.next_u64() as u8).collect();
                    hal.rx.push_back(f);
                }
                fw.step(&mut hal);
            }
            assert!(
                hal.kicks.iter().all(|&k| k == WDG_KEY_A || k == WDG_KEY_B),
                "run {run_i}"
            );
            assert!(
                hal.kicks.len() > 200,
                "run {run_i} : trop peu de rechargements"
            );
            assert!(
                hal.line_on(Line::Radio.mask()),
                "run {run_i} : radio coupée"
            );
        }
    }

    /// Chaque bit de chaque cellule basculé une fois en régime nominal : le firmware ne panique
    /// pas, garde la radio, et continue d'émettre des balises (vivant et visible).
    #[test]
    fn every_slot_bit_flip_keeps_radio_and_beacons() {
        for s in ram::SLOTS {
            for bit in 0..s.len * 8 {
                let (mut fw, mut hal) = booted();
                run(&mut fw, &mut hal, 200);
                fw.ram_mut()[s.start + bit / 8] ^= 1 << (bit % 8);
                let before = hal.beacons().len();
                run(&mut fw, &mut hal, 120);
                assert!(hal.line_on(Line::Radio.mask()), "{} bit {bit}", s.name);
                assert!(hal.beacons().len() > before, "{} bit {bit}", s.name);
            }
        }
    }
}
