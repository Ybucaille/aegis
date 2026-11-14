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
## Compte rendu (rempli par l'agent)
