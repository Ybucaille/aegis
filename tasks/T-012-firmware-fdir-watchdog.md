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
