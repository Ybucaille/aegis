//! Tâche 5 `comms_rx` (non critique) : réception, validation et exécution des TC (SPEC §5).
//!
//! Validation, dans l'ordre : SYNC, version/type (TC uniquement), LEN, CRC (`proto`), puis
//! anti-rejeu `last_tc_seq < SEQ <= last_tc_seq + 1024` (sans débordement), puis décodage de la
//! charge utile (opcode connu, longueur exacte). Toute trame rejetée : silence radio,
//! `CNT_TC_REJECTED += 1` (saturant). À l'acceptation : `LAST_TC_SEQ = SEQ` AVANT l'exécution.
//!
//! Au plus `MAX_FRAMES_PER_TICK` trames lues par cycle, et au plus UNE TC acceptée par cycle :
//! la lecture s'arrête après une acceptation. Raison : `StepReport::executed_tc` ne peut porter
//! qu'une TC (SPEC §3.1) et l'oracle S5 doit les voir toutes.
//!
//! Périmètre T-005 : `Ping` (ACK Ok) et `SetMode` (plafond ; ACK Ok, ou BadArg si mode hors
//! domaine). Les autres opcodes sont acceptés (SEQ consommé) et acquittés `Refused` jusqu'à
//! T-011.

use crate::hal::{Hal, MAX_FRAME};
use crate::proto::{self, AckResult, Command, FrameType};
use crate::ram::{self, Ram};
use crate::sched;
use crate::Mode;

pub const MAX_FRAMES_PER_TICK: usize = 2;

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

fn execute(ram: &mut Ram, cmd: &Command) -> AckResult {
    match *cmd {
        Command::Ping => AckResult::Ok,
        Command::SetMode(m) => match Mode::from_u8(m) {
            Some(m) => {
                ram::CEILING.set(ram, m as u8);
                AckResult::Ok
            }
            None => AckResult::BadArg,
        },
        Command::Reboot | Command::PowerCycle(_) | Command::ClearFdir | Command::DumpEvents => {
            AckResult::Refused
        }
    }
}

/// Renvoie la TC exécutée ce cycle `(seq, opcode)`.
pub fn run<H: Hal>(ram: &mut Ram, hal: &mut H) -> Option<(u32, u8)> {
    let mut executed = None;
    for _ in 0..MAX_FRAMES_PER_TICK {
        let mut buf = [0u8; MAX_FRAME];
        let n = match hal.radio_recv(&mut buf) {
            Some(n) => n.min(MAX_FRAME),
            None => break,
        };
        match validate(ram::LAST_TC_SEQ.get(ram), &buf[..n]) {
            Some((seq, cmd)) => {
                ram::LAST_TC_SEQ.set(ram, seq);
                let result = execute(ram, &cmd);
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
    use super::*;

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
}
