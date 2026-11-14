# T-012 Firmware : FDIR, battement, surveillance des registres, tempête de resets
Agent: systems   Modèle: Opus   Dépend de: T-011
Entrée: `docs/SPEC.md` §4.1 (invariants), §4.3 (« qui surveille le surveillant »), §4.4, §4.2 (verrous), annexe A
Sortie attendue: `firmware/src/tasks/fdir.rs` (latch-up : 3 échantillons → coupure 5 s → réarmement ; 3 récidives
en 600 s → ligne isolée, Radio jamais isolée ; capteur figé 120 s / hors plage / `Err` → `sensor_fault_mask` ;
tempête de resets : >= 3 boots à uptime < 300 s → verrou `Survie` jusqu'à 5700 s stables ou `ClearFdir` ;
incohérence TMR ou index hors bornes → `sys_reset`), `tasks/health.rs` complété (réécriture de `WdgTimeout`,
`WdgCtrl`, `RadioCfg` ; relecture de `LoadSwitches` contre l'attendu + compteur), `sched.rs` (rechargement du
watchdog conditionné au battement FDIR), état FDIR en TMR, seuils en `const`. Feature `unprotected` étendue :
FDIR et rafraîchissement des registres neutralisés. Rien d'autre : périmètre gelé à la fin de cette tâche.
Séquentiel dans `firmware/src` (dernière tâche firmware du jour 2). Parallèle avec T-014, T-015, T-016.
Critère de fin (vérifiable): `cargo test -p firmware` → vert, avec au minimum : surintensité scénarisée → ligne
coupée en <= 5 cycles (bien avant `T_SEL` = 30 s) ; 3 récidives → Payload isolée et mode plafonné à `Eco` ;
Radio jamais isolée ; capteur figé + redondant qui varie → invalidé à 120 s, tension validée = capteur sain ;
les deux capteurs de tension invalides → `Survie` ; 3 boots courts → verrou, levé par `ClearFdir` ; battement
FDIR gelé (cellule forcée) → plus de `wdg_kick` ; registre altéré → réécrit au cycle suivant ; test de masse :
tout bit des régions `Fdir` et `Sched` basculé → sur 300 cycles, soit comportement identique au témoin, soit
reset demandé ou watchdog non rechargé (jamais d'état figé silencieux). `--features unprotected` compile et passe.
`./scripts/ci.sh` → `CI OK` ; `cargo test -p sim --test gate_day1` toujours vert.
Ajout PRE-MORTEM (R14, ambiguïtés capteurs ; R4 ; R8) : choix fixés, à consigner dans `docs/decisions.md` :
désaccord A/B → bit dans `sensor_fault_mask` (sinon `SILENT_LIE` par construction) ; « le redondant a varié » =
étendue max - min sur la fenêtre de 120 s ; températures en désaccord → la plus FROIDE pour le thermostat + bit.
`cfg(feature = "unprotected")` en un point unique (entrée de `fdir`, rafraîchissement de `health`).
Fichiers si lancée en parallèle de T-011 : `tasks/fdir.rs`, `tasks/health.rs`, `sched.rs`, `mode.rs` seulement.
Ordre de sacrifice si retard : détection de capteur figé (garder hors plage / `Err`) ; jamais latch-up, tempête
de resets, battement. Tout ce qui est coupé est écrit dans le compte rendu et ira dans le rapport.
## Compte rendu (rempli par l'agent)
**Fait** (fichiers : `tasks/fdir.rs`, `tasks/health.rs`, `tasks/thermal.rs`, `sched.rs`, `mode.rs` ; ram.rs non modifié,
aucune cellule manquante). Rien coupé de l'ordre de sacrifice (figeage, latch-up, tempête, battement : tous faits).
- **Interfaces exposées** : `pub fn fdir::clear_fdir(ram: &mut Ram)` ; `pub fn fdir::on_boot(ram: &mut Ram)` ;
  `pub fn fdir::line_cut(ram: &mut Ram, l: Line) -> bool` ; `pub fn sched::boot_erase<H: Hal>(ram, hal)` et
  `pub fn sched::boot_finish<H: Hal>(ram, hal)` (`sched::boot` = les deux) ; `pub fn mode::start_power_cycle<H: Hal>(ram,
  hal, l: Line)`, `pub fn mode::tick_power_cycle(ram)` (appelée dans `begin_cycle`), `pub const mode::PWRCYC_S: u8 = 5`.
  T-011 les utilise déjà (comms_rx : PowerCycle, ClearFdir ; nvm persiste `SHORT_BOOTS` et `LINE_ISOLATED`).
- **Latch-up** : 3 échantillons > seuil -> coupure 5 cycles (`LU_OFF_LEFT`, lue via `mode::commanded_lines`) -> réarmement ;
  3 détections dans une fenêtre de 600 s ouverte à la 1re -> isolement (Payload/Heater ; Radio : coupure seulement).
  Seuils = P_nominal / 6000 mV + 150 mA : Payload 350, Radio 184 (récepteur seul : le sim ne met pas l'émission dans
  `ILineRadio`), Heater 400 mA. `FDIR_FLAG_CRITICAL` = latch-up Radio récurrent (bloque Survie -> Eco).
- **Figeage** : `cnt` échantillons bruts identiques, invalidation quand `cnt > 120` et étendue max-min du redondant
  >= 30 mV / 1 °C ; jamais les deux capteurs d'une paire ; invalides publiés dans `sensor_fault_mask` (via power).
  Err / hors plage : immédiat chaque cycle par `power` (non verrouillé). `ClearFdir` lève aussi les invalidations.
- **Tempête** : `on_boot` (après la NVM) : `SHORT_BOOTS += 1`, verrou si >= 3 (ce boot compris) ; remis à 0 à 300 s
  d'uptime SAUF sous verrou (un reset sous verrou le ré-enclenche) ; verrou levé à 5700 s ou `ClearFdir`. Persistance
  par `nvm::request_save`.
- **Incohérence** : `TMR_ALARM` ou état FDIR hors bornes -> état assaini + `sys_reset`. Drapeaux publiés bits 3/4/5 :
  ligne isolée, coupure latch-up en cours, capteur invalidé (recalculés à chaque cycle).
- **Réchauffeur sans température valide** (révision du choix T-005) : boucle ouverte sur l'uptime selon `ISolar` :
  éclipse 50 % (30/60 s), soleil 0 %, `ISolar` illisible 33 %. `TempObc` volontairement PAS utilisée (même nœud
  thermique que la batterie dans le sim = biais favorable).
- **unprotected** : `const PROTECTED` unique dans fdir.rs (run, on_boot, line_cut) et health.rs (aucun rafraîchissement).
  `LoadSwitches` reste écrit par power/thermal (commande normale) dans les deux variantes : à citer au rapport.
- **Tests ajoutés** (fdir.rs, sched.rs) : coupure au cycle 3, 5 cycles off, réarmée ; 3 récidives -> Payload isolée,
  plafond Eco 1000 cycles, ClearFdir -> Nominal ; récidives hors fenêtre -> pas d'isolement ; Radio 10 latch-up jamais
  isolée ; capteur figé invalidé exactement à 120 s, V = capteur sain, puis sain en Err -> Survie ; variation 29 mV ->
  rien ; jamais les deux ; 3 boots courts -> verrou tenu 1000 cycles, ré-enclenché après reset, levé par ClearFdir ;
  levée à 5700 s ; battement forcé -> <= 1 kick sur 30 cycles ; tout bit de tout registre -> réécrit au cycle suivant ;
  TMR sans majorité / hors bornes -> reset ; boucle ouverte du réchauffeur ; témoin unprotected (pas de coupure,
  pas de rafraîchissement). **Masse** : 2112 bits Fdir+Sched x 300 cycles avec scénario FDIR actif (latch-up Payload,
  capteur figé, latch-up Radio) -> 2112 identiques au témoin, 0 divergence silencieuse.
- Tests sched.rs adaptés : `boot_erases_ram…` (SHORT_BOOTS=1, NVM_PENDING=1 après boot), `random_ram_never_panics`
  (capteurs remis sains 10 cycles avant de vérifier la Radio : une surintensité aléatoire persistante la fait couper,
  à juste titre), `set_mode_sets_ceiling` (Reboot -> Ok + reset, T-011), `health_rewrites…` réservé au protégé.
**Écarts / à arbitrer** :
1. `cargo test --workspace` : 1 échec, `sim/src/world.rs::nvm_kept_ram_cleared_on_reset` (fichier builder). Le test écrit
   en NVM à l'offset 7, dans la copie 0 du bloc T-011 ; le firmware écrit désormais la NVM à chaque boot (on_boot ->
   request_save, et boot_load le fera aussi). Correction : écrire à un offset >= 0xB0 (hors des 3 copies). Pas un défaut firmware.
2. **Câblage `nvm::boot_load` OBLIGATOIRE** entre `boot_erase` et `boot_finish` dans `sched::boot` : tant qu'il manque,
   la sauvegarde demandée au boot écrit en NVM les valeurs par défaut de la RAM (last_tc_seq=0, boot_count=0) ->
   anti-rejeu persistant perdu. (Je ne l'ai pas fait, consigne du manager.)
**Commandes** : `cargo fmt --all -- --check` -> ok ; `cargo clippy -p firmware --all-targets [--features unprotected]
-- -D warnings` -> propre ; `cargo test -p firmware` -> 124 + 8 ok ; `cargo test -p firmware --features unprotected` ->
102 + 8 ok ; `cargo test --release -p sim --test gate_day1` -> ok ; `./scripts/ci.sh` -> **CI FAIL** à l'étape 3 (échec
sim ci-dessus uniquement) ; étapes 1, 2, 4–7 relancées hors étape 3 -> ok ; `cargo test --workspace --no-fail-fast` ->
seul échec = celui du point 1. Aucun commit.
