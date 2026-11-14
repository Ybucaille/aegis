#!/usr/bin/env bash
# T-020 : campagne d'injection principale (SPEC §8.3). Résultats bruts dans results/{control,main,unprotected,double,main-check500}/.
# Usage : ./scripts/campaign.sh   (code retour 0 = toutes les étapes ont tourné et les contrôles sont passés)
# Refuse de démarrer si firmware/ ou sim/ ont des modifications non commitées. Seule la graine S est utilisée.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SEED_S=731051977          # graine de campagne S (decisions.md, entrée GEL DE CAMPAGNE)
SEED_V=990274663          # graine de validation V : réservée à T-025, interdite ici
RUNS_MAIN=20000
RUNS_DOUBLE=1000
RUNS_CONTROL=1000
RUNS_CHECK=500

if [ "$SEED_S" = "$SEED_V" ]; then echo "campaign.sh : graine S = graine V, refus" >&2; exit 1; fi

DIRTY="$(git status --porcelain firmware sim)"
if [ -n "$DIRTY" ]; then
  echo "campaign.sh : firmware/ ou sim/ modifiés, refus de lancer :" >&2
  echo "$DIRTY" >&2
  exit 1
fi

PROT_BIN="$ROOT/target/release/campaign"
UNPROT_BIN="$ROOT/target/unprotected/release/campaign"

echo "== build protégé =="
cargo build --release -p sim --bin campaign
echo "== build non protégé (target/unprotected) =="
cargo build --release --target-dir target/unprotected -p sim --features unprotected --bin campaign

mkdir -p "$ROOT/results/main"
git rev-parse HEAD > "$ROOT/results/main/VERSION"
echo "VERSION : $(cat "$ROOT/results/main/VERSION")"

timed() { # $1 = nom ; reste = commande
  local name="$1"; shift
  local t0 t1
  t0=$(date +%s.%N)
  "$@"
  t1=$(date +%s.%N)
  awk -v a="$t0" -v b="$t1" -v n="$name" 'BEGIN { printf "%s,%.1f\n", n, b - a }' >> "$ROOT/results/campaign_durations.csv"
}

echo "name,seconds" > "$ROOT/results/campaign_durations.csv"

echo "== (1) témoin sans panne, $RUNS_CONTROL runs =="
timed control "$PROT_BIN" --seed "$SEED_S" --runs "$RUNS_CONTROL" --class none --out "$ROOT/results/control"
python3 -I - "$ROOT/results/control/runs.csv" "$RUNS_CONTROL" <<'PY'
import csv, sys
path, n = sys.argv[1], int(sys.argv[2])
rows = list(csv.DictReader(open(path, newline="")))
ok = sum(1 for r in rows if r["verdict"] == "OK_NOMINAL")
print(f"témoin : {ok}/{len(rows)} OK_NOMINAL (attendu {n}/{n})")
if len(rows) != n or ok != n:
    sys.exit(3)
PY

echo "== (2) campagne principale protégée, $RUNS_MAIN runs mixtes =="
timed main "$PROT_BIN" --seed "$SEED_S" --runs "$RUNS_MAIN" --out "$ROOT/results/main"

echo "== (3) mêmes graines, non protégé, $RUNS_MAIN runs =="
timed unprotected "$UNPROT_BIN" --unprotected --seed "$SEED_S" --runs "$RUNS_MAIN" --out "$ROOT/results/unprotected"

echo "== (4) double panne D1, $RUNS_DOUBLE runs =="
timed double "$PROT_BIN" --seed "$SEED_S" --runs "$RUNS_DOUBLE" --class double --out "$ROOT/results/double"

echo "== (5) contrôle de reproductibilité : $RUNS_CHECK premiers runs relancés =="
timed main-check500 "$PROT_BIN" --seed "$SEED_S" --runs "$RUNS_CHECK" --out "$ROOT/results/main-check500"
head -n $((RUNS_CHECK + 1)) "$ROOT/results/main/runs.csv" > "$ROOT/results/main-check500/head_main.csv"
cmp "$ROOT/results/main-check500/head_main.csv" <(head -n $((RUNS_CHECK + 1)) "$ROOT/results/main-check500/runs.csv")
echo "CMP OK : $RUNS_CHECK premières lignes identiques (+ en-tête)"

echo "== contrôle effectifs P1 (results/main/summary.json) =="
python3 -I - "$ROOT/results/main/summary.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
pc = d["par_classe"]
p1 = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11"]
bad = []
for c in p1:
    eff = pc[c]["n"] if c in pc and "n" in pc[c] else None
    print(f"{c} n={eff}")
    if eff is None or eff < 1000:
        bad.append(c)
if bad:
    print("effectif P1 insuffisant :", bad)
    sys.exit(4)
PY

echo "CAMPAIGN DONE"
