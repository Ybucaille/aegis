# T-022 ★ Triage : corriger ou documenter
Agent: strategist   Modèle: Fable   Dépend de: T-021
Entrée: `results/main/failures.md`, les quatre `summary.json`, `docs/SPEC.md` §7.2 et §8, `docs/risks.md`
Sortie attendue: `docs/triage.md` : pour chaque couple (classe, catégorie) : cause établie par rejeu ou marquée
« hypothèse non vérifiée », décision CORRIGER / DOCUMENTER, raison en 2 lignes. Liste fermée de correctifs
(5 au plus), classés par gain attendu / coût, chacun avec le test de non-régression exigé ; cette liste devient
le contenu de T-023 et T-024. Entrée dans `docs/decisions.md`.
Règles : on ne corrige que des défauts du firmware ou des bogues du harnais (E0, injection sans effet) ; on ne
touche NI à l'oracle, NI aux seuils, NI au modèle de pannes (SPEC §8.3 règle 5). Une panne hors de portée du
design (D1, limites de §7.2) se documente, elle ne se « corrige » pas. Si aucun correctif ne vaut son risque :
le dire et passer directement à T-027 sur `results/main`.
Séquentiel. Budget : 10 % du jour 3.
Critère de fin (vérifiable): `docs/triage.md` existe et chaque ligne non vide du tableau (1) de `failures.md` y a
une décision (`grep -c "CORRIGER\|DOCUMENTER" docs/triage.md` >= nombre de couples) ; T-023 et T-024 complétées
par le manager avec la liste ; `git status --porcelain docs/SPEC.md` → vide.
## Compte rendu (rempli par l'agent)
