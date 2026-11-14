# T-025 Campagne finale après corrections (même graine + graine de validation)
Agent: tester   Modèle: Haiku   Dépend de: T-023, T-024
Entrée: `scripts/campaign.sh`, entrée « GEL DE CAMPAGNE » de `docs/decisions.md` (graines `S` et `V`), `docs/triage.md`
Sortie attendue: `results/final/` (graine `S`, mêmes options que `results/main`), `results/final-unprotected/`,
`results/final-control/`, `results/final-double/`, et `results/validation/` : >= 20 000 runs sur la graine de
validation `V`, jamais utilisée avant (elle protège contre des correctifs ajustés aux graines vues).
`results/final/DIFF.md` : par classe, survie avant (`main`) / après (`final`) / validation, avec effectifs ;
liste des graines survivantes dans `main` et en échec dans `final` (régressions), sans les minimiser.
`results/main/` n'est NI supprimé NI écrasé. Si T-023 et T-024 sont vides : ne lancer que `results/validation/`.
Séquentiel : code figé pendant les runs. Budget : 10 % du jour 3. Doit être finie à 75 % du jour 3.
Critère de fin (vérifiable): `wc -l results/final/runs.csv results/validation/runs.csv` → >= 20 001 chacun ;
`results/final-control/summary.json` → 1000 `OK_NOMINAL` ; `results/main/runs.csv` inchangé (somme `sha256`
identique à celle notée en T-020) ; `DIFF.md` contient une ligne par classe F1–F13 et la section « régressions »
(éventuellement « aucune »).
Ajout T-019 : graine `V` = 990274663 (jamais lancée avant cette tâche, y compris par `--replay`).
## Compte rendu (rempli par l'agent)
