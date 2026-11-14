//! Machine de modes (SPEC §4.2) et politique des lignes d'alimentation par mode.
//!
//! Table de transitions (une transition au plus par cycle, la descente forcée est évaluée en
//! premier) :
//!
//! | Transition | Condition |
//! |---|---|
//! | boot -> `Survie` | toujours (`sched::boot`) |
//! | tout -> `Survie` | `V < 6720` pendant 10 s ; aucun capteur de tension valide ; `T_batt` hors [-5, +45] °C ; verrou tempête ; plafond = `Survie` |
//! | `Survie` -> `Eco` | `V >= 7080` pendant 60 s, uptime >= 120 s, pas d'alarme FDIR critique, plafond >= `Eco` |
//! | `Eco` -> `Nominal` | `V >= 7560` pendant 60 s, plafond = `Nominal`, pas de verrou tempête, Payload non isolée |
//! | `Nominal` -> `Eco` | `V < 7200` pendant 10 s, ou plafond < `Nominal`, ou Payload isolée |
//!
//! Précision d'implémentation (SPEC muette) : les compteurs de montée (`CNT_V_ECO_UP`,
//! `CNT_V_NOM_UP`) sont remis à zéro à chaque changement de mode et tant qu'une condition de
//! descente forcée est vraie : une montée exige 60 s mesurées DANS le mode de départ, ce qui
//! interdit `Survie` -> `Eco` -> `Nominal` en deux cycles et donne une hystérésis temporelle.
//! Les compteurs de descente, eux, comptent quel que soit le mode.

use crate::hal::{Hal, Line, Reg};
use crate::ram::{self, Ram};
use crate::Mode;

/// Seuils (ROM, SPEC §4.2).
pub const V_ECO_UP_MV: u16 = 7080;
pub const V_NOM_UP_MV: u16 = 7560;
pub const V_NOM_DOWN_MV: u16 = 7200;
pub const V_CRIT_MV: u16 = 6720;
/// Durée de maintien pour une montée (s).
pub const DWELL_UP_S: u16 = 60;
/// Durée de maintien pour une descente (s).
pub const DWELL_DOWN_S: u16 = 10;
/// Uptime minimal pour quitter `Survie` (s).
pub const UPTIME_MIN_S: u32 = 120;
/// Plage de température batterie hors de laquelle on force `Survie` (0,1 °C).
pub const T_SURVIE_MIN_DC: i16 = -50;
pub const T_SURVIE_MAX_DC: i16 = 450;

/// Lecture totale du mode : toute valeur hors domaine vaut `Survie` (le plus sûr).
pub fn current(ram: &mut Ram) -> Mode {
    Mode::from_u8(ram::MODE.get(ram)).unwrap_or(Mode::Survie)
}

/// Lecture totale du plafond : toute valeur hors domaine vaut `Nominal` (défaut de boot ; le sol
/// peut toujours l'abaisser, et forcer `Survie` sur un octet corrompu bloquerait la remontée).
pub fn ceiling(ram: &mut Ram) -> Mode {
    Mode::from_u8(ram::CEILING.get(ram)).unwrap_or(Mode::Nominal)
}

/// Entrées de la machine de modes, toutes issues de tâches critiques du cycle ou de cycles
/// précédents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inputs {
    pub vbatt_mv: Option<u16>,
    pub tbatt_dc: Option<i16>,
    pub uptime_s: u32,
}

/// Compteur de maintien : +1 (saturant) si la condition tient, 0 sinon. Renvoie la nouvelle
/// valeur.
fn dwell(ram: &mut Ram, slot: &ram::Slot<u16>, cond: bool) -> u16 {
    let v = if cond {
        slot.get(ram).saturating_add(1)
    } else {
        0
    };
    slot.set(ram, v);
    v
}

/// Un pas de la machine de modes. Écrit le nouveau mode et le renvoie.
pub fn update(ram: &mut Ram, inp: &Inputs) -> Mode {
    let mode = current(ram);
    let ceil = ceiling(ram);
    let fdir_flags = ram::FDIR_FLAGS.get(ram);
    let storm = fdir_flags & ram::FDIR_FLAG_STORM_LOCK != 0;
    let fdir_critical = fdir_flags & ram::FDIR_FLAG_CRITICAL != 0;
    let payload_isolated = line_isolated(ram, Line::Payload);

    let v = inp.vbatt_mv;
    let crit = dwell(ram, &ram::CNT_V_CRIT, matches!(v, Some(x) if x < V_CRIT_MV));
    let low = dwell(
        ram,
        &ram::CNT_V_LOW,
        matches!(v, Some(x) if x < V_NOM_DOWN_MV),
    );
    let eco_up = dwell(
        ram,
        &ram::CNT_V_ECO_UP,
        matches!(v, Some(x) if x >= V_ECO_UP_MV),
    );
    let nom_up = dwell(
        ram,
        &ram::CNT_V_NOM_UP,
        matches!(v, Some(x) if x >= V_NOM_UP_MV),
    );

    let t_out =
        matches!(inp.tbatt_dc, Some(t) if !(T_SURVIE_MIN_DC..=T_SURVIE_MAX_DC).contains(&t));
    let force_survie =
        v.is_none() || crit >= DWELL_DOWN_S || t_out || storm || ceil == Mode::Survie;

    let next = if force_survie {
        Mode::Survie
    } else {
        match mode {
            Mode::Survie => {
                if eco_up >= DWELL_UP_S
                    && inp.uptime_s >= UPTIME_MIN_S
                    && !fdir_critical
                    && ceil >= Mode::Eco
                {
                    Mode::Eco
                } else {
                    Mode::Survie
                }
            }
            Mode::Eco => {
                if nom_up >= DWELL_UP_S && ceil == Mode::Nominal && !storm && !payload_isolated {
                    Mode::Nominal
                } else {
                    Mode::Eco
                }
            }
            Mode::Nominal => {
                if low >= DWELL_DOWN_S || ceil < Mode::Nominal || payload_isolated {
                    Mode::Eco
                } else {
                    Mode::Nominal
                }
            }
        }
    };

    if force_survie || next != mode {
        ram::CNT_V_ECO_UP.set(ram, 0);
        ram::CNT_V_NOM_UP.set(ram, 0);
    }
    ram::MODE.set(ram, next as u8);
    next
}

/// Période de balise du mode (s), SPEC §4.2.
pub const fn beacon_period(m: Mode) -> u16 {
    match m {
        Mode::Nominal => 10,
        Mode::Eco => 30,
        Mode::Survie => 60,
    }
}

/// Seuils du thermostat `(ON sous, OFF au-dessus)` en 0,1 °C, annexe A.
pub const fn thermostat(m: Mode) -> (i16, i16) {
    match m {
        Mode::Survie => (0, 50),
        Mode::Eco | Mode::Nominal => (20, 80),
    }
}

/// Ligne isolée par le FDIR. Radio : jamais isolée (bit ignoré).
pub fn line_isolated(ram: &mut Ram, l: Line) -> bool {
    if l == Line::Radio {
        return false;
    }
    ram::LINE_ISOLATED.get(ram) & (l.mask() as u8) != 0
}

/// Valeur attendue de `Reg::LoadSwitches`, recalculée à partir des cellules (jamais stockée) :
/// Radio toujours alimentée ; Payload ssi `Nominal` et non isolée ; Heater selon le thermostat
/// et non isolée.
pub fn commanded_lines(ram: &mut Ram) -> u16 {
    let mut v = Line::Radio.mask();
    if current(ram) == Mode::Nominal && !line_isolated(ram, Line::Payload) {
        v |= Line::Payload.mask();
    }
    if ram::HEATER_ON.get(ram) && !line_isolated(ram, Line::Heater) {
        v |= Line::Heater.mask();
    }
    v
}

/// Applique les lignes commandées au registre.
pub fn apply_lines<H: Hal>(ram: &mut Ram, hal: &mut H) {
    let v = commanded_lines(ram);
    hal.reg_write(Reg::LoadSwitches, v);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::RAM_SIZE;

    fn fresh() -> Ram {
        let mut ram = [0u8; RAM_SIZE];
        ram::MODE.set(&mut ram, Mode::Survie as u8);
        ram::CEILING.set(&mut ram, Mode::Nominal as u8);
        ram
    }

    fn inp(v: u16, uptime: u32) -> Inputs {
        Inputs {
            vbatt_mv: Some(v),
            tbatt_dc: Some(200),
            uptime_s: uptime,
        }
    }

    /// Fait tourner la machine `n` cycles à tension constante ; renvoie le cycle (1-based) du
    /// premier changement de mode, s'il y en a un.
    fn run_until_change(ram: &mut Ram, v: u16, start_uptime: u32, n: u32) -> Option<u32> {
        let m0 = current(ram);
        (1..=n).find(|&k| update(ram, &inp(v, start_uptime + k)) != m0)
    }

    #[test]
    fn survie_to_eco_respects_uptime_and_dwell() {
        // Tension haute dès le boot : c'est l'uptime (120 s) qui limite.
        let mut ram = fresh();
        assert_eq!(run_until_change(&mut ram, 7100, 0, 500), Some(120));
        assert_eq!(current(&mut ram), Mode::Eco);
        // Uptime déjà grand : c'est le maintien de 60 s qui limite.
        let mut ram = fresh();
        assert_eq!(run_until_change(&mut ram, 7100, 1000, 500), Some(60));
        // Juste sous le seuil : jamais.
        let mut ram = fresh();
        assert_eq!(run_until_change(&mut ram, V_ECO_UP_MV - 1, 1000, 500), None);
    }

    #[test]
    fn eco_to_nominal_needs_60s_in_eco() {
        let mut ram = fresh();
        assert_eq!(run_until_change(&mut ram, 7600, 1000, 500), Some(60));
        assert_eq!(current(&mut ram), Mode::Eco);
        assert_eq!(run_until_change(&mut ram, 7600, 1060, 500), Some(60));
        assert_eq!(current(&mut ram), Mode::Nominal);
    }

    #[test]
    fn nominal_to_eco_after_10s_low() {
        let mut ram = fresh();
        ram::MODE.set(&mut ram, Mode::Nominal as u8);
        assert_eq!(run_until_change(&mut ram, 7199, 1000, 100), Some(10));
        assert_eq!(current(&mut ram), Mode::Eco);
        // Une seconde au-dessus remet le compteur à zéro.
        let mut ram = fresh();
        ram::MODE.set(&mut ram, Mode::Nominal as u8);
        for k in 0..9 {
            update(&mut ram, &inp(7100, 1000 + k));
        }
        update(&mut ram, &inp(7300, 1009));
        assert_eq!(run_until_change(&mut ram, 7100, 1010, 100), Some(10));
    }

    #[test]
    fn any_to_survie_after_10s_critical() {
        for m in [Mode::Eco, Mode::Nominal] {
            let mut ram = fresh();
            ram::MODE.set(&mut ram, m as u8);
            let mut changed = None;
            for k in 1..=30 {
                let n = update(&mut ram, &inp(6700, 1000 + k));
                if n == Mode::Survie {
                    changed = Some(k);
                    break;
                }
            }
            // Depuis Nominal on passe d'abord par Eco (V < 7200 pendant 10 s tombe au même
            // cycle que V < 6720 pendant 10 s : la descente forcée l'emporte).
            assert_eq!(changed, Some(10), "depuis {m:?}");
        }
    }

    #[test]
    fn no_valid_voltage_forces_survie_immediately() {
        let mut ram = fresh();
        ram::MODE.set(&mut ram, Mode::Nominal as u8);
        let i = Inputs {
            vbatt_mv: None,
            tbatt_dc: Some(200),
            uptime_s: 1000,
        };
        assert_eq!(update(&mut ram, &i), Mode::Survie);
    }

    #[test]
    fn temperature_out_of_range_forces_survie() {
        for t in [T_SURVIE_MIN_DC - 1, T_SURVIE_MAX_DC + 1] {
            let mut ram = fresh();
            ram::MODE.set(&mut ram, Mode::Nominal as u8);
            let i = Inputs {
                vbatt_mv: Some(8000),
                tbatt_dc: Some(t),
                uptime_s: 1000,
            };
            assert_eq!(update(&mut ram, &i), Mode::Survie);
        }
        let mut ram = fresh();
        ram::MODE.set(&mut ram, Mode::Nominal as u8);
        let i = Inputs {
            vbatt_mv: Some(8000),
            tbatt_dc: Some(T_SURVIE_MAX_DC),
            uptime_s: 1000,
        };
        assert_eq!(update(&mut ram, &i), Mode::Nominal);
    }

    #[test]
    fn ceiling_caps_mode() {
        let mut ram = fresh();
        ram::CEILING.set(&mut ram, Mode::Eco as u8);
        assert_eq!(run_until_change(&mut ram, 8000, 1000, 500), Some(60));
        assert_eq!(current(&mut ram), Mode::Eco);
        assert_eq!(run_until_change(&mut ram, 8000, 2000, 500), None);
        // Plafond abaissé en Nominal -> Eco immédiat ; plafond Survie -> Survie immédiat.
        ram::MODE.set(&mut ram, Mode::Nominal as u8);
        assert_eq!(update(&mut ram, &inp(8000, 3000)), Mode::Eco);
        ram::CEILING.set(&mut ram, Mode::Survie as u8);
        assert_eq!(update(&mut ram, &inp(8000, 3001)), Mode::Survie);
        assert_eq!(run_until_change(&mut ram, 8000, 3002, 500), None);
    }

    #[test]
    fn fdir_flags_and_isolation_block_transitions() {
        let mut ram = fresh();
        ram::FDIR_FLAGS.set(&mut ram, ram::FDIR_FLAG_CRITICAL);
        assert_eq!(run_until_change(&mut ram, 8000, 1000, 300), None);

        let mut ram = fresh();
        ram::MODE.set(&mut ram, Mode::Eco as u8);
        ram::LINE_ISOLATED.set(&mut ram, Line::Payload.mask() as u8);
        assert_eq!(run_until_change(&mut ram, 8000, 1000, 300), None);
        ram::MODE.set(&mut ram, Mode::Nominal as u8);
        assert_eq!(update(&mut ram, &inp(8000, 2000)), Mode::Eco);

        let mut ram = fresh();
        ram::MODE.set(&mut ram, Mode::Nominal as u8);
        ram::FDIR_FLAGS.set(&mut ram, ram::FDIR_FLAG_STORM_LOCK);
        assert_eq!(update(&mut ram, &inp(8000, 2000)), Mode::Survie);
    }

    /// Corruption volontaire : mode et plafond hors domaine, compteurs énormes.
    #[test]
    fn corrupted_cells_decode_totally() {
        let mut ram = fresh();
        ram::MODE.set(&mut ram, 0xEE);
        ram::CEILING.set(&mut ram, 0x77);
        assert_eq!(current(&mut ram), Mode::Survie);
        assert_eq!(ceiling(&mut ram), Mode::Nominal);
        ram::CNT_V_CRIT.set(&mut ram, u16::MAX);
        ram::CNT_V_ECO_UP.set(&mut ram, u16::MAX);
        // Le compteur critique corrompu n'agit pas si la condition est fausse ce cycle.
        let m = update(&mut ram, &inp(8000, 1000));
        assert_eq!(m, Mode::Eco); // compteur de montée saturé -> montée (limite connue, T-010 : TMR)
        assert_eq!(ram::MODE.get(&mut ram), Mode::Eco as u8);
        assert_eq!(ram::CNT_V_CRIT.get(&mut ram), 0);
    }

    #[test]
    fn lines_policy() {
        let mut ram = fresh();
        assert_eq!(commanded_lines(&mut ram), Line::Radio.mask());
        ram::MODE.set(&mut ram, Mode::Nominal as u8);
        ram::HEATER_ON.set(&mut ram, true);
        assert_eq!(commanded_lines(&mut ram), 0b111);
        // Radio : le bit d'isolation est ignoré.
        ram::LINE_ISOLATED.set(&mut ram, 0xFF);
        assert_eq!(commanded_lines(&mut ram), Line::Radio.mask());
        ram::MODE.set(&mut ram, 0x42);
        ram::LINE_ISOLATED.set(&mut ram, 0);
        assert_eq!(
            commanded_lines(&mut ram),
            Line::Radio.mask() | Line::Heater.mask()
        );
    }

    #[test]
    fn periods_and_thermostat_table() {
        assert_eq!(beacon_period(Mode::Nominal), 10);
        assert_eq!(beacon_period(Mode::Eco), 30);
        assert_eq!(beacon_period(Mode::Survie), 60);
        assert_eq!(thermostat(Mode::Survie), (0, 50));
        assert_eq!(thermostat(Mode::Eco), (20, 80));
        assert_eq!(thermostat(Mode::Nominal), (20, 80));
    }
}
