# T-030 (FACULTATIVE, non bloquante) Balayage exhaustif F1 : chaque bit de la RAM, protégé et non protégé
Agent: builder   Modèle: Sonnet   Dépend de: T-020 (après la campagne principale, jamais avant)
Entrée: `docs/decisions.md` (entrées T-019) ; API publique de `sim` (`fault::sim_config`, `World`, `FaultEvent`)
Sortie attendue: `sim/examples/ram_sweep.rs` (fichier nouveau, rien d'autre) et `results/sweep/{protected,unprotected}/sweep_<graine>.csv`.
Mesure complémentaire décidée APRÈS la répétition (à dire dans le rapport), jamais mélangée aux taux de campagne.
Pour chaque graine de monde 5501, 5502, 5503 (fixées en T-019) : un run sans panne (référence), puis un run par
bit de la RAM (8192) avec `Fault::RamSeu { byte, bit }` à `t_inj = 3000`. Colonnes :
`byte,bit,region,verdict,resets,differs` où `differs` = 1 si (verdict, resets par cause, boot_count, SoC min,
T min, trames TX/RX, écritures NVM, TC exécutées, dernière balise, balises reçues) diffère de la référence.
Mêmes oracle, seuils et modèle de panne que la campagne (c'est F1 à position imposée au lieu de tirée).
Première tâche sacrifiée si le jour 3 est en retard ; T-027 ne l'attend pas au-delà de 70 % du jour 3.
Critère de fin (vérifiable): 6 fichiers de 8193 lignes ; deux lancements donnent des fichiers identiques ;
aucun chiffre attendu n'est imposé (mesure brute).
## Compte rendu (rempli par l'agent)
Fait : `sim/examples/ram_sweep.rs` (nouveau, seul fichier de code) ; `results/sweep/{protected,unprotected}/sweep_{5501,5502,5503}.csv`. Aucun autre fichier de `sim/` ni `firmware/` touché.
Méthode : par graine, 1 run de référence sans panne puis 8192 runs `Fault::RamSeu{byte,bit}` à t_inj=3000 (`SimConfig::default()` + graine, durée standard), `World::try_new`/`step`/`finish`, protections = `PROTECTED_BUILD`. Parallélisme : threads std (16), compteur atomique, lignes réordonnées par indice de bit (sortie déterministe). `differs` = signature (verdict, resets par cause, boot_count, SoC min, T min en bits, trames TX, RX, écritures NVM, nombre de TC exécutées, dernière balise complète, nombre de balises reçues) différente de la référence. `verdict` = nom de l'oracle, `resets` = 6 compteurs par cause séparés par `/`. Panique/erreur du simulateur -> verdict `HARNESS` (aucun observé). Écart : « TC exécutées » comparé par leur nombre, pas par la liste (t, seq, opcode).
Commandes : `cargo run --release -p sim --example ram_sweep -- --out results/sweep` ; `cargo run --release -p sim --example ram_sweep --features unprotected --target-dir target/unprotected -- --out results/sweep`. Machine chargée par la campagne en parallèle : ~3 min par graine et variante.
Résultats : 6 fichiers de 8193 lignes. Déterminisme : second lancement des deux variantes dans un autre répertoire, `cmp` identique pour les 6 fichiers.
Protégé : differs=1 = 7240 (5501), 7240 (5502), 7240 (5503) ; verdict non OK = 0, 0, 0 ; OK_DEGRADED 0.
Non protégé : differs=1 = 217, 217, 225 ; verdict non OK = 1, 1, 1 ; OK_DEGRADED 3, 3, 3.
Contrôles : `cargo clippy -p sim --examples -- -D warnings` propre ; `cargo fmt --check` propre. `ci.sh` non lancé (consigne). Rien commité.
