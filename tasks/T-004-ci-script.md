# T-004 Script de CI locale
Agent: tester   Modèle: Haiku   Dépend de: T-001
Entrée: `CLAUDE.md` (garde-fous Rust), `docs/SPEC.md` §2
Sortie attendue: `scripts/ci.sh` (exécutable, `set -euo pipefail`) qui enchaîne : `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, puis trois contrôles de
contrat : (1) `firmware/src/lib.rs` contient `#![no_std]` et `#![forbid(unsafe_code)]` ; (2) `Cargo.lock` ne
contient que les 4 paquets du workspace, sinon échec avec la liste ; (3) aucun `extern crate alloc` ni `use std`
hors `#[cfg(test)]` dans `firmware/src`. Pas de CI distante (pas de push).
Lot J1-A, parallèle avec T-002 et T-003 : n'écrit que dans `scripts/`.
Critère de fin (vérifiable): `./scripts/ci.sh` → code 0 et dernière ligne `CI OK` ; en ajoutant temporairement
`use std::vec::Vec;` dans `firmware/src/lib.rs` le script échoue (essai fait puis annulé, à dire dans le compte rendu).
## Compte rendu (rempli par l'agent)
