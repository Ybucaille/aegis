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
Fait (strategist, 2026-11-14). Aucun code du dépôt modifié, `docs/SPEC.md` intact.
- (f) Cause : pas un bogue d'injection. Balayage exhaustif hors dépôt (8192 bits, graine 13679457532755275413,
  t=3000) : protégé 7240/7424 bits hors `Unused` laissent une trace et 8192/8192 `OK_NOMINAL` ; non protégé
  216 bits à effet observable, 1 échec, 3 dégradés. Le témoin survit à un SEU RAM isolé dans ~99,99 % des cas :
  F1/F2/F8 ne peuvent pas discriminer. F7a/F9/F12/F13 : mécanismes hors protections. F7b, F11 : latents.
- Défaut trouvé : faux positif E7 de l'oracle sur F6 (rejeu 10219195993739918047).
- Verdict : NO-GO, liste fermée B1 = T-028, B2 = T-029. Gel écrit. S = 731051977, V = 990274663.
- F10 : accepter et documenter. Débit : suffisant.
- Écarts : `tasks/T-028`, `T-029`, `T-030` créées par le strategist (et non par le manager) ; lignes « Ajout
  T-019 » dans T-020, T-021, T-022, T-025, T-027 ; 4 lignes ajoutées à `docs/later.md`.
- Commandes : `grep -c "GEL DE CAMPAGNE" docs/decisions.md` → 1 ; `git status --porcelain docs/SPEC.md` → vide.
- Non fait : le balayage n'a tourné que sur une graine et une date ; F5/E2 protégé non instruit (T-022).
