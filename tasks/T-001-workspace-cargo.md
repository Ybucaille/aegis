# T-001 Création du workspace cargo (4 crates)
Agent: builder   Modèle: Sonnet   Dépend de: aucune
Entrée: `CLAUDE.md`, `docs/SPEC.md` §2
Sortie attendue: `Cargo.toml` racine (workspace `firmware`, `sim`, `ground`, `dashboard` ; `resolver = "2"` ;
`[profile.release]` avec `overflow-checks = true` et `panic = "unwind"`), `firmware/` (lib, `#![no_std]`,
`#![forbid(unsafe_code)]`), `ground/` (lib, dépend de `firmware`), `sim/` (lib + `src/bin/campaign.rs` vide qui
compile, dépend de `firmware` et `ground`), `dashboard/` (bin, dépend de `sim`, `ground`, `firmware`),
`.gitignore` (`target/`), `docs/later.md` (vide, titre seul), un test trivial par crate.
`git init` seulement si `.git` est absent ; aucun push, aucun remote. Zéro dépendance externe.
Séquentiel : rien d'autre ne démarre avant la fin de cette tâche.
Critère de fin (vérifiable): `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
→ code de sortie 0, au moins 4 tests passés ; `grep -c 'name = ' Cargo.lock` → 4 (aucun paquet externe) ;
`git -C . rev-parse --is-inside-work-tree` → `true`.
## Compte rendu (rempli par l'agent)
Fait : workspace 4 crates (firmware no_std+forbid(unsafe), ground, sim + bin campaign vide, dashboard bin), .gitignore, docs/later.md, 1 test/crate, `git init` (absent). Aucun écart, zéro dépendance externe.
Commandes : `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` -> exit 0, 4 tests passés (1 par crate, 0 échec). `grep -c 'name = ' Cargo.lock` -> 4. `git rev-parse --is-inside-work-tree` -> true.
Note : branche git initiale = défaut du système (vérifier si `main` souhaité).
