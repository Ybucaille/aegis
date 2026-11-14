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
