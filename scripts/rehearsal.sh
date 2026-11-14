#!/usr/bin/env bash
# T-018 : répétition générale de la campagne (SPEC §8.3). Résultats bruts dans results/rehearsal/.
# Usage : ./scripts/rehearsal.sh   (code retour 0 = toutes les étapes ont tourné ; ne juge pas les taux)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
OUT="$ROOT/results/rehearsal"
SEED_ALL=42
mkdir -p "$OUT"

PROT_BIN="$ROOT/target/release/campaign"
UNPROT_BIN="$ROOT/target/unprotected/release/campaign"

echo "== build protégé =="
cargo build --release -p sim --bin campaign
echo "== build non protégé (target/unprotected) =="
cargo build --release --target-dir target/unprotected -p sim --features unprotected --bin campaign

timed() { # $1 = nom ; reste = commande
  local name="$1"; shift
  local t0 t1
  t0=$(date +%s.%N)
  "$@"
  t1=$(date +%s.%N)
  awk -v a="$t0" -v b="$t1" -v n="$name" 'BEGIN { printf "%s,%.1f\n", n, b - a }' >> "$OUT/durations.csv"
}

echo "name,seconds" > "$OUT/durations.csv"

echo "== (1) témoin sans panne, 1000 runs =="
timed nominal_1000 "$PROT_BIN" --seed "$SEED_ALL" --runs 1000 --class none --out "$OUT/1_nominal"

echo "== (2) 2000 runs toutes classes, protégé =="
timed protected_2000 "$PROT_BIN" --seed "$SEED_ALL" --runs 2000 --out "$OUT/2_protected"

echo "== (3) mêmes 2000 graines, non protégé =="
timed unprotected_2000 "$UNPROT_BIN" --unprotected --seed "$SEED_ALL" --runs 2000 --out "$OUT/3_unprotected"

echo "== (4) double panne D1, 200 runs =="
timed double_200 "$PROT_BIN" --seed "$SEED_ALL" --runs 200 --class double --out "$OUT/4_double"

echo "REHEARSAL DONE"
