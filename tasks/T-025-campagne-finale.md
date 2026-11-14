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
(Rempli par le manager à partir du rapport de l'agent tester, qui n'a pas écrit cette section.)
Fait : campaign.sh avec AEGIS_PREFIX=final → 0 ; final 20 000, final-control 1000/1000 OK_NOMINAL, final-unprotected 20 000, final-double 1000, final-check500 cmp OK ; validation V=990274663 lancée une seule fois (binaire direct, sans --replay) → 20 000 runs. E0 = 0 partout.
Survie protégé main / final / validation : F5 97,20 % / 100 % / 100 % ; F10 71,99 % / 71,99 % / 70,87 % ; D1 0 / 0 / 0 ; autres classes inchangées. Régressions (main survivant, final en échec) : 0. Inverse : 40 (toutes F5, C1).
sha256 results/main/runs.csv = fa7739d2…dbab, inchangé. results/final/VERSION = e8bb46f (firmware/sim identiques à a59b7f5).
Écarts : (1) contrôle de propreté de campaign.sh restreint à firmware sim/src sim/tests sim/Cargo.toml (fichier d'exemple T-030 non suivi) ; (2) variable AEGIS_PREFIX ajoutée, défaut inchangé ; (3) check500 en plus. Vérifié par le manager : wc, sha256, témoin, DIFF.md (14 lignes de classes + section régressions).
