# T-021 Extraction et rejeu des échecs
Agent: tester   Modèle: Haiku   Dépend de: T-020
Entrée: `results/{control,main,unprotected,double}/{runs.csv,summary.json}` ; `docs/SPEC.md` §8.1
Sortie attendue: `results/main/failures.md`, purement factuel : (1) tableau classe x catégorie principale
(effectifs) ; (2) pour chaque couple (classe, catégorie) non vide : effectif, 5 graines exemples, et pour la
première la trace `campaign --replay` résumée en 10 lignes max (t_inj, ce qui a été touché, resets, dernier mode,
critère S faux) ; (3) F1/F2 : échecs par région de `RAM_MAP` ; (4) classes où protégé et non protégé ont le même
taux à 2 points près ; (5) runs `OK_DEGRADED` : motif (mode inférieur, ligne isolée/détruite, capteur invalidé),
effectifs. `scripts/failures.sh` qui régénère les tableaux (1), (3), (4), (5) depuis les CSV.
Pas d'hypothèse sur les causes dans ce fichier : c'est le travail de T-022.
Parallèle avec T-026 (fichiers disjoints). Budget : 10 % du jour 3.
Critère de fin (vérifiable): `./scripts/failures.sh results/main` → code 0 et la somme des effectifs du tableau
(1) = nombre de runs non survivants de `summary.json` ; chaque graine exemple citée, rejouée, redonne le verdict
du CSV (vérifié sur toutes, résultat écrit).
Ajout T-019 : (6) par classe, protégé ET non protégé : nombre de runs avec trace de détection (`cnt_ecc_corr + cnt_tmr_fix + cnt_latchup + cnt_tc_rejected > 0`, ou `fdir_flags`/`sensor_fault_mask` non nuls) ; (7) par classe : nombre de lignes identiques octet pour octet entre `main` et `unprotected` (hors colonnes de compteurs) ; (8) F10 : nombre de runs avec S2 vrai (2e caractère de `criteria`) parmi les échecs E2 ; (9) F7 séparé en `WdgSpurious` / `WdgRegHit`.
## Compte rendu (rempli par l'agent)
Fait : `scripts/failures.sh` (régénère (1), (3), (4), (5), (6), (7), (8), (9) depuis les CSV ; code 1 si une vérification de somme échoue) et `results/main/failures.md` (section (2) écrite à partir des rejeux). Rien commité, `ci.sh` non lancé.

Pas fait : rien d'autre dans le périmètre. Traces de (2) : une par couple (3 au total), pas toutes les graines.

Écarts et choix :
- (7) compteurs exclus : resets_*, boot_count, cnt_ecc_corr, cnt_tmr_fix, cnt_tc_rejected, cnt_latchup.
- (6) champs vides (aucune balise) comptés comme absence de trace ; colonne « sans balise » donnée à part.
- (5) motifs « ligne détruite » et « ligne isolée » absents du CSV, non comptés.
- (8) porte sur F10 seulement, comme écrit dans la tâche.

Commandes et résultats :
- `./scripts/failures.sh results/main` : code 0 ; tableau (1) somme = 1868 = non-survivants de `summary.json` (1868 / 20000).
- Rejeux `target/release/campaign --seed 731051977 --runs 20000 --replay <graine>` : 15 graines (5 par couple : F5 40, F10 400, D1 1428, tous E2), 15/15 verdict et catégories concordants, 15/15 lignes CSV identiques octet pour octet.

Résultats clés (protégé) :
- (1) E2 : F5 40, F10 400, D1 1428 ; E3 jamais principale (second dans D1).
- (3) F1/F2 : 0 échec sur 2858 runs (2575 hors `Unused`).
- (4) même taux à 2 pp : F1, F2, F6 à F13, D1 ; écart : F3 (100,00 % vs 74,81 %), F4 (100,00 % vs 1,05 %), F5 (97,20 % vs 94,19 %).
- (5) OK_DEGRADED 518 : mode < Nominal 494 ; Payload/Radio éteinte 0 ; masque capteur 0 ; aucun des trois 24.
- (6) trace protégé / non protégé : F1 1246/25, F2 1253/20, F4 1427/0, F5 715/507, F6 1429/1429, F8 1395/24, autres 0 ; D1 : 437 sans balise.
- (7) identiques : 13990/20000 (ligne entière), 17831/20000 hors compteurs.
- (8) F10 : 400 échecs E2, S2 vrai dans 400.
- (9) F7 : WdgSpurious 682 runs, 0 échec ; WdgRegHit 747 runs, 0 échec.
