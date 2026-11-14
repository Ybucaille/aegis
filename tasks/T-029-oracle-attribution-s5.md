# T-029 Correctif bloquant B2 : faux positif E7 de l'oracle (attribution d'une TC exécutée)
Agent: builder   Modèle: Sonnet   Dépend de: T-019   Parallèle avec: T-028 (fichiers disjoints)
Entrée: `docs/decisions.md` (entrées T-019), `docs/SPEC.md` §8 (S5, inchangé), `sim/src/oracle.rs`
(`on_up_delivered`, `on_tc_executed`), point d'appel dans `sim/src/world.rs`
Sortie attendue: `sim/src/oracle.rs` (+ le seul point d'appel de `world.rs` si la signature change, + tests).
Défaut établi par rejeu (`campaign --seed 42 --runs 2000 --replay 10219195993739918047`) : `TcCorrupt` bascule un
bit du SEQ (1 → 17) ; la trame corrompue est REJETÉE par le firmware (`cnt_tc_rejected = 1`, aucune exécution à
t=2974) mais reste « remise, non consommée » dans l'oracle ; quand le sol émet plus tard la vraie trame SEQ 17,
exécutée une fois et intacte, `on_tc_executed` l'attribue à la vieille trame corrompue de même SEQ → S5 faux à
tort. La définition de S5 ne change pas ; seule l'attribution est corrigée.
Règle exigée : une TC exécutée au tick t est rapprochée des trames réellement LUES par le firmware pendant ce
même pas (vérité du simulateur : ce que `Hw` a remis à `radio_rx` dans ce `step`), jamais d'une trame plus
ancienne de même SEQ. Si aucune trame lue dans ce pas ne porte ce SEQ et cet opcode → violation (inventée ou
rejeu), comme aujourd'hui. Pas de règle « préférer la trame intacte » (ce serait favorable au firmware).
Interdit : toucher aux critères S1–S6, aux seuils, à E8, à `fault.rs`, `params.rs`, `firmware/`.
Critère de fin (vérifiable): test de non-régression nommé `s5_rejected_corrupt_frame_does_not_poison_later_seq`
(vert) ; les tests S5 existants restent verts sans être affaiblis (trame corrompue réellement exécutée → E7 ;
rejeu exécuté → E7 ; TC inventée → E7) ; `./scripts/ci.sh` → CI OK ; rejeu des graines 10219195993739918047 et
16940148439563031665 (campagne 42, 2000 runs) → `OK_NOMINAL` dans les deux variantes ; après
`./scripts/rehearsal.sh`, AUCUNE autre ligne de `2_protected/runs.csv` ni `3_unprotected/runs.csv` ne change
(colonnes d'origine) par rapport à la version commitée. Toute autre ligne modifiée : ARRÊT, rapport au manager.
## Compte rendu (rempli par l'agent)
