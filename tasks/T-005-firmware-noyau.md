# T-005 Firmware : noyau (image RAM, ordonnanceur, modes, tâches de base)
Agent: systems   Modèle: Opus   Dépend de: T-002
Entrée: `docs/SPEC.md` §3.1, §4.1, §4.2, §5 (validation TC), annexe A ; `firmware/src/{hal,proto,lib}.rs`
Sortie attendue: `firmware/src/ram.rs` (cellules typées à décalage constant, `RAM_MAP` réel, `Unused` < 10 %),
`firmware/src/sched.rs` (exécutif cyclique, drapeaux d'exécution, `wdg_kick` conditionnel à clés alternées),
`firmware/src/mode.rs` (table de transitions complète, plafond en RAM), `firmware/src/tasks/{health,power,
thermal,comms_rx,telemetry,payload}.rs`, bouchons `tasks/{fdir,scrub}.rs` (le FDIR avance seulement son battement),
`firmware/src/testhal.rs` (`MockHal`, `#[cfg(test)]`). Dans le périmètre : tension validée A/B, thermostat,
balise à la période du mode, TC `Ping` et `SetMode` avec ACK, anti-rejeu en RAM, rejet silencieux.
Hors périmètre (jour 2) : TMR/ECC réels, NVM, FDIR, autres TC. Accès à l'état UNIQUEMENT via les cellules de
`ram.rs`, pour que T-010 puisse ajouter la protection sans réécrire les tâches.
Lot J1-B, parallèle avec T-006 et T-007 : n'écrit que dans `firmware/`. Un seul agent à la fois dans `firmware/src`.
Critère de fin (vérifiable): `cargo test -p firmware` → vert, avec au minimum : boot → `Survie` ; `Survie` → `Eco`
→ `Nominal` sur tension scénarisée (durées 60 s et uptime 120 s respectées) ; `V < 6720 mV` pendant 10 s → `Survie` ;
désaccord A/B → valeur basse + drapeau ; `Ping` acquitté ; trame rejouée, corrompue, tronquée → aucun ACK et
`cnt_tc_rejected` incrémenté ; watchdog rechargé à chaque cycle avec clés `0xA5`/`0x5A` alternées ; pas de
rechargement si une tâche critique n'a pas posé son drapeau ; `size_of::<Firmware>() == RAM_SIZE` ;
aucun `panic` sur 20 000 cycles avec `MockHal` nominal. `./scripts/ci.sh` → `CI OK`.
Ajout PRE-MORTEM (R5, état caché ; R13, tâche non critique muette) : critère de fin supplémentaire :
`grep -rnE "static mut|Atomic|Cell<|thread_local|lazy" firmware/src` → aucune ligne (aucun état hors de `ram`).
Écrire et tester l'abstraction de cellule de `ram.rs` AVANT les tâches. Les compteurs de période (balise, etc.)
sont bornés à la lecture (`min(valeur, période)`) : un compteur corrompu ne doit pas faire taire la radio.
Si la tâche déborde : réduire `payload` et `thermal` au minimum, aucun raccourci sur `ram.rs` ; le dire.
## Compte rendu (rempli par l'agent)
