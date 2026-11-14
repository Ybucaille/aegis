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
