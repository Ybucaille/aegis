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
Fait : `sim/src/hw.rs` (`Hw: impl Hal`, watchdog 0xA5 puis alternance, NVM 256 o, files radio, cause de reset), `sim/src/world.rs` (SimConfig, World::{new,step,finish}, Snapshot, run, BOOT_DELAY=5, catch_unwind -> Panic, coupure/redémarrage à froid Brownout), `sim/src/lib.rs`. 27 tests sim verts (dont watchdog 10 s + boot 5 s, clés fausses/répétées, NVM/RAM, ligne coupée, TX 3,0 W, passe, déterminisme, brownout).
Commandes : `cargo fmt -p sim` ; `cargo clippy -p sim --all-targets -- -D warnings` (OK) ; `cargo test -p sim` (27 passed) ; `./scripts/ci.sh` -> CI OK. Pas de commit.
Choix / écarts (à relire) :
- `Fault` = enum placeholder (T-014) ; `faults` ignoré ; `protections` stocké, non vérifié (pas de feature `unprotected` côté firmware).
- Reset matériel : LoadSwitches = Radio seule (0b010), WdgTimeout = 10, WdgCtrl/RadioCfg = 0 et IGNORÉS par le sim (valeurs T-005 non connues). Kick recharge depuis le registre WdgTimeout (corrompu à 0 -> reset au tick suivant ; très grand -> watchdog quasi mort).
- Séquence : capteurs échantillonnés en début de tick, énergie/thermique intégrées en fin de tick avec les lignes en vigueur ; un reset en fin de tick N -> boot en fin de tick N+5, 1er step au tick N+6. Cause du redémarrage après coupure = Brownout (compté dans resets_by_cause).
- Radio : 1 trame émise par tick (la 2e = Busy) ; émission hors passe = perdue (comptée tx_lost) mais 3 W consommés ; montante gardée en file (16 max) et remise seulement en passe + ligne Radio alimentée.
- Raccourcis qui flattent un peu le firmware : TempObc = température batterie (un seul nœud) ; courants ligne = P/V sans bruit de charge propre ; capteurs figés pendant le tick (une seule mesure par tick, pas de lecture bruitée différente).
- catch_unwind laisse le hook de panique par défaut (message stderr) : la campagne multi-thread devra le neutraliser (T-015).
