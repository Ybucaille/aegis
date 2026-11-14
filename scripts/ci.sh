#!/usr/bin/env bash
# CI locale Aegis (pas de CI distante). Échoue au premier contrôle rouge.
# Usage : ./scripts/ci.sh   (depuis n'importe où ; code retour 0 = « CI OK »)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail() { echo "CI FAIL : $*" >&2; exit 1; }
step() { echo; echo "== $* =="; }

# Sortie de firmware/src sans commentaires `//` et sans blocs #[cfg(test)].
# Format de sortie : "<numéro de ligne>:<contenu>". Approximation (pas un parseur Rust) :
# les commentaires `/* */` et les chaînes contenant `//` ou des accolades ne sont pas gérés.
firmware_code_lines() {
  find firmware/src -name '*.rs' -print0 | sort -z | while IFS= read -r -d '' f; do
    awk -v file="$f" '
      BEGIN { skip = 0; depth = 0; pending = 0 }
      {
        line = $0
        sub(/\/\/.*$/, "", line)
        if (skip) {
          n = gsub(/\{/, "{", line); m = gsub(/\}/, "}", line)
          depth += n - m
          if (depth <= 0) skip = 0
          next
        }
        if (pending) {
          if (line ~ /^[ \t]*#\[/) next
          pending = 0
          n = gsub(/\{/, "{", line); m = gsub(/\}/, "}", line)
          if (n == 0 && line ~ /;[ \t]*$/) next
          depth = n - m
          if (depth > 0) skip = 1
          next
        }
        if (line ~ /^[ \t]*#\[cfg\(test\)\]/) { pending = 1; next }
        print FILENAME ":" NR ":" line
      }' "$f"
  done
}

step "1/7 cargo fmt --check"
cargo fmt --all -- --check || fail "cargo fmt --check"

step "2/7 cargo clippy -D warnings"
cargo clippy --workspace --all-targets -- -D warnings || fail "cargo clippy"

step "3/7 cargo test"
cargo test --workspace || fail "cargo test"

step "4/7 contrat : attributs de firmware/src/lib.rs"
grep -qE '^#!\[no_std\]' firmware/src/lib.rs || fail "firmware/src/lib.rs ne contient pas #![no_std]"
grep -qE '^#!\[forbid\(unsafe_code\)\]' firmware/src/lib.rs || fail "firmware/src/lib.rs ne contient pas #![forbid(unsafe_code)]"
echo "ok"

step "5/7 contrat : Cargo.lock = 4 paquets du workspace"
[ -f Cargo.lock ] || fail "Cargo.lock absent"
expected="$(printf '%s\n' dashboard firmware ground sim)"
actual="$(awk -F'"' '/^name = /{print $2}' Cargo.lock | sort)"
if [ "$actual" != "$expected" ]; then
  echo "paquets dans Cargo.lock :" >&2
  echo "$actual" | sed 's/^/  /' >&2
  fail "Cargo.lock doit contenir exactement dashboard, firmware, ground, sim"
fi
echo "ok (4 paquets)"

step "6/7 contrat : pas d'alloc ni de std hors #[cfg(test)] dans firmware/src"
bad="$(firmware_code_lines | grep -E '(^|:)[0-9]+:.*(extern[[:space:]]+crate[[:space:]]+(alloc|std)\b|(^|[^A-Za-z0-9_])use[[:space:]]+(std|alloc)([^A-Za-z0-9_]|$))' || true)"
if [ -n "$bad" ]; then
  echo "$bad" >&2
  fail "alloc/std hors #[cfg(test)] dans firmware/src"
fi
echo "ok"

step "7/7 R5 : pas d'état caché dans firmware/src (static / Atomic / Cell)"
bad="$(firmware_code_lines | grep -E "[^A-Za-z0-9_']static([^A-Za-z0-9_]|$)|(^|[^A-Za-z0-9_])Atomic[A-Za-z0-9_]*|(^|[^A-Za-z0-9_])[A-Za-z]*Cell([^A-Za-z0-9_]|$)|OnceLock|LazyLock|thread_local" || true)"
if [ -n "$bad" ]; then
  echo "$bad" >&2
  fail "état caché (R5) dans firmware/src : voir lignes ci-dessus"
fi
echo "ok"

echo
echo "CI OK"
