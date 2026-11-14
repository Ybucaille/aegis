# T-008 Intégration : firmware + sim + ground, vol nominal de 3 orbites
Agent: builder   Modèle: Sonnet   Dépend de: T-005, T-006, T-007
Entrée: `docs/SPEC.md` §3.1, §6, §8 (S1–S6 pour savoir quoi observer), annexe A ; comptes rendus de T-005/006/007
Sortie attendue: `sim/src/world.rs` complété (étapes 5 et 6 : canal radio dans les deux sens + `Ground`,
`World::queue_cmd`, vue sol dans `Snapshot`), `sim/tests/nominal.rs` (un vol de `T_RUN` = 17 100 s, graine fixe),
`sim/examples/fly.rs` (trace texte toutes les 300 s : t, éclipse, SoC, V, T_batt, mode, balises reçues, ACK).
Tout défaut trouvé dans `firmware/` ou `ground/` : correction d'une ligne au plus, sinon le rapporter au manager
(ne pas réécrire le firmware). Aucun réglage de l'annexe A sans entrée dans `docs/decisions.md`.
Séquentiel : seule tâche active (touche `sim/` et lit tout le reste).
Critère de fin (vérifiable): `cargo test -p sim --test nominal` → vert : sur la graine 1, 0 reset, mode `Nominal`
atteint et annoncé par la dernière balise, >= 1 balise et >= 1 `Ping` acquitté à CHAQUE passe, SoC final >= 20 %,
`T_batt` dans [-10, +50] °C. `cargo run -p sim --example fly -- --seed 1` → se termine avec code 0. `./scripts/ci.sh` → `CI OK`.
Ajout PRE-MORTEM (R1, R2) : dans `sim/tests/nominal.rs`, en plus : (1) démarrage à froid : `soc_init_pct = 3`,
début d'éclipse → OBC coupé, redémarre seul dans l'orbite suivante, balise reçue à la passe d'après ; (2) mesures
AFFICHÉES et recopiées dans le compte rendu, sans assertion : rapport cyclique du réchauffeur en éclipse, SoC en
début et fin de chaque éclipse, bilan en Wh par orbite en `Nominal`, et pour le cas (1) le nombre de secondes
consécutives passées sous -10 °C OBC éteint (dit si S4 serait violé). Ces chiffres servent aux décisions R1 / R2.
## Compte rendu (rempli par l'agent)
