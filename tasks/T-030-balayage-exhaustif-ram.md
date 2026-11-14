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
