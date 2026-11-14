//! Tâche 4 `thermal` (critique) : thermostat du réchauffeur batterie (annexe A).
//!
//! `Nominal`/`Eco` : ON sous +2 °C, OFF au-dessus de +8 °C ; `Survie` : 0 / +5 °C. Entre les
//! deux seuils, l'état précédent est conservé (hystérésis, cellule `HEATER_ON`).
//! Aucune température valide : réchauffeur COUPÉ (choix T-005 : sans mesure, chauffer en
//! continu peut dépasser +50 °C au soleil ; le froid d'éclipse reste un risque, à revoir en
//! T-012). La température vient de `power` (critique, même cycle).

use crate::hal::Hal;
use crate::mode;
use crate::ram::{self, Ram};
use crate::sched;

pub fn run<H: Hal>(ram: &mut Ram, hal: &mut H) {
    let m = mode::current(ram);
    let (on_below, off_above) = mode::thermostat(m);
    let heater = if ram::TBATT_VALID.get(ram) {
        let t = ram::TBATT_DC.get(ram);
        if t < on_below {
            true
        } else if t > off_above {
            false
        } else {
            ram::HEATER_ON.get(ram)
        }
    } else {
        false
    };
    ram::HEATER_ON.set(ram, heater);
    mode::apply_lines(ram, hal);
    sched::set_flag(ram, sched::FLAG_THERMAL);
}
