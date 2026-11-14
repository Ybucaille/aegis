#!/usr/bin/env bash
# Régénère les tableaux factuels de results/main/failures.md à partir des CSV de la campagne.
# Usage : scripts/failures.sh [results/main]
# Lit <dir>/runs.csv, <dir>/summary.json et, pour les comparaisons protégé/non protégé,
# <parent>/unprotected/runs.csv. Écrit le markdown sur stdout. Code 1 si une vérification échoue.
# Aucune hypothèse de cause : ce script ne fait que compter.
set -euo pipefail

DIR="${1:-results/main}"
if [[ ! -f "$DIR/runs.csv" || ! -f "$DIR/summary.json" ]]; then
    echo "erreur : $DIR/runs.csv ou $DIR/summary.json absent" >&2
    exit 1
fi
UNP="$(dirname "$DIR")/unprotected"
if [[ ! -f "$UNP/runs.csv" || ! -f "$UNP/summary.json" ]]; then
    echo "erreur : $UNP/runs.csv ou $UNP/summary.json absent" >&2
    exit 1
fi

python3 - "$DIR" "$UNP" <<'PY'
import collections
import json
import os
import sys
from fractions import Fraction

DIR, UNP = sys.argv[1], sys.argv[2]
CLASSES = [f"F{i}" for i in range(1, 14)] + ["D1"]
CODES = [f"E{i}" for i in range(9)]
COUNTER_COLS = [
    "resets_poweron", "resets_watchdog", "resets_brownout", "resets_commanded",
    "resets_panic", "resets_unknown", "boot_count",
    "cnt_ecc_corr", "cnt_tmr_fix", "cnt_tc_rejected", "cnt_latchup",
]
ok = True


def check(cond, msg):
    global ok
    if not cond:
        ok = False
        print(f"\n**VERIFICATION ECHOUEE** : {msg}")


def load(d):
    with open(os.path.join(d, "runs.csv"), encoding="utf-8") as f:
        lines = f.read().split("\n")
    lines = [l for l in lines if l != ""]
    header = lines[0].split(",")
    body = lines[1:]
    for l in body:
        if len(l.split(",")) != len(header):
            raise SystemExit(f"erreur : nombre de champs != en-tête dans {d}/runs.csv (ligne {l[:60]}...)")
    rows = [dict(zip(header, l.split(","))) for l in body]
    return header, body, rows


def summary(d):
    with open(os.path.join(d, "summary.json"), encoding="utf-8") as f:
        return json.load(f)


def principal(r):
    cats = r["categories"].split("+") if r["categories"] else []
    return cats[0] if cats else ""


def num(v):
    return int(v) if v != "" else 0


def pct(k, n):
    return f"{100.0 * k / n:.2f} %" if n else "n/a"


header, _, rows = load(DIR)
_, _, urows = load(UNP)
sm = summary(DIR)
su = summary(UNP)
st = sm["global"]["stats"]
nonsurv = st["n"] - st["survie"]["k"]

print(f"# Tableaux factuels (généré par scripts/failures.sh)\n")
print(f"- Source protégé : `{DIR}/runs.csv` ({len(rows)} lignes, graine_campagne {sm['graine_campagne']}, variante {sm['variante']})")
print(f"- Source non protégé : `{UNP}/runs.csv` ({len(urows)} lignes, variante {su['variante']})")
print(f"- Non-survivants selon `summary.json` : {nonsurv} / {st['n']}\n")

# consistance : échec <=> catégories non vides
for r in rows:
    check((r["survived"] == "0") == bool(r["categories"]),
          f"run {r['run_id']} : survived={r['survived']} categories='{r['categories']}'")

# ---------- (1) classe x catégorie principale ----------
tab = {c: collections.Counter() for c in CLASSES}
for r in rows:
    c = r["class"]
    tab[c]["n"] += 1
    p = principal(r)
    if p:
        tab[c][p] += 1
    else:
        tab[c]["OK"] += 1
print("## (1) Classe x catégorie principale (effectifs, protégé)\n")
print("| Classe | n | " + " | ".join(CODES) + " | Échecs | Survivants |")
print("|---|---:|" + "---:|" * len(CODES) + "---:|---:|")
tot = collections.Counter()
for c in CLASSES:
    t = tab[c]
    fails = sum(t[k] for k in CODES)
    tot.update(t)
    print(f"| {c} | {t['n']} | " + " | ".join(str(t[k]) for k in CODES) + f" | {fails} | {t['OK']} |")
fails_all = sum(tot[k] for k in CODES)
print(f"| **Total** | {tot['n']} | " + " | ".join(str(tot[k]) for k in CODES) + f" | **{fails_all}** | {tot['OK']} |\n")
check(fails_all == nonsurv,
      f"somme des échecs du tableau (1) = {fails_all} != non-survivants summary.json = {nonsurv}")
check(tot["n"] == st["n"], f"n tableau (1) = {tot['n']} != summary n = {st['n']}")
print(f"Vérification somme : tableau (1) = {fails_all}, summary.json non-survivants = {nonsurv} "
      f"-> {'OK' if fails_all == nonsurv else 'ECHEC'}\n")

# ---------- (3) F1 / F2 par région de RAM_MAP ----------
print("## (3) F1 et F2 : échecs par région de RAM_MAP (protégé)\n")
print("| Région | Genre | Runs | Échecs | Taux d'échec |")
print("|---|---|---:|---:|---:|")
reg = collections.defaultdict(lambda: [0, 0, ""])
for r in rows:
    if r["class"] not in ("F1", "F2"):
        continue
    key = r["region"] or "(vide)"
    reg[key][0] += 1
    reg[key][1] += r["survived"] == "0"
    reg[key][2] = r["region_kind"]
tot_all = [0, 0]
tot_nu = [0, 0]
for key in sorted(reg):
    n, f, kind = reg[key]
    tot_all[0] += n
    tot_all[1] += f
    if kind != "Unused":
        tot_nu[0] += n
        tot_nu[1] += f
    print(f"| {key} | {kind} | {n} | {f} | {pct(f, n)} |")
print(f"| **Total F1+F2** | | {tot_all[0]} | {tot_all[1]} | {pct(tot_all[1], tot_all[0])} |")
print(f"| **Total F1+F2 sans `Unused`** | | {tot_nu[0]} | {tot_nu[1]} | {pct(tot_nu[1], tot_nu[0])} |\n")
check(sum(v[0] for v in reg.values()) == tot_all[0], "somme régions F1/F2")

# ---------- (4) protégé vs non protégé ----------
print("## (4) Survie protégé vs non protégé par classe (écart <= 2 points)\n")
print("| Classe | Protégé n | Protégé survie | Non protégé n | Non protégé survie | Écart (pp) | Même taux à 2 pp près |")
print("|---|---:|---:|---:|---:|---:|---|")
for c in CLASSES:
    pr = [r for r in rows if r["class"] == c]
    ur = [r for r in urows if r["class"] == c]
    kp = sum(r["survived"] == "1" for r in pr)
    ku = sum(r["survived"] == "1" for r in ur)
    np_, nu = len(pr), len(ur)
    rp, ru = Fraction(kp, np_), Fraction(ku, nu)
    same = abs(rp - ru) <= Fraction(2, 100)
    print(f"| {c} | {np_} | {kp} ({pct(kp, np_)}) | {nu} | {ku} ({pct(ku, nu)}) | "
          f"{float(100 * (rp - ru)):+.2f} | {'oui' if same else 'non'} |")
print()

# ---------- (5) OK_DEGRADED ----------
deg = [r for r in rows if r["verdict"] == "OK_DEGRADED"]
print("## (5) Runs OK_DEGRADED (protégé) : motifs observables dans le CSV\n")
print(f"Total OK_DEGRADED : {len(deg)}. Les motifs ne sont pas exclusifs ; `(aucun des trois)` = ni l'un ni l'autre.\n")
mot = collections.Counter()
for r in deg:
    lm = r["last_mode"]
    ls = num(r["line_state"]) if r["line_state"] != "" else None
    sf = num(r["sensor_fault_mask"])
    a = lm != "" and int(lm) < 2
    b = lm != "" and int(lm) == 2 and ls is not None and (ls & 0b11) != 0b11
    c_ = sf != 0
    mot["mode annoncé inférieur à Nominal (last_mode < 2)"] += a
    mot["mode Nominal, Payload ou Radio éteinte dans la dernière balise (line_state & 3 != 3)"] += b
    mot["masque de capteurs invalidé (sensor_fault_mask != 0)"] += c_
    mot["(aucun des trois)"] += not (a or b or c_)
print("| Motif | Effectif |")
print("|---|---:|")
for k in ["mode annoncé inférieur à Nominal (last_mode < 2)",
          "mode Nominal, Payload ou Radio éteinte dans la dernière balise (line_state & 3 != 3)",
          "masque de capteurs invalidé (sensor_fault_mask != 0)",
          "(aucun des trois)"]:
    print(f"| {k} | {mot[k]} |")
print()
print("| Classe | OK_DEGRADED | dont last_mode | dont line_state (bits 0-1) | dont masque capteur |")
print("|---|---:|---|---|---|")
for c in CLASSES:
    d_ = [r for r in deg if r["class"] == c]
    lm = collections.Counter(r["last_mode"] for r in d_)
    ls = collections.Counter(r["line_state"] for r in d_)
    sf = sum(num(r["sensor_fault_mask"]) != 0 for r in d_)
    print(f"| {c} | {len(d_)} | {dict(sorted(lm.items()))} | {dict(sorted(ls.items()))} | {sf} |")
print()
print("Les motifs « ligne détruite » et « ligne isolée » ne sont pas dans le CSV (le verdict utilise la vérité "
      "physique de l'oracle, dont `destroyed_lines`) ; ils ne sont donc pas comptés ici.\n")

# ---------- (6) traces de détection, protégé et non protégé ----------
def detected(r):
    return (num(r["cnt_ecc_corr"]) + num(r["cnt_tmr_fix"]) + num(r["cnt_latchup"]) + num(r["cnt_tc_rejected"]) > 0
            or num(r["fdir_flags"]) != 0 or num(r["sensor_fault_mask"]) != 0)


print("## (6) Runs avec trace de détection, par classe\n")
print("Trace = `cnt_ecc_corr + cnt_tmr_fix + cnt_latchup + cnt_tc_rejected > 0`, ou `fdir_flags` ou `sensor_fault_mask` non nul. "
      "Champ vide (aucune balise reçue) compté comme absence de trace ; colonne « sans balise » donnée à part.\n")
print("| Classe | n protégé | Trace protégé | Sans balise (protégé) | n non protégé | Trace non protégé | Sans balise (non protégé) |")
print("|---|---:|---:|---:|---:|---:|---:|")
for c in CLASSES:
    pr = [r for r in rows if r["class"] == c]
    ur = [r for r in urows if r["class"] == c]
    print(f"| {c} | {len(pr)} | {sum(detected(r) for r in pr)} | {sum(r['last_mode'] == '' for r in pr)} | "
          f"{len(ur)} | {sum(detected(r) for r in ur)} | {sum(r['last_mode'] == '' for r in ur)} |")
print()

# ---------- (7) lignes identiques protégé / non protégé ----------
header_lines_p = open(os.path.join(DIR, "runs.csv"), encoding="utf-8").read().split("\n")
header_lines_p = [l for l in header_lines_p if l != ""]
hdr = header_lines_p[0].split(",")
keep = [i for i, h in enumerate(hdr) if h not in COUNTER_COLS]
with open(os.path.join(UNP, "runs.csv"), encoding="utf-8") as f:
    lines_u = [l for l in f.read().split("\n") if l != ""]
lines_p = header_lines_p
check(len(lines_p) == len(lines_u), "nombre de lignes protégé != non protégé")
ident_full = collections.Counter()
ident_nc = collections.Counter()
total = collections.Counter()
for lp, lu in zip(lines_p[1:], lines_u[1:]):
    a, b = lp.split(","), lu.split(",")
    check(a[0] == b[0] and a[2] == b[2], f"appariement run_id/classe : {a[0]} vs {b[0]}")
    c = a[2]
    total[c] += 1
    if lp == lu:
        ident_full[c] += 1
    fa = [a[i] for i in keep]
    fb = [b[i] for i in keep]
    if ",".join(fa) == ",".join(fb):
        ident_nc[c] += 1
print("## (7) Lignes identiques octet pour octet, protégé vs non protégé\n")
print("Colonnes de compteurs exclues de la colonne « hors compteurs » : " + ", ".join(COUNTER_COLS) + ".\n")
print("| Classe | n | Identiques (ligne entière) | Identiques hors compteurs |")
print("|---|---:|---:|---:|")
for c in CLASSES:
    print(f"| {c} | {total[c]} | {ident_full[c]} | {ident_nc[c]} |")
print(f"| **Total** | {sum(total.values())} | {sum(ident_full.values())} | {sum(ident_nc.values())} |\n")

# ---------- (8) F10 : S2 vrai parmi les échecs E2 ----------
f10 = [r for r in rows if r["class"] == "F10" and principal(r) == "E2"]
s2 = [r for r in f10 if r["criteria"][1] == "1"]
print("## (8) F10 : échecs E2 avec S2 vrai (protégé)\n")
print(f"- Échecs E2 de la classe F10 : {len(f10)}")
print(f"- Parmi eux, S2 vrai (2e caractère de `criteria`) : {len(s2)}\n")

# ---------- (9) F7 séparé ----------
print("## (9) F7 séparée par fault (protégé et non protégé)\n")
print("| Fault | Protégé n | Protégé échecs | Protégé E2 | Non protégé n | Non protégé échecs |")
print("|---|---:|---:|---:|---:|---:|")
for fl in ["WdgSpurious", "WdgRegHit"]:
    pr = [r for r in rows if r["class"] == "F7" and r["fault"] == fl]
    ur = [r for r in urows if r["class"] == "F7" and r["fault"] == fl]
    print(f"| {fl} | {len(pr)} | {sum(r['survived'] == '0' for r in pr)} | "
          f"{sum(principal(r) == 'E2' for r in pr)} | {len(ur)} | {sum(r['survived'] == '0' for r in ur)} |")
print()

if not ok:
    print("\nRESULTAT : une ou plusieurs verifications ont echoue.")
    sys.exit(1)
print("RESULTAT : toutes les verifications de somme passent.")
PY
