# Échecs de la campagne principale (T-021)

Purement factuel, aucune hypothèse de cause (T-022). Données : `results/main/runs.csv` (protégé), `results/unprotected/runs.csv` (non protégé), `results/main/summary.json`. Tableaux régénérés par `scripts/failures.sh results/main` ; section (2) et traces de rejeu écrites à partir de `campaign --replay`.

## Sources et totaux

- Source protégé : `results/main/runs.csv` (20000 lignes, graine_campagne 731051977, variante protected)
- Source non protégé : `results/unprotected/runs.csv` (20000 lignes, variante unprotected)
- Non-survivants selon `summary.json` : 1868 / 20000

## (1) Classe x catégorie principale (effectifs, protégé)

| Classe | n | E0 | E1 | E2 | E3 | E4 | E5 | E6 | E7 | E8 | Échecs | Survivants |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| F1 | 1429 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1429 |
| F2 | 1429 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1429 |
| F3 | 1429 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1429 |
| F4 | 1429 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1429 |
| F5 | 1429 | 0 | 0 | 40 | 0 | 0 | 0 | 0 | 0 | 0 | 40 | 1389 |
| F6 | 1429 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1429 |
| F7 | 1429 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1429 |
| F8 | 1429 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1429 |
| F9 | 1428 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1428 |
| F10 | 1428 | 0 | 0 | 400 | 0 | 0 | 0 | 0 | 0 | 0 | 400 | 1028 |
| F11 | 1428 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1428 |
| F12 | 1428 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1428 |
| F13 | 1428 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1428 |
| D1 | 1428 | 0 | 0 | 1428 | 0 | 0 | 0 | 0 | 0 | 0 | 1428 | 0 |
| **Total** | 20000 | 0 | 0 | 1868 | 0 | 0 | 0 | 0 | 0 | 0 | **1868** | 18132 |

Vérification somme : tableau (1) = 1868, summary.json non-survivants = 1868 -> OK

## (2) Couples (classe, catégorie principale) non vides

Graines exemples : les 5 premiers runs de chaque couple, par `run_id` croissant. Rejeu : `target/release/campaign --seed 731051977 --runs 20000 --replay <graine>` (variante protégée).
Vérification : 15 rejeux sur 15 concordent (verdict et catégories identiques au CSV, et ligne CSV rejouée identique octet pour octet à la ligne de `results/main/runs.csv`). Aucun écart.

| Classe | Catégorie principale | Effectif | Graines exemples (run_id : seed) |
|---|---|---:|---|
| F5 | E2 THERMAL | 40 | 32 : 4781019819541052408 ; 1544 : 2990178190404449115 ; 2132 : 14486980315370451616 ; 2454 : 11567456421349545517 ; 2888 : 3557172289911602756 |
| F10 | E2 THERMAL | 400 | 9 : 4921580809659328466 ; 51 : 9251429748398028234 ; 65 : 12631572964557384629 ; 79 : 2543863022207525724 ; 107 : 13288564321533541981 |
| D1 | E2 THERMAL | 1428 | 13 : 16391488761010476146 ; 27 : 1841428591752296710 ; 41 : 8978138193552996872 ; 55 : 5061984495155547576 ; 69 : 9569382132975003584 |

E3 (`DEAD`) n'est jamais catégorie principale : elle apparaît en second (`E2+E3`) dans les 1428 runs de D1.

Traces résumées de la première graine de chaque couple (sortie de `campaign --replay`) :

**F5 / E2, run_id 32, seed 4781019819541052408**
- t_inj 1771 ; `SensorStuck` (`sensor=ILineHeater kind=range value=30000`), appliquée à 1771 ; touché : capteur ILineHeater, valeur 30000.
- resets : 0 (tous types) ; journal vide.
- balises reçues : 170 ; modes annoncés : 0 à t=0 et t=60, puis 2 à partir de t=5227.
- dernière balise : mode 2, line_state 3, fdir_flags 24, sensor_fault_mask 0, cnt_latchup 255.
- critère faux : S4 (temp_min_c = -15.835 dans le CSV).
- verdict THERMAL, catégorie E2.

**F10 / E2, run_id 9, seed 4921580809659328466**
- t_inj 499 ; `BattDeplete` (`soc_x10=58`), appliquée à 499 ; touché : état de charge forcé à 5.8 %.
- resets : 1 brownout (journal : Brownout à t=3204), 0 pour les autres types.
- balises reçues : 90 ; modes annoncés : 0 puis 1 (à partir de t=9510) puis 2 (à partir de t=15210).
- dernier mode : 2 (Nominal), line_state 3 (Payload et Radio alimentées).
- critère faux : S4 (temp_min_c = -15.156 dans le CSV).
- verdict THERMAL, catégorie E2.

**D1 / E2+E3, run_id 13, seed 16391488761010476146**
- injections : `WdgDead` à t_inj 595 (touché : watchdog mort) ; `CpuHang` à t_inj 998 (touché : processeur figé jusqu'au prochain reset).
- resets : 0 ; journal vide.
- balises reçues : 0 ; aucune TC exécutée ; dernier mode : aucun.
- critères faux : S2 et S4 (`criteria` = 101011).
- verdict THERMAL, catégories E2 (S4 faux) et E3 (DEAD).

## (3) F1 et F2 : échecs par région de RAM_MAP (protégé)

| Région | Genre | Runs | Échecs | Taux d'échec |
|---|---|---:|---:|---:|
| fdir | Fdir | 593 | 0 | 0.00 % |
| log | Log | 1452 | 0 | 0.00 % |
| mode | Mode | 142 | 0 | 0.00 % |
| proto | Proto | 139 | 0 | 0.00 % |
| sched | Sched | 143 | 0 | 0.00 % |
| tm | Tm | 106 | 0 | 0.00 % |
| unused | Unused | 283 | 0 | 0.00 % |
| **Total F1+F2** | | 2858 | 0 | 0.00 % |
| **Total F1+F2 sans `Unused`** | | 2575 | 0 | 0.00 % |

## (4) Survie protégé vs non protégé par classe (écart <= 2 points)

| Classe | Protégé n | Protégé survie | Non protégé n | Non protégé survie | Écart (pp) | Même taux à 2 pp près |
|---|---:|---:|---:|---:|---:|---|
| F1 | 1429 | 1429 (100.00 %) | 1429 | 1429 (100.00 %) | +0.00 | oui |
| F2 | 1429 | 1429 (100.00 %) | 1429 | 1428 (99.93 %) | +0.07 | oui |
| F3 | 1429 | 1429 (100.00 %) | 1429 | 1069 (74.81 %) | +25.19 | non |
| F4 | 1429 | 1429 (100.00 %) | 1429 | 15 (1.05 %) | +98.95 | non |
| F5 | 1429 | 1389 (97.20 %) | 1429 | 1346 (94.19 %) | +3.01 | non |
| F6 | 1429 | 1429 (100.00 %) | 1429 | 1429 (100.00 %) | +0.00 | oui |
| F7 | 1429 | 1429 (100.00 %) | 1429 | 1429 (100.00 %) | +0.00 | oui |
| F8 | 1429 | 1429 (100.00 %) | 1429 | 1428 (99.93 %) | +0.07 | oui |
| F9 | 1428 | 1428 (100.00 %) | 1428 | 1428 (100.00 %) | +0.00 | oui |
| F10 | 1428 | 1028 (71.99 %) | 1428 | 1028 (71.99 %) | +0.00 | oui |
| F11 | 1428 | 1428 (100.00 %) | 1428 | 1428 (100.00 %) | +0.00 | oui |
| F12 | 1428 | 1428 (100.00 %) | 1428 | 1428 (100.00 %) | +0.00 | oui |
| F13 | 1428 | 1428 (100.00 %) | 1428 | 1428 (100.00 %) | +0.00 | oui |
| D1 | 1428 | 0 (0.00 %) | 1428 | 0 (0.00 %) | +0.00 | oui |

## (5) Runs OK_DEGRADED (protégé) : motifs observables dans le CSV

Total OK_DEGRADED : 518. Les motifs ne sont pas exclusifs ; `(aucun des trois)` = ni l'un ni l'autre.

| Motif | Effectif |
|---|---:|
| mode annoncé inférieur à Nominal (last_mode < 2) | 494 |
| mode Nominal, Payload ou Radio éteinte dans la dernière balise (line_state & 3 != 3) | 0 |
| masque de capteurs invalidé (sensor_fault_mask != 0) | 0 |
| (aucun des trois) | 24 |

| Classe | OK_DEGRADED | dont last_mode | dont line_state (bits 0-1) | dont masque capteur |
|---|---:|---|---|---|
| F1 | 0 | {} | {} | 0 |
| F2 | 0 | {} | {} | 0 |
| F3 | 0 | {} | {} | 0 |
| F4 | 0 | {} | {} | 0 |
| F5 | 62 | {'1': 38, '2': 24} | {'2': 38, '3': 24} | 0 |
| F6 | 0 | {} | {} | 0 |
| F7 | 0 | {} | {} | 0 |
| F8 | 0 | {} | {} | 0 |
| F9 | 0 | {} | {} | 0 |
| F10 | 456 | {'1': 456} | {'2': 456} | 0 |
| F11 | 0 | {} | {} | 0 |
| F12 | 0 | {} | {} | 0 |
| F13 | 0 | {} | {} | 0 |
| D1 | 0 | {} | {} | 0 |

Les motifs « ligne détruite » et « ligne isolée » ne sont pas dans le CSV (le verdict utilise la vérité physique de l'oracle, dont `destroyed_lines`) ; ils ne sont donc pas comptés ici.

## (6) Runs avec trace de détection, par classe

Trace = `cnt_ecc_corr + cnt_tmr_fix + cnt_latchup + cnt_tc_rejected > 0`, ou `fdir_flags` ou `sensor_fault_mask` non nul. Champ vide (aucune balise reçue) compté comme absence de trace ; colonne « sans balise » donnée à part.

| Classe | n protégé | Trace protégé | Sans balise (protégé) | n non protégé | Trace non protégé | Sans balise (non protégé) |
|---|---:|---:|---:|---:|---:|---:|
| F1 | 1429 | 1246 | 0 | 1429 | 25 | 0 |
| F2 | 1429 | 1253 | 0 | 1429 | 20 | 0 |
| F3 | 1429 | 0 | 0 | 1429 | 0 | 67 |
| F4 | 1429 | 1427 | 0 | 1429 | 0 | 159 |
| F5 | 1429 | 715 | 0 | 1429 | 507 | 0 |
| F6 | 1429 | 1429 | 0 | 1429 | 1429 | 0 |
| F7 | 1429 | 0 | 0 | 1429 | 0 | 0 |
| F8 | 1429 | 1395 | 0 | 1429 | 24 | 0 |
| F9 | 1428 | 0 | 0 | 1428 | 0 | 0 |
| F10 | 1428 | 0 | 0 | 1428 | 0 | 0 |
| F11 | 1428 | 0 | 0 | 1428 | 0 | 0 |
| F12 | 1428 | 0 | 0 | 1428 | 0 | 0 |
| F13 | 1428 | 0 | 0 | 1428 | 0 | 0 |
| D1 | 1428 | 0 | 437 | 1428 | 0 | 437 |

## (7) Lignes identiques octet pour octet, protégé vs non protégé

Colonnes de compteurs exclues de la colonne « hors compteurs » : resets_poweron, resets_watchdog, resets_brownout, resets_commanded, resets_panic, resets_unknown, boot_count, cnt_ecc_corr, cnt_tmr_fix, cnt_tc_rejected, cnt_latchup.

| Classe | n | Identiques (ligne entière) | Identiques hors compteurs |
|---|---:|---:|---:|
| F1 | 1429 | 183 | 1416 |
| F2 | 1429 | 176 | 1413 |
| F3 | 1429 | 1069 | 1069 |
| F4 | 1429 | 2 | 4 |
| F5 | 1429 | 1099 | 1100 |
| F6 | 1429 | 1429 | 1429 |
| F7 | 1429 | 1429 | 1429 |
| F8 | 1429 | 35 | 1403 |
| F9 | 1428 | 1428 | 1428 |
| F10 | 1428 | 1428 | 1428 |
| F11 | 1428 | 1428 | 1428 |
| F12 | 1428 | 1428 | 1428 |
| F13 | 1428 | 1428 | 1428 |
| D1 | 1428 | 1428 | 1428 |
| **Total** | 20000 | 13990 | 17831 |

## (8) F10 : échecs E2 avec S2 vrai (protégé)

- Échecs E2 de la classe F10 : 400
- Parmi eux, S2 vrai (2e caractère de `criteria`) : 400

## (9) F7 séparée par fault (protégé et non protégé)

| Fault | Protégé n | Protégé échecs | Protégé E2 | Non protégé n | Non protégé échecs |
|---|---:|---:|---:|---:|---:|
| WdgSpurious | 682 | 0 | 0 | 682 | 0 |
| WdgRegHit | 747 | 0 | 0 | 747 | 0 |

RESULTAT : toutes les verifications de somme passent.
