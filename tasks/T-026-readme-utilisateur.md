# T-026 README : lancer, rejouer, lire les résultats
Agent: tester   Modèle: Haiku   Dépend de: T-019
Entrée: `docs/SPEC.md` §1, §2, §8.3 (CLI), §9 ; `scripts/` ; aide `--help` de `campaign` et `dashboard`
Sortie attendue: `README.md` à la racine, 80 lignes max : ce qu'est Aegis (une SIMULATION, aucune certification),
prérequis (Rust 1.99, hors ligne), commandes copiables : tests, vol nominal, campagne, rejeu d'une graine,
témoin sans protections, dashboard ; où sont les résultats et comment lire `runs.csv` / `summary.json` ;
renvoi vers `docs/REPORT.md` pour les chiffres (AUCUN chiffre de survie dans le README). Chaque commande citée
doit avoir été lancée.
Lot J3-A, parallèle avec T-021, T-023, T-024 : n'écrit que `README.md`.
Critère de fin (vérifiable): chaque bloc de commande du README lancé tel quel → code 0 (liste commande/résultat
dans le compte rendu) ; `wc -l README.md` → <= 80 ; `grep -ci "certif" README.md` → >= 1.
## Compte rendu (rempli par l'agent)
