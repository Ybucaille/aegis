# T-027 ★ Rapport final `docs/REPORT.md`
Agent: strategist   Modèle: Fable   Dépend de: T-025
Entrée: `results/{main,final,validation,control,unprotected,double}/summary.json`, `results/final/DIFF.md`,
`results/main/failures.md`, `docs/triage.md`, `docs/SPEC.md` §7.2 et §8, `docs/risks.md`, `docs/decisions.md`
Sortie attendue: `docs/REPORT.md` : (1) résultat en une phrase, avec N et intervalle ; (2) définition de
« survit » rappelée ; (3) tableau par classe : effectif, survie, retour au nominal, Wilson 95 %, protégé contre
non protégé ; (4) chiffre de tête = campagne de VALIDATION (graine non vue), avec `main` et `final` à côté ;
(5) catégories d'échec et exemples rejouables (graines) ; (6) ce qui casse et reste cassé ; (7) doubles pannes,
à part ; (8) limites du modèle (§7.2 recopié) et biais possibles du simulateur ; (9) ce qui a été corrigé après
la première campagne, dit explicitement ; (10) commandes pour tout reproduire. Point d'état final dans `docs/decisions.md`.
Aucun chiffre qui ne sorte d'un `summary.json` ; aucun taux global sans le détail par classe ; aucune conclusion
plus forte que les intervalles. Si T-025 n'a pas abouti : rapport sur `results/main`, dit en première page.
RÉSERVE : démarre AU PLUS TARD à 75 % du jour 3 (>= 20 % du jour 3 réservés, non négociable). Séquentiel.
Critère de fin (vérifiable): `test -f docs/REPORT.md` ; chaque taux du tableau (3) retrouvé dans le
`summary.json` cité (contrôle par le manager sur 5 lignes tirées au hasard) ; `grep -c "Wilson" docs/REPORT.md`
>= 1 ; `grep -ci "simulation" docs/REPORT.md` >= 1 ; section « Limites » présente ; `./scripts/ci.sh` → `CI OK`.
## Compte rendu (rempli par l'agent)
