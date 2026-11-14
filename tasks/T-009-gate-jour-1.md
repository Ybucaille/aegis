# T-009 GATE FIN DE JOUR 1 : le satellite tourne sans pannes (multi-graines + calibration)
Agent: tester   Modèle: Haiku   Dépend de: T-008
Entrée: `docs/SPEC.md` §8 (S1–S6), annexe A (contraintes de calibration) ; `sim/tests/nominal.rs`
Sortie attendue: `sim/tests/gate_day1.rs` : (a) 20 graines (1..=20), vol de 3 orbites sans panne, mêmes assertions
que T-008 pour chacune, avec en plus : mode `Nominal` et ligne Payload alimentée à `T_RUN` ; (b) calibration :
bilan d'énergie positif sur une orbite en `Nominal` (SoC fin >= SoC début, départ 80 %) ; thermostat sain →
`T_batt` >= 0 °C sur 3 orbites ; (c) déterminisme : même graine deux fois → mêmes métriques.
Le tester NE CORRIGE PAS le firmware ni le sim et ne touche pas aux seuils : un test rouge se rapporte tel quel,
avec la graine et la première assertion fausse. Si (a) échoue pour raison de calibration (SoC initial bas +
départ en éclipse), le signaler : décision d'annexe A = manager, via `docs/decisions.md`.
**GATE** : le jour 2 ne démarre pas tant que le manager n'a pas lancé lui-même la commande ci-dessous et lu le résultat.
Critère de fin (vérifiable): `cargo test -p sim --test gate_day1 -- --nocapture` → `test result: ok`, 0 échec,
et la sortie liste 20 lignes `seed=N resets=0 mode=Nominal`. `./scripts/ci.sh` → `CI OK`.
Ajout PRE-MORTEM (R1, R3) : (d) grille de coins déterministe, en plus des 20 graines : `soc_init_pct` dans
{60, 80, 100} x `phase_init_s` dans {0, 712, 1425, 2137, 2850, 3562, 4275, 4987} = 24 vols ; une ligne
`coin soc=.. phase=.. mode_final=.. soc_final=.. balises_derniere_passe=.. acks_derniere_passe=..` par vol ;
assertion : mode final `Nominal` pour les 24. C'est LE contrôle du « 100 % `OK_NOMINAL` » du témoin : un coin
rouge se rapporte tel quel (pas de retouche de seuil), et le jour 2 ne démarre pas avant décision du manager.
## Compte rendu (rempli par l'agent)

Agent builder/tester (Haiku), 2026-11-14. Pas de commit. Fichiers écrits : `sim/tests/gate_day1.rs` et ce compte rendu.
Fait : (a) 20 graines (config par défaut), mêmes critères que `nominal_flight_seed1` + mode Nominal et Payload alimentée à T_RUN ;
(b) énergie : graines 1..=20 avec SoC initial 80 %, fenêtre = première orbite (5700 s), phase par défaut ; thermostat : T_batt min >= 0 °C sur les
vols (a) ; (c) déterminisme graine 1 deux fois ; (d) grille 3 SoC x 8 phases, graine 1. Échecs collectés, sortie complète, rien retouché.
Commandes : `./scripts/ci.sh` -> `CI OK` ; `cargo test -p sim --test gate_day1 -- --nocapture` -> `test result: ok. 1 passed; 0 failed`, 12,4 s.
Sortie : 20 lignes `seed=N resets=0 mode=Nominal` (seeds 1..20, toutes OK) ; 24 lignes `coin soc=.. phase=..` toutes `mode_final=Some(Nominal)`
(balises_derniere_passe=60, acks_derniere_passe=10 pour les 24) ; calibration seeds 1..20 : bilan +0,395 à +0,467 Wh, SoC fin 83,95 à 84,67 % (départ 80 %), mode fin Nominal ;
déterminisme seed 1 : identique=true ; `GATE J1 : 0 échec`.
Écarts : (1) la calibration (b) utilise la phase tirée par la graine (aucune phase imposée) et la première orbite du run ; (2) la dernière passe
des coins = dernière passe entièrement contenue dans le run ; (3) le test ne vérifie pas le « +balise à chaque passe » des coins, seulement la passe finale.
