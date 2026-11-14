//! Tâche 3 `fdir` (critique). BOUCHON T-005 : avance seulement le battement.
//! Détections et réactions (latch-up, capteurs figés, tempête de resets) : T-012.

use crate::hal::Hal;
use crate::ram::{self, Ram};
use crate::sched;

pub fn run<H: Hal>(ram: &mut Ram, _hal: &mut H) {
    let hb = ram::FDIR_HEARTBEAT.get(ram).wrapping_add(1);
    ram::FDIR_HEARTBEAT.set(ram, hb);
    sched::set_flag(ram, sched::FLAG_FDIR);
}
