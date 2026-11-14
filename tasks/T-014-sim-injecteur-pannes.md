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
