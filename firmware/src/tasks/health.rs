//! Tâche 1 `health` (critique) : rafraîchissement systématique des registres (SPEC §4.3).
//!
//! `WdgTimeout`, `WdgCtrl`, `RadioCfg` : relus, comptés s'ils diffèrent, puis réécrits à chaque
//! cycle. `LoadSwitches` : relu et comparé à la valeur recalculée par `mode::commanded_lines` ;
//! réécrit et compté s'il diffère. T-012 complète (feature `unprotected`).

use crate::hal::{Hal, Reg};
use crate::mode;
use crate::ram::{self, Ram};
use crate::sched;
use crate::{RADIO_CFG_VALUE, WDG_CTRL_VALUE, WDG_TIMEOUT_S};

/// Valeurs attendues des registres de configuration (ROM).
pub const CONFIG_REGS: [(Reg, u16); 3] = [
    (Reg::WdgTimeout, WDG_TIMEOUT_S),
    (Reg::WdgCtrl, WDG_CTRL_VALUE),
    (Reg::RadioCfg, RADIO_CFG_VALUE),
];

fn count_fix(ram: &mut Ram) {
    let n = ram::CNT_REG_FIX.get(ram).saturating_add(1);
    ram::CNT_REG_FIX.set(ram, n);
}

pub fn run<H: Hal>(ram: &mut Ram, hal: &mut H) {
    for (r, v) in CONFIG_REGS {
        if hal.reg_read(r) != v {
            count_fix(ram);
        }
        hal.reg_write(r, v);
    }
    let expected = mode::commanded_lines(ram);
    if hal.reg_read(Reg::LoadSwitches) != expected {
        count_fix(ram);
        hal.reg_write(Reg::LoadSwitches, expected);
    }
    sched::set_flag(ram, sched::FLAG_HEALTH);
}
