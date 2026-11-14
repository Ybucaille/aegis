//! Métriques d'un run (SPEC §8.2) : verdict, catégories, resets par cause, délais, extrema, dernière balise.

use crate::oracle::{Category, Outcome, Verdict};
use crate::world::{GroundSummary, Snapshot};
use firmware::proto::Beacon;
use firmware::{Mode, ResetCause};

/// Résultat d'un run : métriques brutes + verdict de l'oracle.
#[derive(Clone, Debug, PartialEq)]
pub struct RunResult {
    pub seed: u64,
    pub duration_s: u32,
    pub final_snapshot: Option<Snapshot>,
    pub resets_by_cause: [u32; 6],
    pub reset_log: Vec<(u32, ResetCause)>,
    pub boot_count: u32,
    pub min_soc_pct: f64,
    pub temp_min_c: f64,
    pub temp_max_c: f64,
    pub ticks_obc_off: u32,
    pub tx_frames: u32,
    pub tx_lost: u32,
    pub rx_frames: u32,
    pub nvm_writes: u32,
    /// TC exécutées : (t, seq, opcode).
    pub executed_tcs: Vec<(u32, u32, u8)>,
    /// Vue finale de la station sol (balises, ACK, rejets).
    pub ground: GroundSummary,
    /// Verdict de l'oracle et catégories applicables (ordre de priorité).
    pub verdict: Verdict,
    pub outcome: Outcome,
    /// Date de la première injection (None : run sans panne).
    pub inject_t: Option<u32>,
    /// Délai injection -> première balise valide reçue ensuite (s).
    pub delay_to_beacon_s: Option<u32>,
    /// Délai injection -> retour en `Nominal` annoncé par balise (None : « jamais »).
    pub delay_to_nominal_s: Option<u32>,
    /// Dernière balise valide reçue par le sol (compteurs ECC, TMR, TC rejetées, latch-up...).
    pub last_beacon: Option<Beacon>,
    /// Région RAM touchée par la panne (renseignée par l'injecteur).
    pub ram_region: Option<String>,
}

impl RunResult {
    pub fn categories(&self) -> &[Category] {
        &self.outcome.categories
    }
}

/// Délais (s) depuis `inject_t` : première balise reçue à `t >= inject_t`, et retour Nominal =
/// première balise Nominal suivant la dernière balise non Nominal postérieure à l'injection
/// (None si la dernière balise n'est pas Nominal ou s'il n'y en a pas).
pub fn delays(inject_t: u32, beacons: &[(u32, u8, u16)]) -> (Option<u32>, Option<u32>) {
    let after: Vec<&(u32, u8, u16)> = beacons.iter().filter(|b| b.0 >= inject_t).collect();
    let Some(first) = after.first() else {
        return (None, None);
    };
    let nominal = Mode::Nominal as u8;
    let to_beacon = Some(first.0 - inject_t);
    let last = after[after.len() - 1];
    if last.1 != nominal {
        return (to_beacon, None);
    }
    let back = match after.iter().rposition(|b| b.1 != nominal) {
        Some(i) => after[i + 1].0,
        None => first.0,
    };
    (to_beacon, Some(back - inject_t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_cases() {
        let n = Mode::Nominal as u8;
        let e = Mode::Eco as u8;
        // jamais dégradé : retour = première balise
        assert_eq!(
            delays(100, &[(50, n, 1), (130, n, 1)]),
            (Some(30), Some(30))
        );
        // dégradé puis retour
        assert_eq!(
            delays(100, &[(110, e, 1), (120, e, 1), (200, n, 1), (210, n, 1)]),
            (Some(10), Some(100))
        );
        // jamais revenu
        assert_eq!(delays(100, &[(110, e, 1)]), (Some(10), None));
        // aucune balise après l'injection
        assert_eq!(delays(100, &[(10, n, 1)]), (None, None));
    }
}
