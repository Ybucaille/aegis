# T-024 Corrections du harnais décidées au triage (sim / ground / dashboard)
Agent: builder   Modèle: Sonnet   Dépend de: T-022
Entrée: `docs/triage.md` (section « correctifs harnais », liste fermée)
Sortie attendue: modifications dans `sim/`, `ground/` ou `dashboard/` limitées à : bogues E0, injection sans effet
ou mal consignée, erreur de métrique ou de sérialisation. INTERDIT : modifier `oracle.rs` dans un sens qui change
un verdict, les seuils de l'annexe A, la définition des classes de pannes. Si un bogue d'oracle est avéré (écart
à SPEC §8 écrit noir sur blanc), il faut une entrée `docs/decisions.md` du manager AVANT la correction, et le
rapport le mentionnera. Tâche vide si le triage ne liste rien : le noter et clore.
HEURE LIMITE : 70 % du jour 3, même règle de retrait que T-023.
Lot J3-A, parallèle avec T-023 et T-026 : n'écrit que dans `sim/`, `ground/`, `dashboard/`.
Critère de fin (vérifiable): un test par correctif ; `./scripts/ci.sh` → `CI OK` ;
`cargo test --release -p sim --test determinism` vert ; `git diff --stat` (ou liste des fichiers modifiés) dans
le compte rendu, avec mention explicite « oracle.rs : non modifié » ou la référence de la décision.
## Compte rendu (rempli par l'agent)
