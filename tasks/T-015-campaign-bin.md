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
Fait : `sim/src/campaign.rs` (options, `execute`, `run_campaign` multi-fils par compteur atomique + tri par `run_id`, `runs.csv`, `summary`, Wilson, `locate`/`replay`), `sim/src/json.rs` (écriture à la main, clés ordonnées), `sim/src/bin/campaign.rs`, `sim/tests/campaign.rs`, `sim/Cargo.toml` (`unprotected = ["firmware/unprotected"]`, la feature firmware existait). Classe du run i = ALL[i mod 14] (F1..F13, D1 : 20 000 runs -> >= 1428 par classe, testé) ; `--class none|double|fN`. Graine run = `splitmix64(seed XOR i)`.
`summary.json` : variante protected/unprotected, git (+`-dirty`), options, par classe (n, survie, retour nominal, Wilson 95 %, E1..E8 en catégorie principale et « toutes applicables », injections jamais appliquées comptées à part + stats hors celles-ci, paniques firmware comptées), global marqué « jamais sans détail », F1/F2 : ventilation par type de région (Sched, Mode, Fdir, Proto, Tm, Log, Unused), avec/sans Unused, E0 exclus et listés, débit.
Paniques : hook muet global (`install_quiet_panic_hook`) ; panique firmware rattrapée par `World::step` = reset cause Panic, comptée ; toute autre panique (ou erreur `try_new`) = E0 ligne `HARNESS` avec message, exclue des taux, listée dans summary et sur stderr.
`--unprotected` doit correspondre au binaire compilé (sinon erreur avec la commande à lancer) ; témoin : `cargo build --release --target-dir target/unprotected -p sim --features unprotected --bin campaign`.
Rejeu : `--replay SEED` ; sans `--class`, retrouve la classe en balayant `--runs` runs de `--seed` ; la ligne CSV rejouée est identique (vérifié par diff et test).
Commandes/résultats : campagne `--seed 7 --runs 300` (16 fils) puis `--threads 1` : `cmp runs.csv` identique. `cargo fmt`, `cargo clippy -p sim --all-targets -- -D warnings` verts ; `cargo test -p sim` (debug) et `--release` verts (59 unitaires + 3+18+1+2) ; `./scripts/ci.sh` -> CI OK (le firmware en cours de T-010 compilait à ce moment).
Débit (release, 300 runs, firmware T-010 partiel) : 15,0 runs/s à 1 fil (SOUS la cible 20) ; 115 runs/s à 16 fils (7,2/fil, probablement SMT/mémoire) ; témoin unprotected 134 runs/s à 16 fils. Mesure : sim seul sans firmware ~30 ms/run, avec firmware ~70-100 ms/run : le firmware domine. Extrapolation : 20 000 runs ~ 3 min à 16 fils, ~22 min à 1 fil : acceptable pour le J3, mais la cible par fil n'est pas atteinte.
Non fait / à savoir : GroundSummary non désactivé (historique balises nécessaire aux délais ; coût négligeable devant le firmware) ; pas de chronomètre dans runs.csv (déterminisme) ; en `--class X --replay`, le run_id affiché vaut le premier match du balayage ou 0.
Observation brute (300 runs mixtes, firmware partiel) : 250/300 survivent ; 0 E0 ; F1 : 18 runs hors Unused. Sorties de contrôle dans results/t015a, t015b, t015u (non versionnées).
