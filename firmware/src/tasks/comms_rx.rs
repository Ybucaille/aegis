//! Tâche 5 `comms_rx` (non critique) : réception, validation et exécution des TC (SPEC §5).
//!
//! Au début de chaque cycle : écriture NVM en attente (`nvm::service`, demandée par
//! `nvm::request_save` ou par le FDIR, ou échec d'écriture précédent).
//!
//! Validation, dans l'ordre : SYNC, version/type (TC uniquement), LEN, CRC (`proto`), puis
//! anti-rejeu `last_tc_seq < SEQ <= last_tc_seq + 1024` (arithmétique sans débordement : un
//! `last_tc_seq` proche de `u32::MAX` ne panique pas, il ferme seulement la fenêtre), puis
//! décodage de la charge utile (opcode connu, longueur exacte). Toute trame rejetée : silence
//! radio, `CNT_TC_REJECTED += 1` (saturant).
//!
//! À l'acceptation : `LAST_TC_SEQ = SEQ` en TMR PUIS écriture NVM des trois copies, AVANT
//! l'exécution. Si l'écriture NVM échoue (`HalError`), la TC est quand même exécutée (le SEQ
//! est en TMR) et l'écriture est retentée à chaque cycle ; seul `Reboot` attend la NVM (voir
//! `telemetry`).
//!
//! Au plus `MAX_FRAMES_PER_TICK` trames lues par cycle, et au plus UNE TC acceptée par cycle :
//! la lecture s'arrête après une acceptation. Raison : `StepReport::executed_tc` ne peut porter
//! qu'une TC (SPEC §3.1) et l'oracle S5 doit les voir toutes.
//!
//! | TC | Effet | ACK |
//! |---|---|---|
//! | `Ping` | — | `Ok` |
//! | `SetMode(m)` | plafond = m, persisté en NVM | `Ok` ; `BadArg` si m hors domaine |
//! | `Reboot` | `REBOOT_PENDING` ; `sys_reset` émis par `telemetry` APRÈS l'ACK | `Ok` |
//! | `PowerCycle(l)` | `mode::start_power_cycle` (T-012) : l coupée 5 s puis réarmée (Radio comprise) | `Ok` ; `BadArg` si l hors domaine |
//! | `ClearFdir` | `fdir::clear_fdir` (T-012), puis écriture NVM immédiate | `Ok` |
//! | `DumpEvents` | vidage du journal (trames EVENTS, une par cycle, voir `log`) | `Ok` |
//!
//! `PowerCycle` : état (`PWRCYC_LINES`, `PWRCYC_LEFT`), décompte (`sched::begin_cycle`) et
//! retrait des lignes coupées (`mode::commanded_lines`) appartiennent à `mode` (T-012).

use crate::hal::{Hal, Line, MAX_FRAME};
use crate::log::{self, ev};
use crate::mode;
use crate::nvm;
use crate::proto::{self, AckResult, Command, FrameType};
use crate::ram::{self, Ram};
use crate::sched;
use crate::tasks::fdir;
use crate::Mode;

pub const MAX_FRAMES_PER_TICK: usize = 2;
/// Valeur initiale de `REBOOT_PENDING` : cycles pendant lesquels `telemetry` attend que la
/// NVM soit à jour avant d'émettre `sys_reset` (au-delà, reset quand même).
pub const REBOOT_NVM_TRIES: u8 = 3;

/// Anti-rejeu : `last < seq <= last + ANTI_REPLAY_WINDOW`, sans arithmétique débordante.
pub fn in_window(last: u32, seq: u32) -> bool {
    seq > last && seq - last <= proto::ANTI_REPLAY_WINDOW
}

/// Valide une trame reçue. `Some((seq, cmd))` si elle est acceptable comme TC.
pub fn validate(last_tc_seq: u32, bytes: &[u8]) -> Option<(u32, Command)> {
    let f = proto::decode_frame(bytes).ok()?;
    if f.ftype != FrameType::Tc || !in_window(last_tc_seq, f.seq) {
        return None;
    }
    let cmd = Command::decode(f.payload).ok()?;
    Some((f.seq, cmd))
}

fn execute<H: Hal>(ram: &mut Ram, hal: &mut H, cmd: &Command) -> AckResult {
    match *cmd {
        Command::Ping => AckResult::Ok,
        Command::SetMode(m) => match Mode::from_u8(m) {
            Some(m) => {
                ram::CEILING.set(ram, m as u8);
                log::push(ram, ev::CEILING_SET, m as u8, 0);
                nvm::save(ram, hal);
                AckResult::Ok
            }
            None => AckResult::BadArg,
        },
        Command::Reboot => {
            ram::REBOOT_PENDING.set(ram, REBOOT_NVM_TRIES);
            log::push(ram, ev::REBOOT_CMD, 0, 0);
            AckResult::Ok
        }
        Command::PowerCycle(l) => match Line::from_u8(l) {
            Some(line) => {
                mode::start_power_cycle(ram, hal, line);
                log::push(ram, ev::POWER_CYCLE, l, 0);
                AckResult::Ok
            }
            None => AckResult::BadArg,
        },
        Command::ClearFdir => {
            // Lève isolations et verrou de tempête (T-012) ; NVM réécrite tout de suite pour
            // persister les isolations levées et le compteur de boots courts remis à 0.
            fdir::clear_fdir(ram);
            log::push(ram, ev::CLEAR_FDIR, 0, 0);
            nvm::save(ram, hal);
            AckResult::Ok
        }
        Command::DumpEvents => {
            log::start_dump(ram);
            AckResult::Ok
        }
    }
}

/// Renvoie la TC exécutée ce cycle `(seq, opcode)`.
pub fn run<H: Hal>(ram: &mut Ram, hal: &mut H) -> Option<(u32, u8)> {
    nvm::service(ram, hal);
    let mut executed = None;
    for _ in 0..MAX_FRAMES_PER_TICK {
        let mut buf = [0u8; MAX_FRAME];
        let n = match hal.radio_recv(&mut buf) {
            Some(n) => n.min(MAX_FRAME),
            None => break,
        };
        match validate(ram::LAST_TC_SEQ.get(ram), &buf[..n]) {
            Some((seq, cmd)) => {
                // Anti-rejeu persistant : SEQ en TMR et en NVM AVANT l'exécution.
                ram::LAST_TC_SEQ.set(ram, seq);
                nvm::save(ram, hal);
                // `seq as u16` : 16 bits de poids faible, voulu (format du journal). Journalisé
                // avant l'exécution : un `DumpEvents` contient sa propre entrée.
                log::push(ram, ev::TC_EXEC, cmd.opcode(), seq as u16);
                let result = execute(ram, hal, &cmd);
                ram::ACK_SEQ.set(ram, seq);
                ram::ACK_OPCODE.set(ram, cmd.opcode());
                ram::ACK_RESULT.set(ram, result as u8);
                ram::ACK_PENDING.set(ram, ram::ACK_PENDING_YES);
                executed = Some((seq, cmd.opcode()));
                break;
            }
            None => {
                let c = ram::CNT_TC_REJECTED.get(ram).saturating_add(1);
                ram::CNT_TC_REJECTED.set(ram, c);
            }
        }
    }
    sched::set_flag(ram, sched::FLAG_COMMS_RX);
    executed
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::hal::NVM_SIZE;
    use crate::log::decode_events;
    use crate::nvm::testkit::{boot_full, Call, TraceHal};
    use crate::nvm::{Persist, BLOCK_LEN, COPY_OFF};
    use crate::proto::{opcode, Ack};
    use crate::testhal::MockHal;
    use crate::Firmware;
    use std::vec::Vec;

    fn booted() -> (Firmware, MockHal) {
        let mut fw = Firmware::new();
        let mut hal = MockHal::nominal();
        boot_full(&mut fw, &mut hal);
        (fw, hal)
    }

    fn nvm_copy0(nvm: &[u8; NVM_SIZE]) -> Option<Persist> {
        let mut b = [0u8; BLOCK_LEN];
        b.copy_from_slice(&nvm[..BLOCK_LEN]);
        Persist::decode(&b)
    }

    fn rejected(fw: &mut Firmware) -> u16 {
        ram::CNT_TC_REJECTED.get(fw.ram_mut())
    }

    #[test]
    fn window_edges() {
        assert!(!in_window(10, 10));
        assert!(!in_window(10, 9));
        assert!(in_window(10, 11));
        assert!(in_window(10, 10 + 1024));
        assert!(!in_window(10, 10 + 1025));
        assert!(in_window(0, 1));
        // Saturation : pas de débordement près de u32::MAX.
        assert!(in_window(u32::MAX - 5, u32::MAX));
        assert!(!in_window(u32::MAX - 5, 3));
        assert!(!in_window(u32::MAX, 0));
        assert!(!in_window(u32::MAX, u32::MAX));
    }

    /// Chaque opcode -> ACK attendu (`Ok`, `BadArg` sur argument invalide).
    #[test]
    fn every_opcode_gets_expected_ack() {
        let cases: [(Command, AckResult); 10] = [
            (Command::Ping, AckResult::Ok),
            (Command::SetMode(Mode::Eco as u8), AckResult::Ok),
            (Command::SetMode(3), AckResult::BadArg),
            (Command::SetMode(0xFF), AckResult::BadArg),
            (Command::PowerCycle(Line::Heater as u8), AckResult::Ok),
            (Command::PowerCycle(3), AckResult::BadArg),
            (Command::PowerCycle(0xFF), AckResult::BadArg),
            (Command::ClearFdir, AckResult::Ok),
            (Command::DumpEvents, AckResult::Ok),
            (Command::Reboot, AckResult::Ok),
        ];
        let (mut fw, mut hal) = booted();
        for (i, (cmd, want)) in cases.iter().enumerate() {
            let seq = 1 + i as u32;
            hal.push_tc(seq, cmd);
            let r = fw.step(&mut hal);
            assert_eq!(r.executed_tc, Some((seq, cmd.opcode())), "{cmd:?}");
            let a = *hal.acks().last().unwrap();
            assert_eq!(
                a,
                Ack {
                    tc_seq: seq,
                    opcode: cmd.opcode(),
                    result: *want
                },
                "{cmd:?}"
            );
        }
        assert_eq!(hal.acks().len(), cases.len());
        assert_eq!(rejected(&mut fw), 0);
        // BadArg ne change pas le plafond.
        assert_eq!(mode::ceiling(fw.ram_mut()), Mode::Eco);
        assert_eq!(hal.resets, 1);
    }

    /// Rejeu d'une TC acceptée APRÈS un reset -> rejetée (anti-rejeu persistant).
    #[test]
    fn replay_after_reset_is_rejected() {
        let (mut fw, mut hal) = booted();
        let frame = hal.push_tc(5, &Command::SetMode(Mode::Eco as u8));
        assert!(fw.step(&mut hal).executed_tc.is_some());
        assert_eq!(nvm_copy0(&hal.nvm).unwrap().last_tc_seq, 5);
        // Reset (RAM perdue), puis rejeu de la même trame.
        hal.reset_cause = crate::ResetCause::Watchdog;
        boot_full(&mut fw, &mut hal);
        assert_eq!(ram::LAST_TC_SEQ.get(fw.ram_mut()), 5);
        let acks = hal.acks().len();
        hal.rx.push_back(frame);
        assert_eq!(fw.step(&mut hal).executed_tc, None);
        assert_eq!(hal.acks().len(), acks);
        assert_eq!(rejected(&mut fw), 1);
        // Le plafond aussi a survécu au reset.
        assert_eq!(mode::ceiling(fw.ram_mut()), Mode::Eco);
        // La suivante passe.
        hal.push_tc(6, &Command::Ping);
        assert_eq!(fw.step(&mut hal).executed_tc, Some((6, opcode::PING)));
        // Balise : last_tc_seq publié.
        assert!(hal.beacons().iter().any(|b| b.last_tc_seq == 5));
    }

    #[test]
    fn seq_window_upper_edge() {
        let (mut fw, mut hal) = booted();
        hal.push_tc(1025, &Command::Ping);
        assert_eq!(fw.step(&mut hal).executed_tc, None);
        assert_eq!(rejected(&mut fw), 1);
        hal.push_tc(1024, &Command::Ping);
        assert_eq!(fw.step(&mut hal).executed_tc, Some((1024, opcode::PING)));
        hal.push_tc(1024 + 1025, &Command::Ping);
        assert_eq!(fw.step(&mut hal).executed_tc, None);
        assert_eq!(rejected(&mut fw), 2);
    }

    /// R9 : `last_tc_seq` = `u32::MAX - 5` (relu de la NVM) puis TC quelconques -> rejet sans
    /// panique ; seules les 5 dernières valeurs restent acceptables.
    #[test]
    fn saturated_last_tc_seq_rejects_without_panic() {
        let mut hal = MockHal::nominal();
        let b = Persist {
            last_tc_seq: u32::MAX - 5,
            ..Persist::DEFAULT
        }
        .encode();
        for &o in &COPY_OFF {
            hal.nvm[o as usize..o as usize + BLOCK_LEN].copy_from_slice(&b);
        }
        let mut fw = Firmware::new();
        boot_full(&mut fw, &mut hal);
        assert_eq!(ram::LAST_TC_SEQ.get(fw.ram_mut()), u32::MAX - 5);
        let last = u32::MAX - 5;
        for seq in [
            0,
            1,
            1024,
            last,
            last.wrapping_add(1025),
            last.wrapping_add(2000),
        ] {
            hal.push_tc(seq, &Command::Ping);
            assert_eq!(fw.step(&mut hal).executed_tc, None, "seq {seq}");
        }
        assert_eq!(rejected(&mut fw), 6);
        hal.push_tc(u32::MAX, &Command::Ping);
        assert_eq!(
            fw.step(&mut hal).executed_tc,
            Some((u32::MAX, opcode::PING))
        );
        // Plus aucune TC possible : limite documentée (R9), sans panique.
        for seq in [u32::MAX, 0, 1] {
            hal.push_tc(seq, &Command::Ping);
            assert_eq!(fw.step(&mut hal).executed_tc, None);
        }
        for _ in 0..100 {
            fw.step(&mut hal);
        }
    }

    /// Le SEQ est en NVM AVANT l'exécution : la première écriture NVM après acceptation porte
    /// le nouveau SEQ et l'ANCIEN plafond.
    #[test]
    fn seq_is_persisted_before_execution() {
        let mut fw = Firmware::new();
        let mut hal = TraceHal::new();
        boot_full(&mut fw, &mut hal);
        // Premier cycle : écriture demandée au boot par le FDIR (boots courts).
        fw.step(&mut hal);
        hal.calls.clear();
        hal.inner.push_tc(9, &Command::SetMode(Mode::Survie as u8));
        fw.step(&mut hal);
        let writes: Vec<Persist> = hal
            .calls
            .iter()
            .filter_map(|c| match c {
                Call::NvmWrite(_, d) => {
                    let mut b = [0u8; BLOCK_LEN];
                    b.copy_from_slice(d);
                    Persist::decode(&b)
                }
                _ => None,
            })
            .collect();
        assert_eq!(writes.len(), 6, "3 copies avant, 3 copies après exécution");
        assert!(writes[..3]
            .iter()
            .all(|p| p.last_tc_seq == 9 && p.ceiling == Mode::Nominal));
        assert!(writes[3..]
            .iter()
            .all(|p| p.last_tc_seq == 9 && p.ceiling == Mode::Survie));
        // Toutes les écritures NVM précèdent l'ACK.
        let first_send = hal
            .calls
            .iter()
            .position(|c| matches!(c, Call::Send(_)))
            .unwrap();
        let last_write = hal
            .calls
            .iter()
            .rposition(|c| matches!(c, Call::NvmWrite(..)))
            .unwrap();
        assert!(last_write < first_send);
    }

    /// `Reboot` : ACK émis PUIS `sys_reset`, dans le même cycle.
    #[test]
    fn reboot_acks_then_resets() {
        let mut fw = Firmware::new();
        let mut hal = TraceHal::new();
        boot_full(&mut fw, &mut hal);
        fw.step(&mut hal);
        hal.calls.clear();
        hal.inner.push_tc(3, &Command::Reboot);
        let r = fw.step(&mut hal);
        assert_eq!(r.executed_tc, Some((3, opcode::REBOOT)));
        let ack_pos = hal
            .calls
            .iter()
            .position(|c| match c {
                Call::Send(f) => {
                    let fr = proto::decode_frame(f).unwrap();
                    fr.ftype == FrameType::Ack
                        && Ack::decode(fr.payload).unwrap().opcode == opcode::REBOOT
                }
                _ => false,
            })
            .expect("pas d'ACK");
        let reset_pos = hal.calls.iter().position(|c| *c == Call::Reset).unwrap();
        assert!(ack_pos < reset_pos);
        assert_eq!(hal.inner.resets, 1);
        assert_eq!(ram::REBOOT_PENDING.get(fw.ram_mut()), 0);
        // Un seul reset.
        fw.step(&mut hal);
        assert_eq!(hal.inner.resets, 1);
    }

    /// `Reboot` avec NVM en échec : le reset attend la NVM au plus `REBOOT_NVM_TRIES` cycles,
    /// puis a lieu quand même ; NVM revenue -> reset immédiat.
    #[test]
    fn reboot_waits_for_nvm_then_resets_anyway() {
        let mut fw = Firmware::new();
        let mut hal = TraceHal::new();
        boot_full(&mut fw, &mut hal);
        hal.nvm_write_fail = true;
        hal.inner.push_tc(3, &Command::Reboot);
        fw.step(&mut hal);
        assert_eq!(hal.inner.acks().len(), 1);
        let mut cycles = 1;
        while hal.inner.resets == 0 {
            fw.step(&mut hal);
            cycles += 1;
            assert!(cycles <= 10);
        }
        assert_eq!(cycles, REBOOT_NVM_TRIES as u32);
        // NVM revenue au 2e cycle : reset dès ce cycle.
        let mut fw = Firmware::new();
        let mut hal = TraceHal::new();
        boot_full(&mut fw, &mut hal);
        hal.nvm_write_fail = true;
        hal.inner.push_tc(3, &Command::Reboot);
        fw.step(&mut hal);
        assert_eq!(hal.inner.resets, 0);
        hal.nvm_write_fail = false;
        fw.step(&mut hal);
        assert_eq!(hal.inner.resets, 1);
        assert_eq!(nvm_copy0(&hal.inner.nvm).unwrap().last_tc_seq, 3);
    }

    /// `HalError` sur `nvm_write` à l'acceptation : pas de panique, TC exécutée, nouvel essai.
    #[test]
    fn nvm_write_error_on_accept_does_not_panic() {
        let mut fw = Firmware::new();
        let mut hal = TraceHal::new();
        boot_full(&mut fw, &mut hal);
        hal.nvm_write_fail = true;
        hal.inner.push_tc(4, &Command::SetMode(Mode::Eco as u8));
        assert!(fw.step(&mut hal).executed_tc.is_some());
        assert_eq!(hal.inner.acks()[0].result, AckResult::Ok);
        assert!(nvm::pending(fw.ram_mut()));
        hal.nvm_write_fail = false;
        fw.step(&mut hal);
        assert!(!nvm::pending(fw.ram_mut()));
        let p = nvm_copy0(&hal.inner.nvm).unwrap();
        assert_eq!((p.last_tc_seq, p.ceiling), (4, Mode::Eco));
    }

    /// `PowerCycle` : ligne coupée dès le cycle d'acceptation, réarmée après 5 cycles.
    #[test]
    fn power_cycle_cuts_for_five_seconds() {
        let (mut fw, mut hal) = booted();
        fw.step(&mut hal);
        assert!(hal.line_on(Line::Radio.mask()));
        hal.push_tc(1, &Command::PowerCycle(Line::Radio as u8));
        fw.step(&mut hal);
        let mut off = 0;
        while !hal.line_on(Line::Radio.mask()) {
            off += 1;
            assert!(off <= 10);
            fw.step(&mut hal);
        }
        assert_eq!(off, mode::PWRCYC_S as u32);
        assert_eq!(ram::PWRCYC_LINES.get(fw.ram_mut()), 0);
        let ram = fw.ram_mut();
        let found =
            (0..log::count(ram)).any(|k| log::read(ram, k).unwrap().code == ev::POWER_CYCLE);
        assert!(found);
    }

    /// `DumpEvents` : trames EVENTS, une par cycle, contenant `BOOT` puis les TC exécutées.
    #[test]
    fn dump_events_emits_log_frames() {
        let (mut fw, mut hal) = booted();
        for s in 1..=8 {
            hal.push_tc(s, &Command::Ping);
            fw.step(&mut hal);
        }
        hal.push_tc(9, &Command::DumpEvents);
        for _ in 0..5 {
            fw.step(&mut hal);
        }
        let frames: Vec<Vec<u8>> = hal
            .sent
            .iter()
            .filter_map(|f| {
                let fr = proto::decode_frame(f).ok()?;
                (fr.ftype == FrameType::Events).then(|| fr.payload.to_vec())
            })
            .collect();
        // NVM_FORMAT + BOOT + 9 TC_EXEC (celui du dump compris) = 11 entrées, 3 trames.
        let mut entries = Vec::new();
        for p in &frames {
            let (h, e) = decode_events(p).unwrap();
            entries.extend_from_slice(&e[..h.n as usize]);
        }
        assert_eq!(frames.len(), 3);
        assert_eq!(entries[0].code, ev::NVM_FORMAT);
        assert_eq!(entries[1].code, ev::BOOT);
        let tcs: Vec<u16> = entries
            .iter()
            .filter(|e| e.code == ev::TC_EXEC)
            .map(|e| e.b)
            .collect();
        assert_eq!(tcs, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(decode_events(&frames[2]).unwrap().0.left, 0);
    }
}
