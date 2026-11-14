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
Agent systems (Opus), 2026-11-14. **Fait.** Fichiers : `firmware/src/hal.rs`, `firmware/src/proto.rs`, `firmware/src/lib.rs`.
- `hal` : trait `Hal` et énumérations aux signatures exactes de §3 ; constantes `MAX_FRAME=60`, `NVM_SIZE=256`,
  `RAM_SIZE=1024`. Ajouts sans effet sur le contrat : `repr(u8)`, `SensorId::ALL`, `Reg::ALL`, `Line::{ALL, mask, from_u8}`,
  `ResetCause::from_u8` (total : inconnu → `Unknown`).
- `proto` : `crc32` (table const), `encode_frame`/`decode_frame` (SYNC → type → LEN → CRC, `DecodeError::{BadSync,
  BadType, BadLen, BadCrc}` ; trame tronquée/allongée → `BadLen`), `Command` (args bruts, validation à l'exécution),
  `Ack`/`AckResult`, `Beacon` (28 o, champs `mode`/`reset_cause` bruts en `u8` pour que le sol lise une balise
  corrompue), `encode_tc/ack/beacon/events`, `ANTI_REPLAY_WINDOW=1024` (constante seule, anti-rejeu hors module).
- `lib` : `Firmware { ram }` seul, `new/boot/step/ram_mut`, `StepReport`, `Mode` (`Survie=0 < Eco=1 < Nominal=2`,
  `Ord`), `Region { name, start: usize, len: usize, kind: RegionKind }`, `RAM_MAP` provisoire (Sched 64, Mode 64,
  Fdir 128, Proto 64, Tm 192, Log 448, Unused 64 = 6,25 %), `region_of(offset)`.
  `step` bouchon : `Survie`, écrit `WdgTimeout=10`, kick clés alternées (première clé après boot = `0xA5`), n'émet rien.
- Écarts / points à arbitrer (aucun contournement de la SPEC) :
  1. Format des entrées du journal (trames EVENTS) non défini par la SPEC : charge utile opaque ici, à fixer en T-011.
  2. Codage `u8` de `Mode` (0 Survie, 1 Eco, 2 Nominal) et de `ResetCause` (0..5 dans l'ordre de §3) : choix à consigner.
  3. Le sim doit accepter `0xA5` comme première clé après un reset (état initial du watchdog matériel).
  4. `boot` ne relit pas encore la NVM (T-011) ; `WdgCtrl`/`RadioCfg` non écrits (valeurs non définies par la SPEC).
- Tests (22 dans `firmware`) : `crc32_check_vector`, `golden_frames` (3 trames de l'annexe de risks.md recopiées en
  dur ; vérifiées aussi avec `python3 zlib.crc32` avant écriture), aller-retour TC (toutes commandes), ACK, BEACON 28 o,
  EVENTS (0/1/17/48 o), rejet de chaque bit basculé (exhaustif), ordre des rejets, LEN > 48, tronquées/allongées,
  corruption aléatoire reproductible (20 000 cas) et entrée aléatoire (50 000 cas), graine loguée et rejouable via
  `AEGIS_TEST_SEED` (défaut 12565359783207763970 = 0xAE61202611140002), `size_of::<Firmware>() == RAM_SIZE`, `RAM_MAP` contiguë et
  `Unused` < 10 %, bouchon sur RAM arbitraire sans panique.
- `grep -rnE "static|Atomic|Cell<|unsafe" firmware/src` : seulement `&'static` (noms de région) et `forbid(unsafe_code)`.
Commandes lancées (résultats réels) :
- `cargo fmt --all -- --check` → OK (aucun écart).
- `cargo clippy --workspace --all-targets -- -D warnings` → 0 avertissement (un premier passage avait signalé
  `manual_unwrap_or_default` dans `proto.rs`, corrigé par un encodeur infaillible borné à la compilation).
- `cargo test --workspace` → firmware 22 passed ; autres crates : 1 + 15 + 1 passed ; 0 failed.
- `cargo build --workspace` → Finished, vert.
