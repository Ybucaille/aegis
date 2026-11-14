# T-023 Corrections firmware décidées au triage (rien de plus)
Agent: systems   Modèle: Opus   Dépend de: T-022
Entrée: `docs/triage.md` (section « correctifs firmware », liste fermée), graines exemples de `results/main/failures.md`
Sortie attendue: modifications dans `firmware/` uniquement, un correctif = un test de non-régression nommé d'après
l'entrée du triage. Aucune fonctionnalité nouvelle, aucun changement d'interface publique, de format de trame
ou de `RAM_MAP` visible du sim sans accord du manager. Tout ce qui est tentant mais hors liste → `docs/later.md`.
HEURE LIMITE : 70 % du jour 3. Un correctif non vert à l'heure limite est RETIRÉ (retour à l'état d'avant) et
passe en « documenté ». Budget : 25 % du jour 3, partagé avec T-024.
Lot J3-A, parallèle avec T-024 et T-026 : n'écrit que dans `firmware/`.
Critère de fin (vérifiable): pour chaque correctif, `campaign --replay <graine exemple>` passe d'échec à survie
(avant/après écrit dans le compte rendu, ou « sans effet » dit tel quel) ; `./scripts/ci.sh` → `CI OK` ;
`cargo test -p sim --test gate_day1` vert ; `campaign --seed 1 --runs 200 --class none` → 200 `OK_NOMINAL`.
Liste fermée (T-022, manager) : UN correctif, C1 — voir `docs/triage.md` (section C1) pour le détail exact.
C1 : `firmware/src/tasks/fdir.rs` SEULEMENT. (a) obligatoire : `ILINE_RANGE_MA: (i16, i16) = (-100, 2000)` ; lecture
`Err` ou hors plage ignorée par le latch-up (LU_OVER à 0, pas de coupure, pas de compteur). (b) séparable : OU du bit
`1 << LINE_SENSOR[l]` dans `SENSOR_FAULT_MASK` (retirable seul à l'heure limite). Interdits : seuils, règle d'isolement,
`RAM_MAP`, format de balise, variante `unprotected`.
Tests exigés : `c1_iline_out_of_range_is_not_latchup` et `c1_real_latchup_still_detected_up_to_range` (définis au
triage) ; les 4 tests latch-up existants verts SANS modification.
Rejeux (campagne `--seed 731051977 --runs 20000 --replay <g>`) : 4781019819541052408, 2990178190404449115,
14486980315370451616, 11567456421349545517, 3557172289911602756 : THERMAL → survie ; F4 11712473911359860057,
11486522431347728424, 14102530543474748004 : restent OK_NOMINAL avec cnt_latchup = 1. Build `unprotected` vert.
## Compte rendu (rempli par l'agent)

**Fait : C1 (a) ET (b), rien d'autre.** Seul fichier de code modifié : `firmware/src/tasks/fdir.rs`.
- (a) constante ROM `pub const ILINE_RANGE_MA: (i16, i16) = (-100, 2000)` (justification 250 + 800 = 1050 mA en
  commentaire). Dans `latchup`, `match hal.sensor(LINE_SENSOR[l])` : `Ok(i)` dans la plage (bornes incluses) →
  `i > LU_THRESHOLD_MA[l]` inchangé ; `Err` ou hors plage → `over = false` (donc `LU_OVER` remis à 0, pas de
  coupure, ni `CNT_LATCHUP` ni `LU_RECUR`).
- (b) même branche : `SENSOR_FAULT_MASK |= 1 << LINE_SENSOR[l]` (OU sans état ; `power` réécrit le masque avant
  `fdir`, `telemetry` le lit après). Conservé (aucune difficulté). Pendant une coupure de latch-up en cours la
  ligne n'est pas échantillonnée, donc pas de bit ce cycle-là (comportement d'échantillonnage inchangé).
- Inchangés : seuils `LU_*`, règle d'isolement, `RAM_MAP`, format de balise, variante `unprotected`.
- Tests ajoutés : `c1_iline_out_of_range_is_not_latchup` (3 lignes × {30000, i16::MAX, -30000, Err(Io)},
  2000 cycles chacun, assertions par cycle : CNT_LATCHUP, LINE_ISOLATED, LU_OFF_LEFT[0..3] = 0, CRITICAL et
  LINE_ISOLATED absents, bit capteur dans SENSOR_FAULT_MASK, ligne commandée ; puis bit absent dès retour dans
  la plage) ; `c1_real_latchup_still_detected_up_to_range` (par ligne : courant total 2000 mA et nominal + 800 mA
  coupés au 3e échantillon pendant LU_OFF_S puis réarmés, CNT_LATCHUP = 1 ; 2001 mA jamais coupé, compteur 0).
  Écart de forme : pour Heater, `set_temp(-100)` (sinon le réchauffeur n'est pas commandé) et la vérification
  « mode Nominal » n'est faite que pour Payload/Radio (batterie froide → mode quitte Nominal, Payload
  non commandée : constaté, c'est le comportement normal du mode). 5 cycles de chauffe avant le latch-up.
- Contre-épreuve : avec la condition de plage remplacée par `true`, les 2 tests C1 échouent ; fichier restauré.
- Les 4 tests latch-up existants : verts, corps non modifiés.

Rejeux (`campaign --seed 731051977 --runs 20000 --replay <g>`, binaire protégé reconstruit) :
| graine | panne | avant | après |
|---|---|---|---|
| 4781019819541052408 | F5 ILineHeater 30000 | THERMAL, Tmin -15,835, cnt_latchup 255, flags 24 | OK_NOMINAL, Tmin 1,993, cnt 0, flags 0, sfm 32 |
| 2990178190404449115 | F5 ILineHeater 30000 | THERMAL, Tmin -15,817, cnt 255, flags 8 | OK_NOMINAL, Tmin 2,003, cnt 0, sfm 32 |
| 14486980315370451616 | F5 ILineHeater 32767 | THERMAL, Tmin -15,817, cnt 255 | OK_NOMINAL, Tmin 2,015, cnt 0, sfm 32 |
| 11567456421349545517 | F5 ILineHeater 32767 | THERMAL, Tmin -15,817, cnt 255 | OK_NOMINAL, Tmin 1,973, cnt 0, sfm 32 |
| 3557172289911602756 | F5 ILineHeater 30000 | THERMAL, Tmin -15,817, cnt 255 | OK_NOMINAL, Tmin 1,945, cnt 0, sfm 32 |
| 9664786027175421902 | F5 ILinePayload 32767 | OK_DEGRADED (Eco, Payload isolée, cnt 255) | OK_NOMINAL, cnt 0, sfm 8 |
| 15719726885866825989 | F5 ILineRadio 32767 | OK_DEGRADED (cnt 255, flags 1 CRITICAL) | OK_NOMINAL, cnt 0, sfm 16 |
| 11712473911359860057 | F4 Payload +476 | OK_NOMINAL, cnt 1 | OK_NOMINAL, cnt 1 |
| 11486522431347728424 | F4 Radio +545 | OK_NOMINAL, cnt 1 | OK_NOMINAL, cnt 1 |
| 14102530543474748004 | F4 Heater +506 | OK_NOMINAL, cnt 1 | OK_NOMINAL, cnt 1 |
Témoin `unprotected` (binaire reconstruit), 4781019819541052408 : OK_NOMINAL, Tmin 1,993, cnt 0 (inchangé).
Graine 990274663 non utilisée.

Commandes lancées et résultats réels :
- `cargo fmt` ; `./scripts/ci.sh` → code 0, `CI OK` (7/7 étapes ; fmt, clippy -D warnings, cargo test :
  257 tests passés, 0 échec).
- `cargo test -p sim --test gate_day1` → `test result: ok. 1 passed`.
- `cargo build --release -p sim --bin campaign` → OK ; `cargo build --release -p sim --bin campaign
  --target-dir target/unprotected --features unprotected` → OK.
- `target/release/campaign --seed 1 --runs 200 --class none --out results/t023-check` → 200 OK_NOMINAL
  (survie brute 200/200).
Non fait ici (hors périmètre, T-025) : campagne complète de non-régression. Non commité.
