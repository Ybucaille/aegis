# T-011 Firmware : NVM triple + CRC, jeu de TC complet, journal d'événements
Agent: systems   Modèle: Opus   Dépend de: T-010
Entrée: `docs/SPEC.md` §4.3 (NVM), §5 (tout), §3 (NVM, reset) ; `firmware/src/{proto,ram}.rs`, `tasks/comms_rx.rs`
Sortie attendue: `firmware/src/nvm.rs` (bloc persistant `boot_count`, `last_tc_seq`, plafond, isolations ; 3 copies
+ CRC-32 ; lecture de la première valide puis réparation ; aucune valide → défauts + `NVM_LOST`),
`firmware/src/log.rs` (journal borné protégé SECDED), `tasks/comms_rx.rs` et `tasks/telemetry.rs` complétés :
TC `Reboot` (reset APRÈS l'ACK), `PowerCycle`, `ClearFdir` (point d'accroche pour T-012), `DumpEvents` (trames
EVENTS) ; `last_tc_seq` écrit en TMR + NVM AVANT exécution ; fenêtre `last < SEQ <= last + 1024` ; `boot`
relit la NVM et incrémente `boot_count` ; balise complète (28 octets, tous les champs réels).
Séquentiel dans `firmware/src` (après T-010, avant T-012). Parallèle avec les tâches `sim/` et `dashboard/`.
Critère de fin (vérifiable): `cargo test -p firmware` → vert, avec au minimum : chaque opcode → ACK attendu
(`Ok`, `BadArg` sur argument invalide) ; rejeu d'une TC acceptée APRÈS un reset → rejetée (anti-rejeu persistant) ;
SEQ = last + 1025 → rejetée ; `Reboot` : ACK émis puis `sys_reset` appelé ; NVM : 1 copie corrompue → réparée,
2 corrompues → réparées, 3 corrompues → défauts + `NVM_LOST` dans la balise, sans `panic` ; NVM aléatoire
(1000 images) → `boot` ne panique jamais ; `HalError` sur `nvm_write` → pas de `panic`.
`./scripts/ci.sh` → `CI OK` ; `cargo test -p sim --test gate_day1` toujours vert.
Ajout PRE-MORTEM (R9, état persistant empoisonné ; R4) : critères de fin supplémentaires : fenêtre anti-rejeu en
arithmétique saturante (`last_tc_seq` = `u32::MAX - 5` puis TC quelconque → rejet sans `panic`) ; 1000 blocs
NVM à CRC VALIDE et contenu aléatoire → `boot` puis 300 cycles sans `panic`, chaque champ relu est borné.
Fichiers de cette tâche si lancée en parallèle de T-012 (décision du manager, voir T-010) : `nvm.rs`, `log.rs`,
`tasks/comms_rx.rs`, `tasks/telemetry.rs` seulement. Repli de modèle admis : Sonnet. Ordre de sacrifice si
retard : `DumpEvents` + journal, puis `PowerCycle` ; jamais la NVM ni l'anti-rejeu persistant. Le dire.
## Compte rendu (rempli par l'agent)
