# DIFF T-025 : survie avant (main) / après C1 (final) / validation

Généré par dépouillement brut de `runs.csv` (aucune pondération entre classes).
Survie = colonne `survived` = 1 (S1..S6 vrais), ce qui vaut `OK_NOMINAL` ou `OK_DEGRADED`.
Graines : main/final S = 731051977 ; validation V = 990274663. E0 exclus : main 0, final 0, validation 0.

| Classe | main (protégé, S) | final (protégé, S, après C1) | validation (protégé, V) | effectifs main / final / validation |
|---|---|---|---|---|
| F1 | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429 / 1429 / 1429 |
| F2 | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429 / 1429 / 1429 |
| F3 | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429 / 1429 / 1429 |
| F4 | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429 / 1429 / 1429 |
| F5 | 1389/1429 (97.20 %) | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429 / 1429 / 1429 |
| F6 | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429 / 1429 / 1429 |
| F7 | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429 / 1429 / 1429 |
| F8 | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429/1429 (100.00 %) | 1429 / 1429 / 1429 |
| F9 | 1428/1428 (100.00 %) | 1428/1428 (100.00 %) | 1428/1428 (100.00 %) | 1428 / 1428 / 1428 |
| F10 | 1028/1428 (71.99 %) | 1028/1428 (71.99 %) | 1012/1428 (70.87 %) | 1428 / 1428 / 1428 |
| F11 | 1428/1428 (100.00 %) | 1428/1428 (100.00 %) | 1428/1428 (100.00 %) | 1428 / 1428 / 1428 |
| F12 | 1428/1428 (100.00 %) | 1428/1428 (100.00 %) | 1428/1428 (100.00 %) | 1428 / 1428 / 1428 |
| F13 | 1428/1428 (100.00 %) | 1428/1428 (100.00 %) | 1428/1428 (100.00 %) | 1428 / 1428 / 1428 |
| D1 | 0/1428 (0.00 %) | 0/1428 (0.00 %) | 0/1428 (0.00 %) | 1428 / 1428 / 1428 |

Campagne double (D1 seul, graine S) : main `results/double` 0/1000 (0.00 %), final `results/final-double` 0/1000 (0.00 %).

## Régressions

Graines survivantes dans main et en échec dans final : **0**.


Pour information, sens inverse (échec dans main, survie dans final) : 40.

## Provenance

- `results/final/VERSION` = e8bb46fd062dd662ad200b06c0510069bafa8117 (firmware/ et sim/ identiques à a59b7f5, correctif C1).
- `results/main/` : sha256 de `runs.csv` = fa7739d21b0f4381f07ad1fc2b236c85f12f7bea7e9251ff09dd169cc515dbab, identique à `git show HEAD:results/main/runs.csv`. Non modifié.
- Final : `AEGIS_PREFIX=final scripts/campaign.sh` (log `results/final_campaign.log`, code retour 0).
- Validation : `target/release/campaign --seed 990274663 --runs 20000 --out results/validation` (log `results/validation_campaign.log`, code retour 0), une seule exécution, sans `--replay`.
- Dépouillement : `scratchpad/diff_final.py` (colonne `survived`, aucune réécriture de runs.csv).
