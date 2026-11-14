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

Fait : `scripts/campaign.sh` écrit et lancé une fois (code retour 0, `results/campaign.log`, `EXIT=0`). Graine S = 731051977 seule ; V non utilisée. Aucune modification de `firmware/` ni `sim/`, aucun run écarté, aucune relance avec une autre graine. Non commité.
Écarts : (a) `results/main/VERSION` = `7c4e865893c68f128dd74b121f19f1160286bb96` (HANDOFF) et non `0c32759…` (tag `campagne-gelee`) : un commit `7c4e865` (HANDOFF.md seul, `git diff 0c32759 7c4e865 -- firmware sim` vide) a été posé pendant la campagne, avant la fin du script. Le code est identique au tag. (b) `summary.json` porte `git` = `…-dirty` car `results/` est non ignoré (aucune modification de code). (c) `results/campaign_durations.csv`, `results/campaign.log` et le dossier `results/` sont non suivis.
Commandes : `./scripts/campaign.sh` → 0. Contrôle : `cmp head -501 results/main/runs.csv` vs `results/main-check500/runs.csv` → identique.
Effectifs (`results/main/summary.json`, 20000 runs, 0 E0 exclu) : F1..F8 = 1429 ; F9, F10, F11 = 1428 (min P1 = 1428 ≥ 1000). F12, F13, D1 = 1428.
Témoin `results/control` : 1000/1000 OK_NOMINAL. Double `results/double` : 1000 runs, survie 0/1000. E0 : 0 (`runs_e0_exclus` = 0 sur les 4 campagnes).
Survie brute (sans grille de catégories, à publier avec Wilson par le rapport) : protégé main 18132/20000 ; non protégé 16313/20000 (`results/unprotected`) ; relance 500 : 453/500.
Durées (`results/campaign_durations.csv`) : control 14,1 s ; main 258,1 s ; unprotected 126,1 s ; double 4,7 s ; check500 6,1 s (total ≈ 409 s hors builds).
