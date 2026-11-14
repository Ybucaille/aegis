//! Tâche 6 `telemetry` (non critique) : ACK en attente, puis balise périodique (SPEC §4.2, §5).
//!
//! ACK : émis une seule fois (succès ou échec de `radio_send`, pas de nouvel essai : le sol
//! relance un `Ping` toutes les 60 s). Un ACK n'est émis que si `ACK_PENDING` vaut exactement
//! `ACK_PENDING_YES`.
//!
//! Balise : période du mode (10 / 30 / 60 s). Compteur `BEACON_ELAPSED` lu BORNÉ
//! (`min(valeur, période)`, R13) : un compteur corrompu avance l'émission, il ne la supprime
//! jamais. En cas d'échec d'émission, le compteur reste à la période : nouvel essai au cycle
//! suivant. Au boot le compteur vaut `u16::MAX` : première balise au premier cycle.
//! `line_state` = registre `LoadSwitches` RELU (état réel, pas l'état commandé).
//! Le journal (`DumpEvents`) est T-011.

use crate::hal::{Hal, Reg, MAX_FRAME};
use crate::mode;
use crate::proto::{self, Ack, AckResult, Beacon};
use crate::ram::{self, Ram};
use crate::sched;

fn next_tm_seq(ram: &mut Ram) -> u32 {
    let s = ram::TM_SEQ.get(ram).wrapping_add(1);
    ram::TM_SEQ.set(ram, s);
    s
}

fn send_ack<H: Hal>(ram: &mut Ram, hal: &mut H) {
    if ram::ACK_PENDING.get(ram) != ram::ACK_PENDING_YES {
        return;
    }
    ram::ACK_PENDING.set(ram, 0);
    let ack = Ack {
        tc_seq: ram::ACK_SEQ.get(ram),
        opcode: ram::ACK_OPCODE.get(ram),
        // Résultat corrompu hors domaine : on annonce `Refused` plutôt qu'un faux `Ok`.
        result: AckResult::from_u8(ram::ACK_RESULT.get(ram)).unwrap_or(AckResult::Refused),
    };
    let seq = next_tm_seq(ram);
    let mut out = [0u8; MAX_FRAME];
    let n = proto::encode_ack(seq, &ack, &mut out);
    let _ = hal.radio_send(&out[..n]);
}

/// Construit la balise à partir des cellules.
pub fn beacon<H: Hal>(ram: &mut Ram, hal: &mut H) -> Beacon {
    Beacon {
        uptime_s: ram::UPTIME.get(ram),
        boot_count: ram::BOOT_COUNT.get(ram),
        reset_cause: ram::RESET_CAUSE.get(ram),
        mode: mode::current(ram) as u8,
        vbatt_mv: ram::VBATT_MV.get(ram),
        temp_batt_dc: ram::TBATT_DC.get(ram),
        last_tc_seq: ram::LAST_TC_SEQ.get(ram),
        fdir_flags: ram::FDIR_FLAGS.get(ram),
        sensor_fault_mask: ram::SENSOR_FAULT_MASK.get(ram),
        line_state: (hal.reg_read(Reg::LoadSwitches) & 0xFF) as u8,
        cnt_ecc_corr: ram::CNT_ECC_CORR.get(ram),
        cnt_tmr_fix: ram::CNT_TMR_FIX.get(ram),
        cnt_tc_rejected: ram::CNT_TC_REJECTED.get(ram),
        cnt_latchup: ram::CNT_LATCHUP.get(ram),
    }
}

fn beacon_due<H: Hal>(ram: &mut Ram, hal: &mut H) {
    let period = mode::beacon_period(mode::current(ram));
    let elapsed = ram::bounded(ram::BEACON_ELAPSED.get(ram), period);
    let elapsed = if elapsed < period {
        elapsed + 1
    } else {
        period
    };
    if elapsed >= period {
        let b = beacon(ram, hal);
        let seq = next_tm_seq(ram);
        let mut out = [0u8; MAX_FRAME];
        let n = proto::encode_beacon(seq, &b, &mut out);
        let sent = hal.radio_send(&out[..n]).is_ok();
        ram::BEACON_ELAPSED.set(ram, if sent { 0 } else { period });
    } else {
        ram::BEACON_ELAPSED.set(ram, elapsed);
    }
}

pub fn run<H: Hal>(ram: &mut Ram, hal: &mut H) {
    send_ack(ram, hal);
    beacon_due(ram, hal);
    sched::set_flag(ram, sched::FLAG_TELEMETRY);
}
