# T-026 README : lancer, rejouer, lire les résultats
Agent: tester   Modèle: Haiku   Dépend de: T-019
Entrée: `docs/SPEC.md` §1, §2, §8.3 (CLI), §9 ; `scripts/` ; aide `--help` de `campaign` et `dashboard`
Sortie attendue: `README.md` à la racine, 80 lignes max : ce qu'est Aegis (une SIMULATION, aucune certification),
prérequis (Rust 1.99, hors ligne), commandes copiables : tests, vol nominal, campagne, rejeu d'une graine,
témoin sans protections, dashboard ; où sont les résultats et comment lire `runs.csv` / `summary.json` ;
renvoi vers `docs/REPORT.md` pour les chiffres (AUCUN chiffre de survie dans le README). Chaque commande citée
doit avoir été lancée.
Lot J3-A, parallèle avec T-021, T-023, T-024 : n'écrit que `README.md`.
Critère de fin (vérifiable): chaque bloc de commande du README lancé tel quel → code 0 (liste commande/résultat
dans le compte rendu) ; `wc -l README.md` → <= 80 ; `grep -ci "certif" README.md` → >= 1.
## Compte rendu (rempli par l'agent)

- Fait : `README.md` écrit à la racine (78 lignes). Commandes copiées telles quelles et lancées ci-dessous.
  Sorties sous `results/readme-check/` (`log.txt`, `log-verif.txt`, `verif/`). Pas de chiffre de survie, renvoi vers `docs/REPORT.md`.
- Pas fait : `./scripts/ci.sh` non lancé (consigne) ; `scripts/campaign.sh` cité en prose, non lancé (écrit dans `results/main`).
  Remplacement annoncé dans le README : `cargo test --release -p firmware` à la place de `ci.sh`.
- Écarts :
  - Le bloc dashboard n'a pas code 0 : le serveur ne se termine pas. Vérifié avec `timeout 30` (124, attendu), puis
    même binaire sur le port 8081 : `GET /` 200, `GET /api/state` 200.
  - `results/readme-check` n'est pas dans `.gitignore` (seul `results/t0*/` l'est) : ces fichiers apparaîtront en `??`. `.gitignore` non modifié (hors périmètre).
  - Un dashboard de test lancé sur le port 8099 : `pkill` a tué mon shell (code 144), puis l'arrêt explicite a été refusé
    par le classifieur. `pgrep -x dashboard` ensuite : aucun processus. À vérifier par le manager.
  - Build témoin dans `target/readme-check` (et non `target/unprotected`) pour ne pas toucher au binaire de campagne.
- Commande → code retour (passe finale, README tel que livré) :
  - `cargo test --release -p firmware` → 0
  - `cargo run --release -p sim --example fly -- --seed 1` → 0
  - `cargo run --release -p sim --bin campaign -- --seed 42 --runs 14 --out results/readme-check/essai` → 0
  - `cargo run --release -p sim --bin campaign -- --seed 42 --runs 10 --class none --out results/readme-check/temoin` → 0
  - `cargo run --release --target-dir target/readme-check -p sim --features unprotected --bin campaign -- --unprotected --seed 42 --runs 14 --out results/readme-check/unprotected` → 0
  - `cargo run --release -p sim --bin campaign -- --replay 13679457532755275413 --seed 42 --runs 14` → 0
  - `cargo run --release -p dashboard -- --port 8080` (avec `timeout 30`) → 124 (serveur, attendu)
  - `wc -l README.md` → 78 ; `grep -ci "certif" README.md` → 1 ; `grep -c '%' README.md` → 1 (« 95 % » = niveau de l'intervalle, pas un taux)
  - Graines interdites (731051977, 990274663) : absentes des essais et du README.
