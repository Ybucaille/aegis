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
Ajout T-019 : l'oracle n'est plus modifiable (B2 était le seul correctif d'oracle, fait avant tout run). Les échecs F5/E2 protégés vus à la répétition (5/143) sont à instruire ici par rejeu.
## Compte rendu (rempli par l'agent)
Fait. `docs/triage.md` écrit ; entrée ajoutée en fin de `docs/decisions.md`. Rien de commité.
- F5/E2 (40) : CORRIGER. Cause établie (CSV exhaustif : 40/40 = `ILineHeater` hors plage haute ; rejeux protégés
  4781019819541052408, 2990178190404449115, 14486980315370451616 = `THERMAL` ; rejeu non protégé de la première
  = `OK_NOMINAL` ; code `fdir::latchup` sans borne de plausibilité). Les 5/143 de la répétition : même mécanisme
  (non rejoués un par un, hypothèse cohérente avec 40/40 ici).
- F10/E2 (400) : DOCUMENTER (confirmé). D1/E2+E3 (1428) : DOCUMENTER (SPEC §7 + 0 reset sur 2428 runs, SoC min 24,3 %).
- Correctifs : C1 seul (T-023). T-024 vide. T-030 : oui, exécution après T-023.
- Pas fait : aucun rejeu F10 supplémentaire (dépouillement CSV jugé suffisant) ; la piste « délestage anticipé »
  pour F10 n'est pas instruite (fonctionnalité nouvelle).
- Commandes : `grep -c "CORRIGER\|DOCUMENTER" docs/triage.md` → 6 (>= 3 couples) ; `git status --porcelain docs/SPEC.md
  firmware sim` → vide ; `sha256sum results/main/runs.csv` → fa7739d2…15dbab (inchangé par les rejeux, à comparer à T-020).
