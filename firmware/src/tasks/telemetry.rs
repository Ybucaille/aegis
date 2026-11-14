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
//! `line_state` = registre `LoadSwitches` RELU (état réel, pas l'état commandé). Tous les
//! champs de la balise (28 octets) viennent de cellules réelles : `boot_count` relu de la NVM
//! (T-011), `fdir_flags` porte `NVM_LOST`, compteurs de réparation, rejets, latch-up (T-012).
//!
//! Journal (`DumpEvents`) : une trame EVENTS par cycle tant qu'un vidage est en cours (format
//! dans `log`), sans nouvel essai en cas d'échec d'émission.
//!
//! `Reboot` (SPEC §5 : `sys_reset` APRÈS l'ACK) : l'ACK est émis en premier dans ce cycle, puis
//! `sys_reset` si `REBOOT_PENDING` est non nul. Si une écriture NVM est en attente (SEQ du
//! `Reboot` pas encore persisté), elle est retentée d'abord ; le reset attend au plus
//! `REBOOT_NVM_TRIES` cycles une NVM à jour, puis a lieu quand même (NVM en panne permanente :
//! le reset ne doit pas être bloqué).

use crate::hal::{Hal, Reg, MAX_FRAME};
use crate::log::{self, ev};
use crate::mode;
use crate::nvm;
use crate::proto::{self, Ack, AckResult, Beacon, MAX_PAYLOAD};
use crate::ram::{self, Ram};
use crate::sched;
use crate::tasks::comms_rx::REBOOT_NVM_TRIES;

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

/// Une trame EVENTS si un vidage du journal est en cours.
fn events_due<H: Hal>(ram: &mut Ram, hal: &mut H) {
    let mut p = [0u8; MAX_PAYLOAD];
    let n = match log::dump_next(ram, &mut p) {
        Some(n) => n,
        None => return,
    };
    let seq = next_tm_seq(ram);
    let mut out = [0u8; MAX_FRAME];
    if let Ok(len) = proto::encode_events(seq, &p[..n], &mut out) {
        let _ = hal.radio_send(&out[..len]);
    }
}

/// `sys_reset` différé d'un `Reboot` accepté (après l'ACK, émis plus haut dans ce cycle).
fn reboot_due<H: Hal>(ram: &mut Ram, hal: &mut H) {
    let tries = ram::REBOOT_PENDING.get(ram).min(REBOOT_NVM_TRIES);
    if tries == 0 {
        return;
    }
    let nvm_ok = nvm::service(ram, hal);
    if nvm_ok || tries == 1 {
        ram::REBOOT_PENDING.set(ram, 0);
        log::push(ram, ev::REBOOT_RESET, nvm_ok as u8, 0);
        hal.sys_reset();
    } else {
        ram::REBOOT_PENDING.set(ram, tries - 1);
    }
}

pub fn run<H: Hal>(ram: &mut Ram, hal: &mut H) {
    send_ack(ram, hal);
    beacon_due(ram, hal);
    events_due(ram, hal);
    reboot_due(ram, hal);
    sched::set_flag(ram, sched::FLAG_TELEMETRY);
}
