# T-006 Simulateur : matériel simulé (`impl Hal`) et `World` sans pannes
Agent: builder   Modèle: Sonnet   Dépend de: T-002, T-003
Entrée: `docs/SPEC.md` §3 (watchdog, NVM, registres), §3.1 (séquence d'un tick), §6 ; `firmware/src/hal.rs`
Sortie attendue: `sim/src/hw.rs` (struct matériel : registres, interrupteurs de lignes, NVM 256 octets, watchdog
à clés alternées, files radio montante/descendante, cause de reset, `impl firmware::Hal`), `sim/src/world.rs`
(`SimConfig`, `World::{new, step, finish}`, `Snapshot`, `run`, séquence en 7 étapes, `BOOT_DELAY`, coupure et
redémarrage à froid, `catch_unwind` autour de `Firmware::step` → `ResetCause::Panic`), `sim/src/lib.rs`.
`World` appelle le firmware réel (bouchon de T-002 tant que T-005 n'est pas fini). Le sol n'est PAS branché ici
(T-008) : exposer les deux files radio par des méthodes. `faults` est accepté mais ignoré, `finish` renvoie des
métriques brutes sans verdict (oracle = T-013).
Lot J1-B, parallèle avec T-005 et T-007 : n'écrit que dans `sim/` ; ne modifie aucun fichier de T-003 sauf correction signalée.
Critère de fin (vérifiable): `cargo test -p sim` → vert, avec au minimum : watchdog non rechargé → reset après
10 s puis `boot` après 5 s ; clé répétée ou fausse → pas de rechargement ; NVM conservée au reset, RAM effacée ;
ligne coupée → capteur de courant à ~0 et charge non comptée ; émission radio = 3,0 W pendant 1 s ; trame montante
remise seulement pendant la passe ; deux `run` de même graine → `Snapshot` final identique. `./scripts/ci.sh` → `CI OK`.
## Compte rendu (rempli par l'agent)
