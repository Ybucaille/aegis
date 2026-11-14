# T-015 Binaire `campaign` : exécution de masse, `runs.csv`, `summary.json`, rejeu
Agent: builder   Modèle: Sonnet   Dépend de: T-014
Entrée: `docs/SPEC.md` §7.1, §8.2, §8.3, ligne CLI de fin de §8.3
Sortie attendue: `sim/src/campaign.rs` + `sim/src/bin/campaign.rs` + `sim/src/json.rs` (écriture JSON à la main) :
options `--seed`, `--runs`, `--class C` (dont `none` = témoin sans panne et `double` = D1), `--unprotected`,
`--replay SEED` (rejoue un run et imprime sa trace), `--out`, `--threads N` ; graine du run i =
`splitmix64(graine XOR i)` ; répartition des classes garantissant >= 1000 runs par classe P1 pour 20 000 runs ;
`runs.csv` (colonnes de §7.1, ordre des lignes = `run_id`, indépendant du nombre de fils) ; `summary.json` :
par classe et global : effectif, survie, retour au nominal, intervalle de Wilson 95 %, décompte par catégorie
E0–E8, F1/F2 avec et sans région `Unused`, runs E0 exclus et listés ; graine, version git, options.
Lot J2-B, parallèle avec T-016 (dashboard) : n'écrit que dans `sim/`. T-016 ne lit que l'API publique de `sim`.
Critère de fin (vérifiable): `cargo run --release -p sim --bin campaign -- --seed 7 --runs 300 --out results/t015a`
puis la même avec `--threads 1 --out results/t015b` → `cmp results/t015a/runs.csv results/t015b/runs.csv` sans
différence ; `--replay <seed d'une ligne>` reproduit le verdict de la ligne ; test unitaire de Wilson (50/100 →
[0,404 ; 0,596] à 0,001 près) ; débit mesuré et écrit dans le compte rendu (cible >= 20 runs/s par fil, sinon le
signaler : la campagne du jour 3 en dépend). `./scripts/ci.sh` → `CI OK`.
Ajout PRE-MORTEM (R6, R8, R11) : `summary.json` ventile F1/F2 par TYPE de région de `RAM_MAP` (effectif, survie,
nominal, Wilson) et écrit la variante compilée (`protected` / `unprotected`) ; le binaire non protégé se compile
dans un répertoire cible séparé (`--target-dir target/unprotected`) pour ne pas recompiler à chaque bascule ni
contaminer le binaire protégé ; hook de panique silencieux pendant les runs (les paniques du firmware sont
comptées, pas imprimées) ; toute panique HORS `Firmware::step` reste un E0 visible.
## Compte rendu (rempli par l'agent)
