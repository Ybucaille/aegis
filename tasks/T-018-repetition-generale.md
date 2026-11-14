# T-018 Répétition générale : tests par classe de panne, déterminisme, témoins
Agent: tester   Modèle: Haiku   Dépend de: T-012, T-015
Entrée: `docs/SPEC.md` §7, §7.1, §8.3 ; `sim/src/bin/campaign.rs` (aide `--help`)
Sortie attendue: `sim/tests/determinism.rs` (deux campagnes de 200 runs, même graine → `runs.csv` identique octet
pour octet ; 1 fil contre 4 fils → identique), `scripts/rehearsal.sh` qui lance et range dans `results/rehearsal/` :
(1) témoin sans panne 1000 runs ; (2) 2000 runs toutes classes ; (3) les mêmes 2000 graines en `--unprotected` ;
(4) 200 runs `--class double`. `results/rehearsal/NOTES.md` : tableau brut par classe (effectif, survie, nominal,
catégories), nombre de E0, durée mesurée, extrapolation de durée pour 20 000 + 20 000 + 1000 runs.
Chiffres rapportés tels quels, sans commentaire flatteur. Aucune correction de code.
Séquentiel : tourne quand `firmware/` et `sim/` sont stables (aucun autre agent n'y écrit).
Critère de fin (vérifiable): `cargo test --release -p sim --test determinism` → vert ; `./scripts/rehearsal.sh` →
code 0 ; contrôles de la GATE DE FIN DE JOUR 2 lisibles dans `NOTES.md` : (a) témoin sans panne = 1000/1000
`OK_NOMINAL` ; (b) E0 `HARNESS` = 0 ; (c) chaque classe P1 a >= 1 run ; (d) taux de survie sans protections
publié à côté du taux protégé pour chaque classe. Si (a) ou (b) est faux : le dire en première ligne.
Ajout PRE-MORTEM (R6, R12, R2) : `NOTES.md` donne aussi : (e) F1/F2 par type de région, protégé et non protégé ;
(f) liste des classes à 100 % en protégé avec leur taux non protégé à côté (une classe à 100 % des deux côtés
= injection probablement sans effet, à signaler) ; (g) F10 : répartition des catégories d'échec ; (h) nombre
moyen de resets par classe. Le binaire non protégé vient du répertoire cible séparé (voir T-015).
## Compte rendu (rempli par l'agent)
