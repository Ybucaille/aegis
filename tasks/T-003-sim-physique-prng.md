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
