# T-013 Simulateur : oracle de survie, verdicts, catégories, métriques
Agent: builder   Modèle: Sonnet   Dépend de: T-009
Entrée: `docs/SPEC.md` §8, §8.1, §8.2 (à implémenter À LA LETTRE, sans interprétation favorable) ; `sim/src/world.rs`
Sortie attendue: `sim/src/oracle.rs` (S1–S6, `OK_NOMINAL`/`OK_DEGRADED`, E0–E8 avec catégorie principale selon
l'ordre de la spec, `SILENT_LIE` = échec), `sim/src/metrics.rs` (`RunResult` : verdict, catégories, resets par
cause, délais injection → balise et → `Nominal`, SoC min, `T_batt` min/max, compteurs de la dernière balise,
région RAM touchée), `World::finish` branché. L'oracle ne lit QUE la vérité physique, la vue sol et
`StepReport.executed_tc` ; S5 compare chaque TC exécutée au registre des trames réellement émises par le sol.
L'oracle est écrit AVANT l'injecteur : la mesure existe avant ce qu'elle mesure.
Lot J2-A, parallèle avec T-010 et T-017 : n'écrit que dans `sim/src/`. Séquentiel dans `sim/` : T-013 → T-014 → T-015.
Critère de fin (vérifiable): `cargo test -p sim` → vert, avec au minimum un test PAR critère qui fabrique la
violation à la main sur un `World` (sans passer par l'injecteur) : S1 ligne Radio détruite → `VITAL_LOSS` ; S2
aucun ACK en dernière passe → `COMMS_LOST` ; S3 SoC final 19 % → `POWER` ; S4 301 s hors plage → `THERMAL` et
300 s → pas d'échec ; S5 TC exécutée deux fois → `CMD_INTEGRITY` ; S6 3 resets en dernière orbite → `BOOT_LOOP` ;
balise fausse de 201 mV sans drapeau → `SILENT_LIE` ; vol sans panne, graines 1..=20 → `OK_NOMINAL`.
`grep -n "ram_mut\|RAM_MAP" sim/src/oracle.rs` → aucune ligne. `./scripts/ci.sh` → `CI OK`.
Ajout PRE-MORTEM (R3) : interprétations à appliquer, que le manager consigne dans `docs/decisions.md` AVANT cette
tâche (précisions, pas de modification de la SPEC) : S2 « dernière passe » = dernière passe ENTIÈREMENT contenue
dans `[0, T_RUN]` ; S6 compte tout redémarrage de l'OBC, quelle qu'en soit la cause (coupure matérielle
comprise) ; S5 se mesure par comparaison octet à octet avec le registre des trames réellement émises (montantes
et descendantes), sans repasser par `firmware::proto`. Tests en plus : passe tronquée par `T_RUN` ignorée ;
trame descendante altérée puis acceptée par un `Ground` factice → `CMD_INTEGRITY`.
## Compte rendu (rempli par l'agent)
Fait : `sim/src/oracle.rs` (S1-S6, E0-E8, OK_NOMINAL/DEGRADED, registre des trames S5 octet à octet, passes complètes), `sim/src/metrics.rs` (`RunResult` étendu : verdict, outcome/catégories, délais, dernière balise, région RAM), `World` branché (`finish` évalue ; `truth_mut`, `note_ram_region`, `ground_receive` pub(crate) = point d'insertion canal pour T-014), `Hw::drain_delivered`, setters de test (cfg(test)) dans power/thermal.
Tests : un par critère sur World sans injecteur (S1 radio/batterie, S2 muet, S2 passe tronquée ignorée, S3 19 %, S4 301 s/300 s, S5 rejeu/inventée/TM altérée acceptée, S6 3 vs 2 resets, E8 201 mV/mode>60 s), 20 graines OK_NOMINAL.
Commandes : cargo fmt --all ; cargo clippy -p sim --all-targets -- -D warnings (vert) ; cargo test -p sim (46 + 1 + 2 verts) ; ./scripts/ci.sh -> CI OK ; grep ram_mut|RAM_MAP oracle.rs -> vide.
Écarts / choix : DEAD (>60 s sans cycle, OBC alimenté) compté comme échec même si S1-S6 vrais ; aucune passe complète => S2 faux ; OK_NOMINAL exige Payload+Radio réellement alimentées (isolement Heater non observable) ; E8 mode = incohérence continue >60 s sur les balises reçues. `Truth.destroyed_lines` n'a pas encore d'effet physique (T-014 doit le câbler) : en l'état une Payload « détruite » donne SILENT_LIE.
