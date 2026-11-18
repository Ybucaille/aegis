# Aegis

*[English version](README.md)*

Aegis est une **simulation** sur PC : firmware de CubeSat tolérant aux radiations (Rust `no_std`),
simulateur, ground station simulée, dashboard web. Aucun matériel réel, **aucune certification** :
ce dépôt ne permet pas de conclure à une conformité spatiale. Cadre exact : `docs/SPEC.md`.

Prérequis : Rust 1.99 (vérifié avec cargo 1.99.1), hors ligne, sans dépendance externe.
Lancer les commandes depuis la racine du dépôt.

## Tests

```
cargo test --release -p firmware
```

La CI locale complète est `./scripts/ci.sh` ; ce README ne cite que les tests du firmware.

## Vol nominal (texte, sans panne)

```
cargo run --release -p sim --example fly -- --seed 1
```

Options : `--soc <pct>`, `--phase <s>`, `--duration <s>`.

## Campagne d'injection

```
cargo run --release -p sim --bin campaign -- --seed 42 --runs 14 --out results/readme-check/essai
```

Remplacer `results/readme-check/essai` par `results/<nom>`. Options : `--class` (`none`, `double`,
`f1`..`f13`, `d1` ; absent = mixte), `--threads N`. La campagne complète (au moins 20 000 runs) est
décrite dans `docs/SPEC.md` §8.3 et lancée par `scripts/campaign.sh`.

Témoin sans panne :

```
cargo run --release -p sim --bin campaign -- --seed 42 --runs 10 --class none --out results/readme-check/temoin
```

Témoin sans protections, dans un répertoire cible séparé pour ne pas écraser le binaire protégé :

```
cargo run --release --target-dir target/readme-check -p sim --features unprotected --bin campaign -- --unprotected --seed 42 --runs 14 --out results/readme-check/unprotected
```

## Rejeu d'une graine

La graine d'un run est la colonne `seed` de `runs.csv`. Le rejeu affiche la trace sur la sortie
standard et n'écrit aucun fichier :

```
cargo run --release -p sim --bin campaign -- --replay 13679457532755275413 --seed 42 --runs 14
```

## Dashboard

```
cargo run --release -p dashboard -- --port 8080
```

Écoute sur `127.0.0.1:8080` jusqu'à Ctrl+C. Routes : `GET /`, `GET /api/state`,
`GET /api/campaign?name=<dossier sous results/>`, `POST /api/fault`, `POST /api/cmd`.

## Résultats

Chaque campagne écrit `results/<nom>/runs.csv` (une ligne par run) et `summary.json`.

- `runs.csv` : `seed` (graine du run, pour `--replay`), `verdict` (`OK_NOMINAL`, `OK_DEGRADED`, ou
  la catégorie principale en échec), `categories` (E0 à E8 de SPEC §8.1, séparés par `+`), `survived`
  et `nominal` (1 ou 0), `criteria` (S1 à S6), `harness_msg` (non vide = run E0, exclu des taux).
- `summary.json` : `runs_total`, `runs_e0_exclus`, `global.stats` (`survie` avec `k`, `taux`,
  `wilson95`), `par_classe` (F1 à F13, D1, avec l'effectif `n`), `git` (suffixe `-dirty` si le dépôt
  était modifié).

Lire un taux avec son effectif, son intervalle de Wilson à 95 % et le détail par classe.
Les chiffres de survie ne sont donnés que dans `docs/REPORT.md`, rédigé en fin de projet.
