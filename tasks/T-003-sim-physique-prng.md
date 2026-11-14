# T-003 Simulateur : PRNG, paramètres, orbite, énergie, thermique, capteurs
Agent: builder   Modèle: Sonnet   Dépend de: T-001
Entrée: `docs/SPEC.md` §6 (modèles), §7.1, annexe A
Sortie attendue: `sim/src/prng.rs` (`splitmix64`, `xoshiro256**`, uniforme, gaussienne ; aucun appel horloge/OS),
`sim/src/params.rs` (`Params` = annexe A, `Default`), `sim/src/orbit.rs` (éclipse, passe sol, phase initiale),
`sim/src/power.rs` (bilan d'énergie borné, tension linéaire, `SOC_OFF`/`SOC_ON`), `sim/src/thermal.rs` (un nœud),
`sim/src/sensors.rs` (vérité + bruit quantifié). Modules purs : aucune dépendance à `firmware` dans cette tâche.
Lot J1-A, parallèle avec T-002 et T-004 : n'écrit que dans `sim/src/` (sauf `world.rs`, `hw.rs`, `bin/`).
Critère de fin (vérifiable): `cargo test -p sim` → vert, avec au minimum : même graine → même suite (et vecteur
de référence `xoshiro256**` figé dans le test) ; fraction d'éclipse = 2100/5700 et une passe de 600 s par orbite ;
sans réchauffeur `T_batt` < -10 °C en moins de 1500 s d'éclipse ; réchauffeur permanent → `T_batt` >= 0 °C ;
coupure sous 5 % et reprise au-dessus de 15 % ; `V(50 %) = 7200 mV`. Clippy `-D warnings` vert.
Ajout PRE-MORTEM (R1, calibration) : `Params` expose deux surcharges optionnelles, `soc_init_pct` et `phase_init_s`
(absentes = tirage au hasard), pour que T-008 et T-009 puissent tester les coins sans toucher au sim. Test en plus :
thermostat idéal (ON < +2 °C, OFF > +8 °C) sur une éclipse → rapport cyclique du réchauffeur AFFICHÉ, et bilan
d'une orbite en charges nominales (1,8 W + balise 3 W x 1 s / 10 s + réchauffeur selon thermostat) affiché en Wh.
Si le bilan est négatif : ne rien régler, le dire en première ligne du compte rendu (décision d'annexe A = manager).
## Compte rendu (rempli par l'agent)
**Bilan : positif (aucun réglage d'annexe A nécessaire).**
Fait : `sim/src/{prng,params,orbit,power,thermal,sensors}.rs` + `lib.rs` (déclare les 6 modules ; `world`/`hw` à ajouter par T-004).
Commandes (résultats réels) :
- `cargo fmt --all -- --check` -> OK
- `cargo clippy -p sim --all-targets -- -D warnings` -> OK
- `cargo test -p sim -- --nocapture` -> 15 passed, 0 failed
Mesures : sans réchauffeur, -10 °C franchi après 1253 s d'éclipse (< 1500) ; réchauffeur permanent T min 15,1 °C ;
thermostat idéal : rapport cyclique 32,9 % en éclipse, 12,1 % sur l'orbite, T min 2,0 °C ;
bilan orbite nominale (OBC+RX+payload 1,8 W + balise 3 W x 1 s/10 s + réchauffeur) : produit 4,00 Wh, consommé 3,61 Wh, net +0,39 Wh.
Vecteur xoshiro256** (graine 42) figé, recalculé indépendamment en Python ; splitmix64(0)=0xE220A8397B1DCDAF.
Choix hors annexe A (à consigner dans decisions.md) : thermique tau = 1000 s, R = 40 K/W, T_env soleil +15 °C / éclipse -20 °C ;
passe sol à phase 1500..2100 de l'orbite (en plein soleil, éclipse = fin d'orbite [3600, 5700)) ; gaussienne = somme de 12
uniformes (queues tronquées à ±6 sigma, donc pas d'événements extrêmes de capteur : léger biais favorable, assumé).
Écarts / biais : le test "sans réchauffeur" part de l'équilibre soleil (+15 °C) ; la charge OBC coupée met toutes les charges à 0
(le réchauffeur est donc aussi coupé sous SOC_OFF : T_batt peut alors descendre, réaliste mais à surveiller en campagne).
Pas de commit.
