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
Agent systems (Opus), 2026-11-14. Pas de commit (consigne).

**Fait** — fichiers : `firmware/src/{ram,sched,mode,testhal}.rs`, `firmware/src/tasks/{mod,health,power,fdir,
thermal,comms_rx,telemetry,scrub,payload}.rs`, `firmware/src/lib.rs` modifié. `hal.rs` et `proto.rs` intacts.
- `ram.rs` écrit et testé en premier (6 tests verts avant toute tâche) : `Slot<T: Scalar>` (u8/u16/i16/u32/bool,
  gros-boutiste), décalage `const` vérifié à la compilation, cellules chaînées (`Slot::after`) donc sans
  chevauchement, assertions de bornes par région, table `SLOTS` testée (pas de chevauchement, bonne région).
  `get` prend déjà `&mut Ram` pour que T-010 ajoute le vote TMR sans toucher les tâches. Nom `Slot` et non
  `...Cell` : le contrôle 7/7 de `ci.sh` interdit tout identifiant finissant par `Cell`.
- `RAM_MAP` : bornes de T-002 inchangées, déplacé dans `ram.rs` et réexporté (`firmware::RAM_MAP`, `Region`,
  `RegionKind`, `region_of` : API inchangée pour sim/ground). `Unused` = 64 o (6,25 %).
  **Couverture réelle : 63 octets de cellules sur 1024 (6 %)** ; le reste des régions est réservé (TMR T-010,
  journal T-011, FDIR T-012) et un SEU y est aujourd'hui sans effet (R6 : à ne pas lire comme de la robustesse).
- Valeurs fixées (à consigner dans decisions.md) : `WDG_CTRL_VALUE = 0x0001` (bit 0 = watchdog activé),
  `RADIO_CFG_VALUE = 0x0003` (bit 0 = RX actif, bit 1 = TX autorisé), `WDG_TIMEOUT_S = 10` ; constantes `pub`
  dans `lib.rs`. `health` relit les 3 registres + `LoadSwitches`, compte les écarts (`CNT_REG_FIX`), réécrit.
- Watchdog : kick en fin de cycle ssi 4 drapeaux critiques posés ET battement FDIR = vu + 1 ; clé suivante
  inchangée si pas de kick ; clé corrompue → 0xA5. 0xA5 en première clé après boot.
- Modes : table complète §4.2 dans `mode.rs` (plafond en RAM, lecture totale : mode invalide → Survie, plafond
  invalide → Nominal). Précision : compteurs de montée remis à 0 à chaque changement de mode et tant qu'une
  descente forcée tient (60 s mesurées dans le mode de départ). `T_batt` hors [-5, +45] °C → Survie immédiat.
- Capteurs : valide = `Ok` + plage plausible (V [5000, 9000] mV, T [-60, +90] °C) + non invalidé par
  `SENSOR_INVALID_MASK` (T-012). Désaccord V > 150 mV → plus basse + bit 9 ; T > 2 °C → plus froide + bit 10 ;
  capteur manquant → bit `SensorId`. Aucune T valide → réchauffeur COUPÉ (choix à revoir en T-012).
- Balise : période du mode, compteur lu `min(valeur, période)` (R13), première balise au 1er cycle, nouvel essai
  si `radio_send` échoue ; `line_state` = `LoadSwitches` relu. ACK émis une fois (pas de nouvel essai).
- TC : Ping, SetMode (BadArg si mode invalide) ; Reboot/PowerCycle/ClearFdir/DumpEvents acceptés (SEQ consommé)
  et acquittés `Refused` jusqu'à T-011. Anti-rejeu sans débordement (`seq > last && seq - last <= 1024`).

**Écarts / points pour le manager**
1. **Interface (§3.1 vs §4.1)** : `StepReport.executed_tc` ne porte qu'UNE TC alors que `comms_rx` lit jusqu'à
   2 trames/tick. Contournement sans toucher la SPEC : au plus 2 trames LUES et au plus 1 TC ACCEPTÉE par cycle
   (la lecture s'arrête après une acceptation). Si 2 TC/tick sont voulues, il faut changer `StepReport`.
2. **CI rouge à cause de `sim`** : `./scripts/ci.sh` → `CI FAIL : cargo test` sur 3 tests de `sim/src/world.rs`
   (builder, T-006) : `cut_line_reads_zero_current_and_costs_nothing`, `transmit_costs_3w_for_one_second`,
   `uplink_delivered_only_during_pass`. Ils supposent le bouchon T-002 (firmware passif). Vérifié : le même
   `sim` passe à 27/27 contre le firmware de HEAD (copie dans le scratchpad), et échoue contre le firmware réel
   qui, conformément à la SPEC, réécrit `LoadSwitches` à chaque cycle, émet une balise au 1er cycle et consomme
   la file montante. Correction à faire côté sim (tests qui pilotent le matériel sans `firmware.step`, ou qui
   tiennent compte de l'actionnement). Je n'ai pas touché `sim/`.
3. `tasks/mod.rs` créé (nécessaire au module `tasks`), non listé dans la tâche.

**Commandes lancées et résultats réels**
- `cargo test -p firmware` → `test result: ok. 63 passed; 0 failed` (debug, overflow-checks) ; `--release` → 63 ok.
  Couvre : boot → Survie ; Survie → Eco (cycle 160 avec maintien, 120 avec uptime) → Nominal (60 s plus tard) ;
  V < 6720 mV 10 s → Survie au 10e cycle ; désaccord A/B → 7400 + bit ; Ping acquitté ; rejeu, SEQ ancien, hors
  fenêtre, chaque bit basculé, chaque troncature, mauvais type → aucun ACK, `cnt_tc_rejected` +1 à chaque ;
  kicks 0xA5/0x5A alternés sur 1000 cycles ; pas de kick si un drapeau critique manque (×4) ou battement gelé /
  sauté ; `size_of::<Firmware>() == RAM_SIZE` ; 20 000 cycles nominaux avec bruit (graine 0x5EED0005) sans panic,
  toujours rechargé ; 200 RAM aléatoires + SEU/capteurs/trames aléatoires × 300 cycles (graine 0xA3615005) sans
  panic ; chaque bit de chaque cellule basculé → radio alimentée et balises continuent ; compteur de balise
  corrompu → balise quand même (R13).
- `cargo fmt -p firmware` puis `cargo clippy -p firmware --all-targets -- -D warnings` → aucun avertissement.
- `grep -rnE "static mut|Atomic|Cell<|thread_local|lazy" firmware/src` → aucune ligne (code retour 1).
- `./scripts/ci.sh` → **`CI FAIL : cargo test`** (3 tests `sim`, voir point 2) ; étapes 1-2 vertes.
- Même script dans une copie (scratchpad) avec l'étape 3 limitée à `cargo test -p firmware -p ground` → étapes
  1 à 7 vertes, **`CI OK`**.
