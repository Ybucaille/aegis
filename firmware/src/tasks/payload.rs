//! Tâche 8 `payload` (non critique) : charge utile factice. Appelée seulement en `Nominal`.
//! Lit le courant de sa ligne et compte les échantillons acquis.

use crate::hal::{Hal, SensorId};
use crate::ram::{self, Ram};
use crate::sched;

pub fn run<H: Hal>(ram: &mut Ram, hal: &mut H) {
    if hal.sensor(SensorId::ILinePayload).is_ok() {
        let n = ram::PAYLOAD_SAMPLES.get(ram).wrapping_add(1);
        ram::PAYLOAD_SAMPLES.set(ram, n);
    }
    sched::set_flag(ram, sched::FLAG_PAYLOAD);
}
