# T-014 Simulateur : injecteur de pannes (F1–F13, D1)
Agent: builder   Modèle: Sonnet   Dépend de: T-013
Entrée: `docs/SPEC.md` §7 (table complète), §7.1, §3.1 (port `ram_mut`, `RAM_MAP`), annexe A (latch-up)
Sortie attendue: `sim/src/fault.rs` (`Fault`, `FaultEvent`, tirage `(classe, paramètres, t_inj)` depuis le PRNG du
run, `t_inj` uniforme dans [300, 6000] s, sérialisation texte stable des paramètres), application dans
`World::step` étape 1 et `World::inject` ; modèle de latch-up (surintensité, destruction à `T_SEL`, effacement
après 2 s ligne coupée) ; `CpuHang` ; `BattDeplete` tiré pendant une éclipse ; pannes de canal F6/F12 sur la
première trame concernée après `t_inj` ; `WdgDead` pour D1 ; F8 ciblé via `RAM_MAP`. Chaque injection consigne
ce qui a été réellement touché (région, bit, ligne) dans les métriques.
Témoin sans protections : feature cargo `unprotected` de `sim` qui active `firmware/unprotected` ;
`SimConfig.protections` doit correspondre au binaire compilé, sinon erreur explicite (pas de bascule silencieuse).
Séquentiel dans `sim/` (après T-013, avant T-015). Parallèle avec T-011/T-012 (firmware) et T-017.
Critère de fin (vérifiable): `cargo test -p sim` → vert, avec au minimum : pour chaque classe F1–F13 et D1, un
test qui injecte et vérifie l'EFFET PHYSIQUE dans le sim (bit réellement basculé, courant réellement augmenté,
SoC réellement forcé, trame réellement altérée…), indépendamment de la réaction du firmware ; latch-up non coupé
→ ligne détruite à 30 s ; F1 sur 10 000 tirages → positions couvrant toutes les régions de `RAM_MAP` ; même
graine → même panne ; chaque classe, 20 graines → aucun `HARNESS` (E0). `./scripts/ci.sh` → `CI OK`.
## Compte rendu (rempli par l'agent)
Fait : `sim/src/fault.rs` (Fault, FaultClass incl. `none`/`double`, `draw`, `sim_config`, texte stable, mutations de trames pures), `World::inject`/`try_new`/`truth()`/`injections()`/`set_firmware_enabled`, `InjectionRecord` dans `RunResult`, ajouts `Hw` (watchdog mort, défaut capteur, ligne morte, `uplink_peek`) et `Power` (coupure forcée F13). Tests : `sim/tests/faults.rs` (un test d'effet physique par classe sur banc sans firmware, latch-up détruit à 30 s, effacement 2 s vs 1 s, F1 10 000 tirages couvrant toute RAM_MAP, même graine = même run, 14 classes x 20 graines sans HARNESS) + tests de `fault.rs`.
Commandes : cargo fmt --all ; cargo clippy -p sim --all-targets -- -D warnings (vert) ; cargo test -p sim --release (54 + 18 + gate_day1 + nominal verts). `./scripts/ci.sh` : CI FAIL à l'étape 6/7, `firmware/src/protection_tests.rs:11 extern crate std` (fichier de T-010/systems, hors périmètre sim).
Écarts / à savoir : (1) feature `sim/unprotected` déclarée vide : `["firmware/unprotected"]` à câbler quand T-010 aura ajouté la feature (cargo refuse sinon) ; `World::try_new` refuse protections != binaire. (2) Choix défavorables/favorables à citer : latch-up latent tant que la ligne n'est pas alimentée (jamais alimentée => `applied_t=None`, sans effet : à compter en T-015) ; temporisation T_SEL cumulative (coupures < 2 s ne la remettent pas à zéro) ; ligne détruite = circuit ouvert ; registres basculés sur leur largeur utile seulement (LoadSwitches 3, WdgCtrl 1, RadioCfg 2, WdgTimeout 16) ; D1 = WdgDead + CpuHang 1..600 s plus tard ; F6 TcReplay rejoue une trame ancienne du sol. (3) Pannes de canal F6/F12 : `applied_t` None si aucune trame après t_inj.
Verdicts observés (graines 1..20, firmware actuel) : F1,F2,F3,F6,F7,F8,F9,F11,F12,F13 20/20 OK_NOMINAL ; F5 18 OK, 2 Fail(SilentLie/Thermal) ; F10 5 Fail(Thermal), reste OK_DEGRADED/NOMINAL ; F4 20/20 échec (VitalLoss/SilentLie/Thermal : pas encore de FDIR) ; D1 20/20 Fail(Thermal).
