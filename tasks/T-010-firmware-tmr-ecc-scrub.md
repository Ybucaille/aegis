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
**Fait** (fichiers : `firmware/src/{tmr,ecc,ram,lib,testhal,protection_tests}.rs`, `tasks/{scrub,telemetry}.rs`,
`sched.rs` (tests seulement), `firmware/Cargo.toml` (feature `unprotected`)).
- **TMR** (`tmr.rs`) : vote **bit à bit**, réécriture à la lecture, 1 octet réparé = +1 `cnt_tmr_fix`. Copies dans
  3 blocs distincts par région (copie c à `base + c*bloc`, écart >= 1 octet garanti à la compilation). « Absence de
  majorité » = 3 copies d'un octet deux à deux différentes → `ram::TMR_ALARM` posé (réaction `sys_reset` = T-012).
  Limite connue : même bit basculé sur 2 copies → vote faux, indétectable (testé et documenté).
- **SECDED** (`ecc.rs`) : Hamming étendu **(13,8)**, **entrelacé bit à bit** par groupes de 8 octets de données dans 13
  octets (bit k du mot de la voie i = bit i de l'octet k) : tout MBU de 2 à 8 bits adjacents touche chaque mot au plus
  une fois → corrigé. Erreur double → valeur de repli de la cellule, réécrite, comptée dans `CNT_ECC_UNCORR`
  (tampon `LOG_BUF` : `None`, laissé à T-011). Choix (13,8) : test exhaustif sur tous les mots + entrelacement.
- **Scrub** : 2 curseurs TMR (lus modulo), 4 octets TMR + 2 groupes SECDED par cycle → image complète en **30 s**.
- **`ram.rs` FIGÉ** : `Slot` gagne une protection (`Plain`/`Tmr`/`Ecc`) sans changer `get`/`set` vus par les tâches.
  Toutes les cellules de T-011 (REBOOT_PENDING, PWRCYC_*, NVM_PENDING/STATUS, CNT_NVM_*, DUMP_*, LOG_HEAD/COUNT/TOTAL,
  LOG_BUF 320 o, FDIR_FLAG_NVM_LOST) et T-012 (BOOT_STABLE, STORM_STABLE_S, SHORT_BOOTS, LU_OVER/OFF_LEFT/RECUR/WIN_LEFT
  par ligne, STUCK_LAST/CNT/MIN/MAX par capteur redondant, TMR_ALARM) sont réservées, marquées « RÉSERVÉ T-0xx ».
  Non protégées : 8 o de mesures du cycle (VBATT_MV…SENSOR_FAULT_MASK), réécrites par `power` avant toute lecture.
- **RAM_MAP modifiée** (tailles de régions, noms/types inchangés) : sched 0..60, mode 60..104, fdir 104..308,
  proto 308..356, tm 356..395, log 395..928, unused 928..1024 (96 o, 9,4 %). Le sim doit lire `RAM_MAP` (pas de
  bornes en dur).
- Occupation (taille région, part RAM, octets portés par des cellules) : sched 60 o 5 % TMR, 90 % ; mode 44 o 4 %
  (36 TMR + 8 brut), 93 % ; fdir 204 o 19 % TMR, 97 % ; proto 48 o 4 % TMR, 87 % ; tm 39 o 3 % SECDED, 79 % ;
  log 533 o 52 % SECDED, 99 % ; unused 96 o 9 %. Dilution R6 : la moitié de la RAM est du journal.
- `cfg(feature = "unprotected")` : un seul point par fichier (`const PROTECTED` dans `tmr.rs` et `ecc.rs`), aucun
  dans les tâches. Balise : `cnt_tmr_fix` et `cnt_ecc_corr` réels.
- Tests ajoutés : SECDED exhaustif (256 mots × 13 simples × 78 doubles, distance 4) ; TMR 1 copie / 2 copies ; test de
  masse : **chaque bit des 928 o protégés** (7424 basculements) + 120 cycles → mêmes rapports, rechargements, trames
  (balises hors compteurs de réparation), registres et image finale que le témoin ; MBU 2–4 bits (LSB et MSB d'abord)
  sur toutes les régions → valeurs justes, aucune alarme ; scrub seul → tout bit corrigé en <= 60 cycles.
**Écarts / à arbitrer** : (1) choix de vote et SECDED à recopier dans `docs/decisions.md` par le manager (hors de mon
périmètre d'écriture). (2) `cargo test -p firmware` dure ~45–50 s en debug (test de masse). (3) Le test
`sched::every_slot_bit_flip…` a été réécrit (instantané cloné, bits de données en SECDED) ; `boot_erases_ram…` compare
désormais à un boot depuis une RAM à zéro.
**Commandes** : `cargo test -p firmware` → 84 + 8 ok ; `cargo test -p firmware --features unprotected` → 75 + 8 ok ;
`cargo clippy -p firmware --all-targets [--features unprotected] -- -D warnings` → propre ; `cargo fmt` fait ;
`./scripts/ci.sh` → **CI OK** (workspace compris, après correction d'un `extern crate std` non annoté signalé par
l'étape 6/7 au premier passage) ; `cargo test -p sim --test gate_day1` → ok (1 passed) ; `--features unprotected`
relancé après la correction → vert. Aucun commit.
**Parallélisation T-011/T-012** : possible. `ram.rs` est figé et contient toutes leurs cellules ; fichiers disjoints
(T-011 : nvm.rs, log.rs, comms_rx.rs, telemetry.rs ; T-012 : fdir.rs, health.rs, sched.rs, mode.rs). Points de contact
à fixer par le manager : `lib.rs` (déclaration de `mod nvm; mod log;` par T-011) et `sched::boot` (relecture NVM par
T-011 alors que `sched.rs` revient à T-012) → T-011 expose `nvm::boot_load(ram, hal)`, T-012 l'appelle dans `boot`.
