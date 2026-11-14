# T-020 Campagne d'injection principale (métriques brutes)
Agent: tester   Modèle: Haiku   Dépend de: T-019
Entrée: `docs/SPEC.md` §8.3 ; entrée « GEL DE CAMPAGNE » de `docs/decisions.md` (graine de campagne `S`) ;
`scripts/rehearsal.sh` comme modèle
Sortie attendue: `scripts/campaign.sh` et les résultats, sur le code tel que gelé (noter `git rev-parse HEAD` ou,
à défaut de commit, la somme `sha256` de `firmware/src` et `sim/src` dans `results/main/VERSION`) :
`results/control/` (1000 runs `--class none`), `results/main/` (>= 20 000 runs, >= 1000 par classe P1),
`results/unprotected/` (mêmes graines, `--unprotected`), `results/double/` (>= 1000 runs `--class double`).
Aucune modification de code, aucun run écarté, aucune relance « pour voir » avec une autre graine. Un run E0 se
compte et se liste. Si le témoin sans panne n'est pas à 1000/1000 : ARRÊT et rapport au manager, pas de suite.
Séquentiel : aucun agent n'écrit dans `firmware/` ni `sim/` pendant la campagne. Budget : 15 % du jour 3.
Critère de fin (vérifiable): `./scripts/campaign.sh` → code 0 ; `wc -l results/main/runs.csv` → >= 20 001 ;
`results/main/summary.json` donne effectif >= 1000 pour chaque classe P1 ; `results/control/summary.json` →
1000 `OK_NOMINAL` ; relance de `results/main` sur 500 runs (`--runs 500`) → 500 premières lignes identiques
(`cmp` sur `head -501`).
Ajout T-019 : dépend aussi de T-028 et T-029 (NO-GO levé par le manager). Graine `S` = 731051977. `scripts/campaign.sh` vérifie que `git status --porcelain firmware sim` est vide avant de lancer. Résultats écrits sous `results/` (jamais sous `/tmp` : quota partagé, R20).
## Compte rendu (rempli par l'agent)
