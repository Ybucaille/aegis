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

**Fait** : `scripts/ci.sh` (exécutable, `set -euo pipefail`, s'arrête au premier échec). Étapes : 1 `cargo fmt --all -- --check`, 2 `cargo clippy --workspace --all-targets -- -D warnings`, 3 `cargo test --workspace`, 4 `#![no_std]` et `#![forbid(unsafe_code)]` dans `firmware/src/lib.rs`, 5 `Cargo.lock` = exactement dashboard, firmware, ground, sim (sinon liste affichée), 6 `extern crate alloc|std` et `use std|alloc` hors `#[cfg(test)]` dans `firmware/src`, 7 R5 : `static`, `Atomic*`, `*Cell` (y compris `RefCell`, `OnceCell`), `OnceLock`, `LazyLock`, `thread_local` hors commentaires `//` et `#[cfg(test)]`. Les `const` ne sont pas concernés. `'static` est exclu.
**Écarts** : (a) ajouts au-delà de la spec : `use alloc`, et les types Cell/OnceLock/LazyLock/thread_local pour R5 ; (b) pas de liste blanche « const justifiée » : une `static` justifiée ferait échouer le script, à trancher par le manager ; (c) analyse par awk (pas un parseur Rust) : `/* */` et chaînes contenant `//` ou `{` non gérés ; (d) le test `use std` demandé a été fait sur une copie dans le scratchpad, pas dans `firmware/src/lib.rs` (règle « n'écris que dans `scripts/` », fichier en cours d'écriture par un autre agent). Sur cette copie, `use std::vec::Vec;` hors test est rejeté par le compilateur à l'étape 2 (clippy), donc le contrôle 6 n'est pas atteint pour ce cas ; le contrôle 6 attrape `extern crate alloc;`, qui compile.
**Commande lancée (dépôt réel, 2026-11-14)** : `./scripts/ci.sh` → code 0, dernière ligne `CI OK` (fmt, clippy, tous les tests verts (`test result: ok`, aucun échec), contrats 4 à 7 ok).
**Négatifs sur copie** (`scratchpad/ci-neg`, dépôt réel non modifié) : `extern crate alloc;` hors test → code 1, « alloc/std hors #[cfg(test)] » ; `static COMPTEUR` → code 1, R5 ; `AtomicU32` → code 1, R5 ; `core::cell::Cell` → code 1, R5 ; `#![no_std]` retiré → code 1, étape 4 ; dépendance path ajoutée à `firmware` → code 1, étape 5 (liste des paquets) ; `use std` dans `#[cfg(test)]` avec `#[cfg(test)] extern crate std;` → code 0 ; `&'static str` et `static` dans un commentaire → code 0. Restauré : code 0.
