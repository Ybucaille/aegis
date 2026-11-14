//! Tâche 4 `thermal` (critique) : thermostat du réchauffeur batterie (annexe A).
//!
//! `Nominal`/`Eco` : ON sous +2 °C, OFF au-dessus de +8 °C ; `Survie` : 0 / +5 °C. Entre les
//! deux seuils, l'état précédent est conservé (hystérésis, cellule `HEATER_ON`). La température
//! vient de `power` (critique, même cycle) ; en désaccord A/B c'est la plus FROIDE (R14).
//!
//! Aucune température batterie valide (révision T-012 du choix T-005 « réchauffeur coupé ») :
//! boucle OUVERTE, cadencée sur l'uptime, selon l'éclairement mesuré par `ISolar` :
//! - éclipse (`ISolar` valide et < `ISOLAR_SUN_MA`) : `DUTY_ECLIPSE_S` s sur `DUTY_PERIOD_S`
//!   (50 %, ~0,75 W : équilibre vers +10 °C avec un environnement à -20 °C) ;
//! - soleil : coupé (l'environnement est à +15 °C) ;
//! - `ISolar` illisible : `DUTY_UNKNOWN_S` s sur `DUTY_PERIOD_S` (33 % : ni gel en éclipse, ni
//!   surchauffe au soleil, ordre de grandeur annexe A / T-003).
//!
//! Raison : sans réchauffeur, la batterie passe sous -10 °C en moins de 1500 s d'éclipse ; avec
//! un réchauffeur permanent, elle dépasserait +45 °C au soleil. `TempObc` n'est PAS utilisée en
//! secours : dans le sim elle lit le même nœud thermique que la batterie, ce serait un biais
//! favorable au firmware.

use crate::hal::{Hal, SensorId};
use crate::mode;
use crate::ram::{self, Ram};
use crate::sched;

/// Courant solaire au-dessus duquel on est au soleil (mA ; ~600 mA au soleil, 0 en éclipse).
pub const ISOLAR_SUN_MA: i16 = 100;
/// Période du cadencement en boucle ouverte (s).
pub const DUTY_PERIOD_S: u32 = 60;
/// Secondes ON par période en éclipse, et quand l'éclairement est inconnu.
pub const DUTY_ECLIPSE_S: u32 = 30;
pub const DUTY_UNKNOWN_S: u32 = 20;

/// Commande du réchauffeur sans température valide.
fn open_loop<H: Hal>(ram: &mut Ram, hal: &mut H) -> bool {
    let phase = ram::UPTIME.get(ram) % DUTY_PERIOD_S;
    match hal.sensor(SensorId::ISolar) {
        Ok(i) if i >= ISOLAR_SUN_MA => false,
        Ok(i) if i >= 0 => phase < DUTY_ECLIPSE_S,
        _ => phase < DUTY_UNKNOWN_S,
    }
}

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
        open_loop(ram, hal)
    };
    ram::HEATER_ON.set(ram, heater);
    mode::apply_lines(ram, hal);
    sched::set_flag(ram, sched::FLAG_THERMAL);
}
