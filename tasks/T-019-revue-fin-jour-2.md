# T-019 ★ Revue de fin de jour 2 : risques, plan du jour 3, gel de campagne
Agent: strategist   Modèle: Fable   Dépend de: T-018, T-016
Entrée: `docs/SPEC.md`, `docs/risks.md`, `docs/decisions.md`, `results/rehearsal/NOTES.md`,
`results/rehearsal/*/summary.json`, comptes rendus des tâches T-010 à T-018
Sortie attendue: `docs/risks.md` mis à jour (checklist de `CLAUDE.md` reparcourue, état de chaque risque) ;
entrée dans `docs/decisions.md` : verdict de la gate du jour 2, valeurs finales de l'annexe A, GEL DE CAMPAGNE
(plus aucun changement de seuil, d'oracle ni de modèle de panne après cette entrée), graine de campagne et
graine de validation choisies et écrites AVANT tout run du jour 3 ; `tasks/` du jour 3 réordonnées si besoin.
Points à trancher explicitement : le témoin sans protections mord-il (écart net par classe) ? Une classe à
100 % de survie est-elle réellement injectée ou sans effet ? Le débit permet-il 41 000 runs dans le budget ?
Séquentiel : aucun agent n'écrit de code pendant la revue.
Critère de fin (vérifiable): `grep -c "GEL DE CAMPAGNE" docs/decisions.md` → 1 ; l'entrée contient les deux
graines et le verdict (GO / GO avec réserves listées / NO-GO avec la liste des correctifs bloquants) ;
`git status --porcelain docs/SPEC.md` → vide (spec non modifiée).
## Compte rendu (rempli par l'agent)
