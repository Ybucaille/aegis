//! Tâche 2 `power` (critique) : validation des capteurs batterie et machine de modes.
//!
//! Tension validée `V` (SPEC §4.2) : A et B valides et d'accord (écart <= 150 mV) -> moyenne ;
//! désaccord -> la plus basse + `SFM_VBATT_DISAGREE` ; un seul valide -> celui-là + bit du
//! capteur manquant ; aucun -> pas de tension (le mode passe en `Survie`).
//! Température batterie (même schéma, choix R14) : d'accord (écart <= 2 °C) -> moyenne ;
//! désaccord -> la plus FROIDE + `SFM_TEMP_DISAGREE`.
//!
//! Un capteur est valide ssi : lecture `Ok`, valeur dans sa plage plausible, et non invalidé
//! durablement par le FDIR (`SENSOR_INVALID_MASK`, rempli en T-012).

use crate::hal::{Hal, SensorId};
use crate::mode;
use crate::ram::{self, Ram};
use crate::sched;

/// Plages plausibles (ROM). Batterie modélisée entre 6000 et 8400 mV.
pub const VBATT_RANGE_MV: (i16, i16) = (5000, 9000);
pub const TBATT_RANGE_DC: (i16, i16) = (-600, 900);
/// Écart A/B toléré.
pub const VBATT_AGREE_MV: i32 = 150;
pub const TBATT_AGREE_DC: i32 = 20;

/// Résultat de la fusion de deux capteurs redondants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fused {
    pub value: Option<i16>,
    /// Bits à ajouter à `SENSOR_FAULT_MASK`.
    pub faults: u16,
}

fn read_valid<H: Hal>(
    hal: &mut H,
    id: SensorId,
    range: (i16, i16),
    invalid_mask: u16,
) -> Option<i16> {
    if invalid_mask & (1u16 << (id as u16)) != 0 {
        return None;
    }
    match hal.sensor(id) {
        Ok(v) if v >= range.0 && v <= range.1 => Some(v),
        _ => None,
    }
}

/// Fusion A/B : `agree` = écart toléré ; en désaccord, la plus basse (tension : pessimiste ;
/// température : la plus froide, pour le thermostat).
pub fn fuse(
    a: Option<i16>,
    b: Option<i16>,
    id_a: SensorId,
    id_b: SensorId,
    agree: i32,
    disagree_bit: u16,
) -> Fused {
    match (a, b) {
        (Some(x), Some(y)) => {
            let (xi, yi) = (x as i32, y as i32);
            if (xi - yi).abs() <= agree {
                // Moyenne sur i32 puis retour en i16 : les deux bornes sont des i16, la moyenne
                // aussi, la conversion ne perd rien.
                Fused {
                    value: Some(((xi + yi) / 2) as i16),
                    faults: 0,
                }
            } else {
                Fused {
                    value: Some(x.min(y)),
                    faults: disagree_bit,
                }
            }
        }
        (Some(x), None) => Fused {
            value: Some(x),
            faults: 1 << (id_b as u16),
        },
        (None, Some(y)) => Fused {
            value: Some(y),
            faults: 1 << (id_a as u16),
        },
        (None, None) => Fused {
            value: None,
            faults: (1 << (id_a as u16)) | (1 << (id_b as u16)),
        },
    }
}

pub fn run<H: Hal>(ram: &mut Ram, hal: &mut H) {
    let invalid = ram::SENSOR_INVALID_MASK.get(ram);

    let va = read_valid(hal, SensorId::VBattA, VBATT_RANGE_MV, invalid);
    let vb = read_valid(hal, SensorId::VBattB, VBATT_RANGE_MV, invalid);
    let v = fuse(
        va,
        vb,
        SensorId::VBattA,
        SensorId::VBattB,
        VBATT_AGREE_MV,
        ram::SFM_VBATT_DISAGREE,
    );

    let ta = read_valid(hal, SensorId::TempBattA, TBATT_RANGE_DC, invalid);
    let tb = read_valid(hal, SensorId::TempBattB, TBATT_RANGE_DC, invalid);
    let t = fuse(
        ta,
        tb,
        SensorId::TempBattA,
        SensorId::TempBattB,
        TBATT_AGREE_DC,
        ram::SFM_TEMP_DISAGREE,
    );

    // Plage de tension >= 5000 : la conversion en u16 est exacte.
    let vbatt = v.value.map(|x| x as u16);
    ram::VBATT_MV.set(ram, vbatt.unwrap_or(0));
    ram::VBATT_VALID.set(ram, vbatt.is_some());
    ram::TBATT_DC.set(ram, t.value.unwrap_or(0));
    ram::TBATT_VALID.set(ram, t.value.is_some());
    ram::SENSOR_FAULT_MASK.set(ram, v.faults | t.faults | invalid);

    let inputs = mode::Inputs {
        vbatt_mv: vbatt,
        tbatt_dc: t.value,
        uptime_s: ram::UPTIME.get(ram),
    };
    mode::update(ram, &inputs);
    mode::apply_lines(ram, hal);
    sched::set_flag(ram, sched::FLAG_POWER);
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: SensorId = SensorId::VBattA;
    const B: SensorId = SensorId::VBattB;
    const BIT: u16 = ram::SFM_VBATT_DISAGREE;

    #[test]
    fn agreement_gives_mean() {
        let f = fuse(Some(7500), Some(7650), A, B, VBATT_AGREE_MV, BIT);
        assert_eq!(
            f,
            Fused {
                value: Some(7575),
                faults: 0
            }
        );
    }

    #[test]
    fn disagreement_gives_low_value_and_flag() {
        let f = fuse(Some(7500), Some(7651), A, B, VBATT_AGREE_MV, BIT);
        assert_eq!(
            f,
            Fused {
                value: Some(7500),
                faults: BIT
            }
        );
        let f = fuse(Some(8000), Some(6000), A, B, VBATT_AGREE_MV, BIT);
        assert_eq!(
            f,
            Fused {
                value: Some(6000),
                faults: BIT
            }
        );
    }

    #[test]
    fn single_sensor_gives_it_and_flags_the_other() {
        let f = fuse(None, Some(7000), A, B, VBATT_AGREE_MV, BIT);
        assert_eq!(
            f,
            Fused {
                value: Some(7000),
                faults: 1 << (A as u16)
            }
        );
        let f = fuse(Some(7000), None, A, B, VBATT_AGREE_MV, BIT);
        assert_eq!(
            f,
            Fused {
                value: Some(7000),
                faults: 1 << (B as u16)
            }
        );
        let f = fuse(None, None, A, B, VBATT_AGREE_MV, BIT);
        assert_eq!(f.value, None);
        assert_eq!(f.faults, 0b11);
    }

    #[test]
    fn extreme_values_do_not_overflow() {
        let f = fuse(Some(i16::MAX), Some(i16::MIN), A, B, 150, BIT);
        assert_eq!(f.value, Some(i16::MIN));
        let f = fuse(Some(i16::MAX), Some(i16::MAX), A, B, 150, BIT);
        assert_eq!(f.value, Some(i16::MAX));
    }
}
