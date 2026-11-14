# Répétition générale (T-018) — résultats bruts

Commande : `./scripts/rehearsal.sh` (code retour 0). Graine campagne 42, 16 fils, `git` = `3c18c40-dirty`
(fichiers T-018 non commités au moment de la campagne). Définition de « survit » : SPEC §8, appliquée telle quelle.
Tous les chiffres ci-dessous viennent de `1_nominal/`, `2_protected/`, `3_unprotected/`, `4_double/` (summary.json et runs.csv) et sont recalculés depuis runs.csv.
`5_threads1/` : mesure de débit à 1 fil (200 runs), hors script.

## Contrôles de gate

- (a) Témoin sans panne : **OK**, 1000/1000 `OK_NOMINAL` (1000 survivants, 1000 retours nominal).
- (b) E0 `HARNESS` : **OK**, 0 sur les 5 campagnes (aucun `harness_msg`, aucune catégorie E0).
- (c) Classes P1 (F1–F11) : **OK**, 143 runs chacune, protégé et non protégé.
- (d) Taux non protégé publié à côté du protégé pour chaque classe : **OK** (tableau ci-dessous).
- (e) F1/F2 par type de région, protégé et non protégé : **OK** (tableau ci-dessous). F1 et F2 sont à 100 % des deux côtés dans toutes les régions.
- (f) Classes à 100 % en protégé : F1, F2, F7, F8, F9, F11, F12, F13 (F3 et F4 non). **Signalé** : F1, F2, F7, F8, F9, F11, F12, F13 sont aussi à 100 % en non protégé. Pour F1, F8, F11, F7, F9, F12, F13 les 143 lignes sont identiques octet pour octet entre les deux variantes (F2 : 142/143). L'injection semble sans effet observable pour ces classes. Non investigué (pas de correction autorisée).
- (g) F10, catégories d'échec : protégé 50 échecs, tous `E2 THERMAL` (principale = seule catégorie) ; non protégé 50, tous `E2 THERMAL`. Échecs globaux principaux : protégé `E2` 197, `E7` 2 ; non protégé `E8` 68, `E2` 231, `E6` 36, `E1` 47, `E7` 2.
- (h) Resets moyens par run : voir tableau « Resets moyens ».

Observations brutes :
- D1 (double panne) : 0/142 survivants des deux côtés, catégorie `E2` sur les 142 runs et `E3` aussi sur les 142 (toutes applicables). Attendu par SPEC §7 (perte attendue).
- Les 2000 runs protégés (graine 42) : 1801 survivants (90,0 %), dont 1754 nominal. Non protégé : 1616 survivants (80,8 %), dont 1575 nominal. Taux global donné pour mémoire seulement (SPEC §8.3.4).
- Le témoin non protégé de F1 et de F8 ne change rien : `runs.csv` identiques protégé/non protégé pour F1 (143/143, y compris `Fdir` et `Sched`). Cela signifie que, pour F1, l'injection ne modifie aucune trace observable dans cette variante, ce que SPEC §8.3.2 demande de signaler.
- Le binaire `campaign` ne reconnaît pas `--help` : `campagne : option inconnue "--help"`, code 2. Options lues dans `sim/src/bin/campaign.rs`.

## Durées mesurées

| Étape | Runs | Durée (s) | Débit (runs/s, 16 fils) |
|---|---|---|---|
| (1) témoin nominal | 1000 | 13,7 | 73,0 |
| (2) mixte protégé | 2000 | 27,2 | 73,4 |
| (3) mixte non protégé | 2000 | 13,1 | 152,6 |
| (4) D1 | 200 | 1,1 | 184,3 |
| hors script : mixte protégé, 1 fil | 200 | 20,1 | 9,9 |

Le script complet : 60 s de temps réel (build compris), 787 s de temps CPU utilisateur.

Extrapolation pour SPEC §8.3.4 (débit mesuré) :
- 16 fils : 20 000 protégé ≈ 272 s ; 20 000 non protégé ≈ 131 s ; 1000 nominal ≈ 14 s ; total ≈ 417 s (≈ 7 min).
- 1 fil (mesuré sur le protégé seulement) : 20 000 protégé ≈ 2020 s (≈ 34 min) ; le non protégé à 1 fil n'a pas été mesuré.
- Cible T-015 de 20 runs/s par fil : **non atteinte** (9,9 runs/s mesuré à 1 fil). T-015 notait 15,0 runs/s à 1 fil avec un firmware partiel.

Déterminisme (contrôles associés) : `cargo test --release -p sim --test determinism` → 2 passed (24,8 s). Les 200 premières lignes de `2_protected/runs.csv` (16 fils) sont identiques octet pour octet à `5_threads1/runs.csv` (1 fil).

## Tableaux bruts (généré depuis runs.csv par un script hors dépôt)

## Tableau par classe (2000 runs, graine 42)
| Classe | n (prot.) | Survie prot. | IC95 prot. | Nominal prot. | n (non prot.) | Survie non prot. | IC95 non prot. | Nominal non prot. | Diff. prot.-non prot. (pts) |
|---|---|---|---|---|---|---|---|---|---|
| F1 | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | +0.0 |
| F2 | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | +0.0 |
| F3 | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | 143 | 74.8 % (107/143) | [67.1 ; 81.2] | 74.8 % | +25.2 |
| F4 | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | 143 | 0.0 % (0/143) | [0.0 ; 2.6] | 0.0 % | +100.0 |
| F5 | 143 | 96.5 % (138/143) | [92.1 ; 98.5] | 92.3 % | 143 | 92.3 % (132/143) | [86.8 ; 95.7] | 92.3 % | +4.2 |
| F6 | 143 | 98.6 % (141/143) | [95.0 ; 99.6] | 98.6 % | 143 | 98.6 % (141/143) | [95.0 ; 99.6] | 98.6 % | +0.0 |
| F7 | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | +0.0 |
| F8 | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | +0.0 |
| F9 | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | +0.0 |
| F10 | 143 | 65.0 % (93/143) | [56.9 ; 72.4] | 36.4 % | 143 | 65.0 % (93/143) | [56.9 ; 72.4] | 36.4 % | +0.0 |
| F11 | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | +0.0 |
| F12 | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | 143 | 100.0 % (143/143) | [97.4 ; 100.0] | 100.0 % | +0.0 |
| F13 | 142 | 100.0 % (142/142) | [97.4 ; 100.0] | 100.0 % | 142 | 100.0 % (142/142) | [97.4 ; 100.0] | 100.0 % | +0.0 |
| D1 | 142 | 0.0 % (0/142) | [0.0 ; 2.6] | 0.0 % | 142 | 0.0 % (0/142) | [0.0 ; 2.6] | 0.0 % | +0.0 |
| Global (mixte, F1-F13+D1) | 2000 | 90.0 % (1801/2000) | [88.7 ; 91.3] | 87.7 % | 2000 | 80.8 % (1616/2000) | [79.0 ; 82.5] | 78.8 % | +9.2 |

## Catégorie principale des échecs (prot. / non prot.)
| Classe | Échecs prot. | Détail prot. | Échecs non prot. | Détail non prot. |
|---|---|---|---|---|
| F1 | 0 | - | 0 | - |
| F2 | 0 | - | 0 | - |
| F3 | 0 | - | 36 | {'E6': 36} |
| F4 | 0 | - | 143 | {'E8': 61, 'E2': 35, 'E1': 47} |
| F5 | 5 | {'E2': 5} | 11 | {'E8': 7, 'E2': 4} |
| F6 | 2 | {'E7': 2} | 2 | {'E7': 2} |
| F7 | 0 | - | 0 | - |
| F8 | 0 | - | 0 | - |
| F9 | 0 | - | 0 | - |
| F10 | 50 | {'E2': 50} | 50 | {'E2': 50} |
| F11 | 0 | - | 0 | - |
| F12 | 0 | - | 0 | - |
| F13 | 0 | - | 0 | - |
| D1 | 142 | {'E2': 142} | 142 | {'E2': 142} |

## Classes à 100 % en protégé
F1 prot 100 % non prot 100.0 % (143/143)
F2 prot 100 % non prot 100.0 % (143/143)
F3 prot 100 % non prot 74.8 % (107/143)
F4 prot 100 % non prot 0.0 % (0/143)
F7 prot 100 % non prot 100.0 % (143/143)
F8 prot 100 % non prot 100.0 % (143/143)
F9 prot 100 % non prot 100.0 % (143/143)
F11 prot 100 % non prot 100.0 % (143/143)
F12 prot 100 % non prot 100.0 % (143/143)
F13 prot 100 % non prot 100.0 % (142/142)

## F1/F2 par type de région (survie prot. / non prot.)
| Classe | Région | n prot. | Survie prot. | IC95 prot. | n non prot. | Survie non prot. | IC95 non prot. |
|---|---|---|---|---|---|---|---|
| F1 | Fdir | 28 | 100.0 % (28/28) | [87.9 ; 100.0] | 28 | 100.0 % (28/28) | [87.9 ; 100.0] |
| F1 | Log | 71 | 100.0 % (71/71) | [94.9 ; 100.0] | 71 | 100.0 % (71/71) | [94.9 ; 100.0] |
| F1 | Mode | 5 | 100.0 % (5/5) | [56.6 ; 100.0] | 5 | 100.0 % (5/5) | [56.6 ; 100.0] |
| F1 | Proto | 7 | 100.0 % (7/7) | [64.6 ; 100.0] | 7 | 100.0 % (7/7) | [64.6 ; 100.0] |
| F1 | Sched | 10 | 100.0 % (10/10) | [72.2 ; 100.0] | 10 | 100.0 % (10/10) | [72.2 ; 100.0] |
| F1 | Tm | 6 | 100.0 % (6/6) | [61.0 ; 100.0] | 6 | 100.0 % (6/6) | [61.0 ; 100.0] |
| F1 | Unused | 16 | 100.0 % (16/16) | [80.6 ; 100.0] | 16 | 100.0 % (16/16) | [80.6 ; 100.0] |
| F2 | Fdir | 22 | 100.0 % (22/22) | [85.1 ; 100.0] | 22 | 100.0 % (22/22) | [85.1 ; 100.0] |
| F2 | Log | 88 | 100.0 % (88/88) | [95.8 ; 100.0] | 88 | 100.0 % (88/88) | [95.8 ; 100.0] |
| F2 | Mode | 4 | 100.0 % (4/4) | [51.0 ; 100.0] | 4 | 100.0 % (4/4) | [51.0 ; 100.0] |
| F2 | Proto | 4 | 100.0 % (4/4) | [51.0 ; 100.0] | 4 | 100.0 % (4/4) | [51.0 ; 100.0] |
| F2 | Sched | 8 | 100.0 % (8/8) | [67.6 ; 100.0] | 8 | 100.0 % (8/8) | [67.6 ; 100.0] |
| F2 | Tm | 5 | 100.0 % (5/5) | [56.6 ; 100.0] | 5 | 100.0 % (5/5) | [56.6 ; 100.0] |
| F2 | Unused | 12 | 100.0 % (12/12) | [75.8 ; 100.0] | 12 | 100.0 % (12/12) | [75.8 ; 100.0] |

## Resets moyens par run (prot. / non prot.)
| Classe | Resets/run prot. | Resets/run non prot. |
|---|---|---|
| F1 | 0.000 | 0.000 |
| F2 | 0.000 | 0.000 |
| F3 | 0.000 | 0.000 |
| F4 | 0.000 | 0.000 |
| F5 | 0.000 | 0.000 |
| F6 | 0.000 | 0.000 |
| F7 | 0.503 | 0.503 |
| F8 | 0.000 | 0.000 |
| F9 | 1.000 | 1.000 |
| F10 | 0.804 | 0.804 |
| F11 | 0.000 | 0.000 |
| F12 | 0.000 | 0.000 |
| F13 | 1.000 | 1.000 |
| D1 | 0.000 | 0.000 |

## Verdicts bruts prot. {'OK_NOMINAL': 1754, 'THERMAL': 197, 'OK_DEGRADED': 47, 'CMD_INTEGRITY': 2}
## Verdicts bruts non prot. {'OK_NOMINAL': 1575, 'SILENT_LIE': 68, 'THERMAL': 231, 'COMMS_LOST': 36, 'OK_DEGRADED': 41, 'VITAL_LOSS': 47, 'CMD_INTEGRITY': 2}
## Catégorie principale échecs prot. {'E2': 197, 'E7': 2}
## Catégorie principale échecs non prot. {'E8': 68, 'E2': 231, 'E6': 36, 'E1': 47, 'E7': 2}
## Toutes catégories (non prot.) {'E8': 68, 'E2': 231, 'E3': 142, 'E6': 83, 'E1': 47, 'E7': 2}
## Toutes catégories (prot.) {'E2': 197, 'E3': 142, 'E7': 2}
