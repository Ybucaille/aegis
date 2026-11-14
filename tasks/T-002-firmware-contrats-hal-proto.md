# T-002 Firmware : contrats partagés (`hal`, `proto`, squelette `Firmware`)
Agent: systems   Modèle: Opus   Dépend de: T-001
Entrée: `docs/SPEC.md` §3, §3.1, §5, annexe A
Sortie attendue: `firmware/src/hal.rs` (trait `Hal`, `SensorId`, `Reg`, `Line`, `ResetCause`, `HalError`,
`MAX_FRAME`, `NVM_SIZE`, `RAM_SIZE`, signatures EXACTES de la spec) ; `firmware/src/proto.rs` (CRC-32, encodage et
décodage de trame, types `Command`/`Ack`/`Beacon`, ordre de validation SYNC → type → LEN → CRC ; l'anti-rejeu
n'est pas ici) ; `firmware/src/lib.rs` (`Firmware::{new, boot, step, ram_mut}`, `StepReport`, `Mode`, `Region`,
`RAM_MAP` provisoire). `step` est un BOUCHON : reste en `Survie`, recharge le watchdog, n'émet rien.
But : débloquer `sim` et `ground` au plus tôt. Les signatures publiques ne changent plus après cette tâche
sans entrée dans `docs/decisions.md`.
Lot J1-A, parallèle avec T-003 et T-004 : n'écrit que dans `firmware/`.
Critère de fin (vérifiable): `cargo test -p firmware` → vert, avec au minimum : `crc32_check_vector`
(`"123456789"` → `0xCBF43926`), aller-retour encodage/décodage pour TC, ACK, BEACON (28 octets) et EVENTS,
rejet d'une trame avec 1 bit basculé, rejet de LEN > 48 et de trame tronquée, `size_of::<Firmware>() == RAM_SIZE`.
`cargo clippy -p firmware --all-targets -- -D warnings` → 0 avertissement. `cargo build --workspace` → vert.
Ajout PRE-MORTEM (R7, bogue symétrique sol/bord) : critère de fin supplémentaire : test `golden_frames` qui encode
ET décode les trois trames de l'annexe de `docs/risks.md` (octets calculés hors du projet avec `zlib.crc32`) et
retrouve exactement ces octets. Les octets sont recopiés en dur dans le test, jamais produits par `proto`.
## Compte rendu (rempli par l'agent)
