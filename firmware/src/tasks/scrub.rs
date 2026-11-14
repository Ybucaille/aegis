//! Tâche 7 `scrub` (non critique). BOUCHON T-005 : pose seulement son drapeau.
//! Vote TMR et correction SECDED en tourniquet : T-010.

use crate::hal::Hal;
use crate::ram::Ram;
use crate::sched;

pub fn run<H: Hal>(ram: &mut Ram, _hal: &mut H) {
    sched::set_flag(ram, sched::FLAG_SCRUB);
}
