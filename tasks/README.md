# Plan de tâches Aegis (phase 0, 2026-11-14)

27 tâches. Contrat : `docs/SPEC.md`. Taille : S < 1 h d'agent, M = 1 à 2 h, L = 2 à 4 h (estimations, non mesurées).
Une tâche n'est « faite » que lorsque le MANAGER a lancé lui-même la commande du critère de fin.

| Id | Titre | Agent (modèle) | Jour | Taille | Dépend de | Lot parallèle |
|---|---|---|---|---|---|---|
| T-001 | Workspace cargo, 4 crates | builder (Sonnet) | 1 | S | — | seul |
| T-002 | Firmware : contrats `hal`, `proto`, squelette | systems (Opus) | 1 | M | 001 | J1-A |
| T-003 | Sim : PRNG, orbite, énergie, thermique, capteurs | builder (Sonnet) | 1 | M | 001 | J1-A |
| T-004 | Script de CI locale | tester (Haiku) | 1 | S | 001 | J1-A |
| T-005 | Firmware : noyau (RAM, ordonnanceur, modes, tâches) | systems (Opus) | 1 | L | 002 | J1-B |
| T-006 | Sim : matériel simulé (`impl Hal`) + `World` | builder (Sonnet) | 1 | L | 002, 003 | J1-B |
| T-007 | Ground station | builder (Sonnet) | 1 | M | 002 | J1-B |
| T-008 | Intégration bout en bout, vol nominal | builder (Sonnet) | 1 | M | 005, 006, 007 | seul |
| T-009 | **GATE J1** : 20 graines sans panne + calibration | tester (Haiku) | 1 | S | 008 | seul |
| T-010 | Firmware : TMR, SECDED, scrub | systems (Opus) | 2 | L | 009 | J2-A |
| T-013 | Sim : oracle, verdicts, métriques | builder (Sonnet) | 2 | M | 009 | J2-A |
| T-017 | Fuzzing maison du protocole | tester (Haiku) | 2 | M | 009 | J2-A |
| T-011 | Firmware : NVM, TC complètes, journal | systems (Opus) | 2 | L | 010 | J2-B |
| T-014 | Sim : injecteur de pannes F1–F13, D1 | builder (Sonnet) | 2 | L | 013 | J2-B |
| T-012 | Firmware : FDIR, battement, registres, tempête | systems (Opus) | 2 | L | 011 | J2-C |
| T-015 | Binaire `campaign` | builder (Sonnet) | 2 | M | 014 | J2-C |
| T-016 | Dashboard | builder (Sonnet) | 2 | M | 014 | J2-C |
| T-018 | Répétition générale + **contrôles GATE J2** | tester (Haiku) | 2 | M | 012, 015 | seul |
| T-019 | ★ Revue fin J2, gel de campagne | strategist (Fable) | 2 | S | 018, 016 | seul |
| T-020 | Campagne principale (>= 20 000 runs) | tester (Haiku) | 3 | M | 019 | seul |
| T-021 | Extraction et rejeu des échecs | tester (Haiku) | 3 | S | 020 | J3-A |
| T-026 | README utilisateur | tester (Haiku) | 3 | S | 019 | J3-A |
| T-022 | ★ Triage corriger / documenter | strategist (Fable) | 3 | S | 021 | seul |
| T-023 | Corrections firmware (liste fermée) | systems (Opus) | 3 | M | 022 | J3-B |
| T-024 | Corrections harnais (liste fermée) | builder (Sonnet) | 3 | S | 022 | J3-B |
| T-025 | Campagne finale + graine de validation | tester (Haiku) | 3 | M | 023, 024 | seul |
| T-027 | ★ Rapport final | strategist (Fable) | 3 | M | 025 | seul |

## Lots parallèles (fichiers disjoints)

| Lot | Tâches | Répertoires réservés |
|---|---|---|
| J1-A | T-002 / T-003 / T-004 | `firmware/` / `sim/src/` (hors `world.rs`, `hw.rs`) / `scripts/` |
| J1-B | T-005 / T-006 / T-007 | `firmware/` / `sim/` / `ground/` |
| J2-A | T-010 / T-013 / T-017 | `firmware/src/` / `sim/src/` / `firmware/tests/` + `ground/tests/` |
| J2-B | T-011 / T-014 | `firmware/src/` / `sim/` |
| J2-C | T-012 / T-015 / T-016 | `firmware/src/` / `sim/` / `dashboard/` |
| J3-A | T-021 / T-026 | `results/main/failures.md` + `scripts/failures.sh` / `README.md` |
| J3-B | T-023 / T-024 (+ T-026 si non finie) | `firmware/` / `sim/` + `ground/` + `dashboard/` |

Règles de séquencement :
- **Un seul agent à la fois dans `firmware/src`** (T-002 → 005 → 010 → 011 → 012 → 023) et **un seul dans
  `sim/src`** (T-003 → 006 → 008 → 013 → 014 → 015 → 024). Ces deux chaînes avancent en parallèle l'une de l'autre.
- `sim` compile contre le firmware pendant que celui-ci change : les signatures publiques de T-002 sont figées.
  Un agent `sim` qui voit un test firmware rouge le signale, il ne le corrige pas.
- Pendant T-018, T-020 et T-025 (runs de mesure), personne n'écrit dans `firmware/` ni `sim/`.

## Chemin critique

T-001 → T-002 → T-005 → T-008 → T-009 (gate J1) → T-010 → T-011 → T-012 → T-018 → T-019 (gate J2)
→ T-020 → T-021 → T-022 → T-023 → T-025 → T-027.

La chaîne firmware du jour 2 (trois tâches L à la suite, Opus) est le goulot. La chaîne `sim` a de la marge ;
T-016 (dashboard) et T-017 sont hors chemin critique. Si le jour 2 déborde : réduire T-016 d'abord, jamais T-013/T-018.

## Gates

| Gate | Commande lancée par le manager | Résultat exigé |
|---|---|---|
| Fin J1 | `./scripts/ci.sh && cargo test -p sim --test gate_day1 -- --nocapture` | `CI OK`, 20 lignes `seed=N resets=0 mode=Nominal`, 0 échec |
| Mi-J2 (gel de périmètre) | après T-011 et T-014 | plus aucune fonctionnalité ; tout ajout → `docs/later.md` |
| Fin J2 | `./scripts/rehearsal.sh` puis lecture de `results/rehearsal/NOTES.md` | témoin 1000/1000 `OK_NOMINAL`, 0 E0, écart net protégé / non protégé, entrée « GEL DE CAMPAGNE » |
| J3 à 70 % | — | correctifs non verts retirés |
| J3 à 75 % | — | T-027 démarre, quoi qu'il arrive (sur `results/main` si T-025 n'est pas finie) |

## Budget du jour 3

Campagne 15 % · extraction 10 % · triage 10 % · corrections 25 % · campagne finale 10 % · **rapport 25 %** · marge 5 %.

## Points ouverts à trancher par le manager (entrée `docs/decisions.md`, sans toucher à la spec)

1. `SimConfig.protections` (valeur à l'exécution) contre feature cargo `unprotected` (à la compilation) : le plan
   retient une feature `sim/unprotected` qui active `firmware/unprotected`, avec erreur explicite si le drapeau
   et le binaire ne concordent pas (T-010, T-012, T-014). `scripts/` lance donc deux compilations.
2. Valeurs de `--class` hors table des pannes : `none` (témoin sans panne) et `double` (D1), définies en T-015.
3. Calibration de l'annexe A : avec un SoC initial de 60 % et un départ en éclipse, le retour en `Nominal` avant
   `T_RUN` et le « 100 % `OK_NOMINAL` » du témoin ne sont pas acquis. T-009 le mesure ; tout réglage passe par
   `docs/decisions.md` avant le gel de campagne (T-019).

## Ajouts du PRE-MORTEM (2026-11-14, détail dans `docs/risks.md`)

- Tâches complétées (ligne « Ajout PRE-MORTEM » dans chaque fichier) : T-002, T-003, T-005, T-008, T-009, T-010,
  T-011, T-012, T-013, T-015, T-017, T-018. Aucune dépendance ni taille changée, sauf T-008 et T-009 (S → M possible).
- **Gate J1 renforcée** : la sortie de `gate_day1` doit aussi lister 24 lignes `coin ...` toutes en `Nominal`
  (T-009 d). Sinon : décision d'annexe A avant T-010, pas pendant le jour 2.
- **Option de mise en parallèle T-011 / T-012** (exception à « un seul agent dans `firmware/src` ») : seulement si
  T-010 n'est pas close à 35 % du jour 2, et seulement parce que T-010 a figé `ram.rs` ; fichiers disjoints listés
  dans les deux tâches. Les points d'accroche communs (`ClearFdir`, isolations en NVM) passent par les cellules
  réservées, pas par des appels croisés. Décision à consigner.
- **Instantanés** : le manager fige l'état du code (commit local, jamais de push, ou copie datée) à chaque gate,
  avant T-020 et avant T-023/T-024 ; sans cela la règle de retrait des correctifs à 70 % du J3 est inapplicable.
- Décisions attendues du manager : liste en §4 de `docs/risks.md`.
