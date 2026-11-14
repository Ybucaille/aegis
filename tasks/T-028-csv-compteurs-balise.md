# T-028 Correctif bloquant B1 : compteurs de la dernière balise dans `runs.csv` (SPEC §8.2)
Agent: builder   Modèle: Sonnet   Dépend de: T-019   Parallèle avec: T-029 (fichiers disjoints)
Entrée: `docs/decisions.md` (entrées T-019), `docs/SPEC.md` §8.2, `sim/src/campaign.rs`
Sortie attendue: `sim/src/campaign.rs` et `sim/tests/campaign.rs` SEULEMENT. Sept colonnes ajoutées à
`runs.csv`, insérées juste avant `harness_msg`, lues dans `RunResult.last_beacon` (vides si aucune balise) :
`cnt_ecc_corr, cnt_tmr_fix, cnt_tc_rejected, cnt_latchup, fdir_flags, sensor_fault_mask, line_state`.
Aucune autre colonne modifiée, aucun changement d'oracle, de seuil, de tirage ni de modèle de panne.
Interdit de toucher `oracle.rs`, `fault.rs`, `world.rs`, `params.rs`, `firmware/`.
Raison : SPEC §8.2 exige « compteurs de la dernière balise » par run ; ils manquent, or ce sont eux qui
séparent « panne détectée et corrigée » de « panne sans effet » (point (f) de la répétition).
Critère de fin (vérifiable): `./scripts/ci.sh` → CI OK ; `cargo test --release -p sim --test determinism` vert ;
`./scripts/rehearsal.sh` relancé puis : (1) `runs.csv` privé des 7 nouvelles colonnes identique octet pour octet à
la version commitée de `results/rehearsal/{2_protected,3_unprotected}/runs.csv`, HORS les lignes que T-029
corrige (run_id 1195 et 1279) ; (2) F1 protégé hors `Unused` : au moins 80 % des lignes avec
`cnt_tmr_fix + cnt_ecc_corr >= 1` ; (3) F1 non protégé : ces deux compteurs à 0 partout ; (4) témoin
`1_nominal` : les quatre compteurs à 0 sur les 1000 runs. Un écart sur (2), (3) ou (4) se rapporte tel quel.
## Compte rendu (rempli par l'agent)
