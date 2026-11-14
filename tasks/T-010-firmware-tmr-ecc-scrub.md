# T-010 Firmware : TMR, SECDED, tâche `scrub`
Agent: systems   Modèle: Opus   Dépend de: T-009
Entrée: `docs/SPEC.md` §4.3, §4.4 (ligne « incohérence d'état »), §8.3 règle 2 ; `firmware/src/ram.rs`
Sortie attendue: `firmware/src/tmr.rs` (3 copies, vote bit à bit ou par valeur — consigner le choix —, réécriture
à la lecture, absence de majorité → signalée), `firmware/src/ecc.rs` (SECDED (13,8) ou (39,32), choix consigné
dans `docs/decisions.md`), `firmware/src/tasks/scrub.rs` (tourniquet, image complète en <= 60 s), `ram.rs`
adapté : cellules TMR pour mode, plafond, `last_tc_seq`, drapeaux d'ordonnanceur, état d'isolation ; SECDED pour
journal et tampons TM ; compteurs `cnt_tmr_fix` et `cnt_ecc_corr` publiés dans la balise. Feature cargo
`unprotected` : vote et ECC neutralisés (lecture de la copie 0, pas de correction). L'API des cellules vue par
les tâches ne change pas.
Lot J2-A, parallèle avec T-013 et T-017 (crates disjointes). Séquentiel dans `firmware/src` : T-010 → T-011 → T-012.
Critère de fin (vérifiable): `cargo test -p firmware` → vert, avec au minimum : SECDED exhaustif (tout mot, toute
erreur simple corrigée, toute erreur double détectée et jamais « corrigée » en faux) ; TMR : 1 copie altérée →
valeur juste et copie réparée, 2 copies altérées différemment → incohérence signalée ; test de masse : pour CHAQUE
bit de chaque région TMR/ECC de `RAM_MAP`, basculement puis 120 cycles `MockHal` → même mode et mêmes trames
émises qu'un témoin sans basculement ; scrub : bit basculé jamais relu par ailleurs → corrigé en <= 60 cycles.
`cargo test -p firmware --features unprotected` → compile et passe (tests de protection marqués `cfg(not)`).
`./scripts/ci.sh` → `CI OK` ; `cargo test -p sim --test gate_day1` toujours vert.
Ajout PRE-MORTEM (R4, R8, MBU) : (1) vote TMR **bit à bit** recommandé et les 3 copies rangées dans 3 blocs
distincts de l'image (jamais adjacentes) : un MBU de 2 à 4 bits ne doit pas toucher deux copies d'une même
cellule ; test : tout MBU de 2 à 4 bits adjacents dans une région TMR → valeur juste. (2) `ram.rs` RÉSERVE dès
cette tâche toutes les cellules dont T-011 et T-012 auront besoin (miroir NVM, journal, état FDIR par ligne,
compteurs de boot, masques capteurs) et fige `RAM_MAP` : T-011 et T-012 ne modifient plus `ram.rs`, ce qui permet
de les lancer en parallèle si T-010 n'est pas close à 35 % du jour 2. (3) `cfg(feature = "unprotected")`
confiné à `tmr.rs` et `ecc.rs` (un point chacun), jamais dans les tâches.
## Compte rendu (rempli par l'agent)
