//! Campagne d'injection de masse (SPEC §7.1, §8) : exécution parallèle déterministe, `runs.csv`,
//! `summary.json`, rejeu d'un run.
//!
//! Le résultat d'un run ne dépend que de `(graine du run, classe)` : ni du nombre de fils ni de
//! l'ordre d'exécution. Les lignes sont triées par `run_id` avant écriture.

use crate::fault::{self, FaultClass};
use crate::json::Value;
use crate::metrics::RunResult;
use crate::oracle::{Category, Verdict};
use crate::prng::splitmix64;
use crate::world::{World, PROTECTED_BUILD};
use firmware::{ResetCause, RAM_MAP};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Options de campagne.
#[derive(Clone, Debug)]
pub struct Options {
    pub seed: u64,
    pub runs: u64,
    /// `None` : répartition cyclique sur F1..F13 et D1 (run `i` -> classe `i mod 14`).
    pub class: Option<FaultClass>,
    pub unprotected: bool,
    pub threads: usize,
}

/// Une ligne de `runs.csv`.
#[derive(Clone, Debug)]
pub struct Row {
    pub run_id: u64,
    pub seed: u64,
    pub class: FaultClass,
    pub fault: String,
    pub params: String,
    pub t_inj: Option<u32>,
    /// `None` : classe sans panne. `Some(false)` : au moins une injection jamais appliquée.
    pub applied: Option<bool>,
    pub region: String,
    pub region_kind: String,
    pub verdict: String,
    pub categories: Vec<Category>,
    pub survived: bool,
    pub nominal: bool,
    /// Message de panique du simulateur lui-même (E0) ; le run est exclu des taux.
    pub harness: Option<String>,
    pub criteria: [bool; 6],
    pub resets: [u32; 6],
    pub panics: u32,
    pub delay_beacon: Option<u32>,
    pub delay_nominal: Option<u32>,
    pub min_soc: f64,
    pub temp_min: f64,
    pub temp_max: f64,
    pub boot_count: u32,
    pub last_mode: Option<u8>,
}

/// Classe du run `i`.
pub fn class_of(opts: &Options, i: u64) -> FaultClass {
    opts.class
        .unwrap_or(FaultClass::ALL[(i % FaultClass::ALL.len() as u64) as usize])
}

/// Graine du run `i` (SPEC §7.1).
pub fn run_seed(campaign_seed: u64, i: u64) -> u64 {
    splitmix64(campaign_seed ^ i)
}

/// Type de région de `RAM_MAP` d'après son nom (`Unused` compris), `?` si inconnue.
fn region_kind_of(name: &str) -> String {
    RAM_MAP
        .iter()
        .find(|r| r.name == name)
        .map_or("?".to_string(), |r| format!("{:?}", r.kind))
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "panique sans message".to_string()
    }
}

/// Remplace le hook de panique par défaut (qui imprime, sérialise les fils sur stderr et noie la
/// sortie) par un hook muet. Les paniques du firmware sont rattrapées par `World::step` et
/// comptées (cause `Panic`) ; toute autre panique remonte jusqu'à `execute` et devient un E0 listé.
pub fn install_quiet_panic_hook() {
    std::panic::set_hook(Box::new(|_| {}));
}

fn run_world(cfg: crate::world::SimConfig) -> Result<RunResult, String> {
    let n = cfg.duration_s;
    let mut w = World::try_new(cfg)?;
    for _ in 0..n {
        w.step();
    }
    Ok(w.finish())
}

/// Exécute un run ; renvoie la ligne et, hors E0, le résultat complet (pour le rejeu).
pub fn execute(
    opts: &Options,
    run_id: u64,
    seed: u64,
    class: FaultClass,
) -> (Row, Option<RunResult>) {
    let mut cfg = fault::sim_config(class, seed);
    cfg.protections = !opts.unprotected;
    match catch_unwind(AssertUnwindSafe(|| run_world(cfg))) {
        Ok(Ok(r)) => (row_of(run_id, seed, class, &r), Some(r)),
        Ok(Err(e)) => (harness_row(run_id, seed, class, e), None),
        Err(p) => (harness_row(run_id, seed, class, panic_text(&*p)), None),
    }
}

fn harness_row(run_id: u64, seed: u64, class: FaultClass, msg: String) -> Row {
    Row {
        run_id,
        seed,
        class,
        fault: String::new(),
        params: String::new(),
        t_inj: None,
        applied: None,
        region: String::new(),
        region_kind: String::new(),
        verdict: Category::Harness.name().to_string(),
        categories: vec![Category::Harness],
        survived: false,
        nominal: false,
        harness: Some(msg),
        criteria: [false; 6],
        resets: [0; 6],
        panics: 0,
        delay_beacon: None,
        delay_nominal: None,
        min_soc: f64::NAN,
        temp_min: f64::NAN,
        temp_max: f64::NAN,
        boot_count: 0,
        last_mode: None,
    }
}

fn row_of(run_id: u64, seed: u64, class: FaultClass, r: &RunResult) -> Row {
    let inj = &r.injections;
    let applied = if inj.is_empty() {
        None
    } else {
        Some(inj.iter().all(|i| i.applied_t.is_some()))
    };
    let region = r.ram_region.clone().unwrap_or_default();
    let region_kind = if region.is_empty() {
        String::new()
    } else {
        region_kind_of(&region)
    };
    Row {
        run_id,
        seed,
        class,
        fault: inj.iter().map(|i| i.fault).collect::<Vec<_>>().join("+"),
        params: inj
            .iter()
            .map(|i| i.params.as_str())
            .collect::<Vec<_>>()
            .join(" | "),
        t_inj: r.inject_t,
        applied,
        region,
        region_kind,
        verdict: r.verdict.name().to_string(),
        categories: r.categories().to_vec(),
        survived: r.verdict.survived(),
        nominal: r.verdict == Verdict::OkNominal,
        harness: None,
        criteria: r.outcome.criteria,
        resets: r.resets_by_cause,
        panics: r.resets_by_cause[ResetCause::Panic as usize],
        delay_beacon: r.delay_to_beacon_s,
        delay_nominal: r.delay_to_nominal_s,
        min_soc: r.min_soc_pct,
        temp_min: r.temp_min_c,
        temp_max: r.temp_max_c,
        boot_count: r.boot_count,
        last_mode: r.last_beacon.as_ref().map(|b| b.mode),
    }
}

/// Exécute toute la campagne ; lignes triées par `run_id`.
pub fn run_campaign(opts: &Options) -> Vec<Row> {
    let next = AtomicU64::new(0);
    let all: Mutex<Vec<Row>> = Mutex::new(Vec::with_capacity(opts.runs as usize));
    let threads = opts.threads.max(1);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut local = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= opts.runs {
                        break;
                    }
                    let (row, _) = execute(opts, i, run_seed(opts.seed, i), class_of(opts, i));
                    local.push(row);
                }
                all.lock().expect("mutex").extend(local);
            });
        }
    });
    let mut rows = all.into_inner().expect("mutex");
    rows.sort_by_key(|r| r.run_id);
    rows
}

// ---------------------------------------------------------------------------------------------
// runs.csv
// ---------------------------------------------------------------------------------------------

pub const CSV_HEADER: &str = "run_id,seed,class,fault,params,t_inj,applied,region,region_kind,verdict,categories,survived,nominal,criteria,resets_poweron,resets_watchdog,resets_brownout,resets_commanded,resets_panic,resets_unknown,delay_beacon_s,delay_nominal_s,min_soc_pct,temp_min_c,temp_max_c,boot_count,last_mode,harness_msg";

fn csv_field(s: &str, out: &mut String) {
    if s.contains([',', '"', '\n', '\r']) {
        out.push('"');
        out.push_str(&s.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(s);
    }
}

fn opt<T: ToString>(o: Option<T>) -> String {
    o.map_or(String::new(), |v| v.to_string())
}

/// Ligne CSV d'un run (sans fin de ligne).
pub fn csv_line(r: &Row) -> String {
    let mut o = String::new();
    let mut f = |s: &str, last: bool| {
        csv_field(s, &mut o);
        if !last {
            o.push(',');
        }
    };
    f(&r.run_id.to_string(), false);
    f(&r.seed.to_string(), false);
    f(r.class.label(), false);
    f(&r.fault, false);
    f(&r.params, false);
    f(&opt(r.t_inj), false);
    f(&opt(r.applied.map(u8::from)), false);
    f(&r.region, false);
    f(&r.region_kind, false);
    f(&r.verdict, false);
    f(
        &r.categories
            .iter()
            .map(|c| c.code())
            .collect::<Vec<_>>()
            .join("+"),
        false,
    );
    f(&u8::from(r.survived).to_string(), false);
    f(&u8::from(r.nominal).to_string(), false);
    f(
        &r.criteria
            .iter()
            .map(|b| if *b { '1' } else { '0' })
            .collect::<String>(),
        false,
    );
    for n in r.resets {
        f(&n.to_string(), false);
    }
    f(&opt(r.delay_beacon), false);
    f(&opt(r.delay_nominal), false);
    f(&fmt_f(r.min_soc), false);
    f(&fmt_f(r.temp_min), false);
    f(&fmt_f(r.temp_max), false);
    f(&r.boot_count.to_string(), false);
    f(&opt(r.last_mode), false);
    f(r.harness.as_deref().unwrap_or(""), true);
    o
}

fn fmt_f(x: f64) -> String {
    if x.is_finite() {
        format!("{x:.3}")
    } else {
        String::new()
    }
}

pub fn csv(rows: &[Row]) -> String {
    let mut s = String::from(CSV_HEADER);
    s.push('\n');
    for r in rows {
        s.push_str(&csv_line(r));
        s.push('\n');
    }
    s
}

// ---------------------------------------------------------------------------------------------
// Statistiques
// ---------------------------------------------------------------------------------------------

/// Intervalle de Wilson à 95 % pour `k` succès sur `n` (n = 0 : [0, 1]).
pub fn wilson(k: u64, n: u64) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let z = 1.959_963_984_540_054_f64;
    let n = n as f64;
    let p = k as f64 / n;
    let z2 = z * z;
    let den = 1.0 + z2 / n;
    let centre = (p + z2 / (2.0 * n)) / den;
    let half = z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / den;
    ((centre - half).max(0.0), (centre + half).min(1.0))
}

fn rate_json(k: u64, n: u64) -> Value {
    let (lo, hi) = wilson(k, n);
    Value::obj()
        .with("k", k)
        .with(
            "taux",
            if n == 0 {
                f64::NAN
            } else {
                k as f64 / n as f64
            },
        )
        .with("wilson95", vec![Value::Num(lo), Value::Num(hi)])
}

/// Effectif, survie et retour au nominal d'un sous-ensemble de runs (E0 déjà exclus).
fn stats_json(rows: &[&Row]) -> Value {
    let n = rows.len() as u64;
    let surv = rows.iter().filter(|r| r.survived).count() as u64;
    let nom = rows.iter().filter(|r| r.nominal).count() as u64;
    Value::obj()
        .with("n", n)
        .with("survie", rate_json(surv, n))
        .with("retour_nominal", rate_json(nom, n))
}

fn category_counts(rows: &[&Row]) -> Value {
    let cats = [
        Category::Harness,
        Category::VitalLoss,
        Category::Thermal,
        Category::Dead,
        Category::BootLoop,
        Category::Power,
        Category::CommsLost,
        Category::CmdIntegrity,
        Category::SilentLie,
    ];
    let mut primary = Value::obj();
    let mut any = Value::obj();
    for c in cats.iter().skip(1) {
        let p = rows
            .iter()
            .filter(|r| r.categories.first() == Some(c))
            .count() as u64;
        let a = rows.iter().filter(|r| r.categories.contains(c)).count() as u64;
        let key = format!("{} {}", c.code(), c.name());
        primary = primary.with(&key, p);
        any = any.with(&key, a);
    }
    Value::obj()
        .with("principale", primary)
        .with("toutes_applicables", any)
}

const KINDS: [&str; 7] = ["Sched", "Mode", "Fdir", "Proto", "Tm", "Log", "Unused"];

fn class_json(rows: &[&Row], class: Option<FaultClass>) -> Value {
    let na: Vec<&Row> = rows
        .iter()
        .copied()
        .filter(|r| r.applied == Some(false))
        .collect();
    let applied: Vec<&Row> = rows
        .iter()
        .copied()
        .filter(|r| r.applied != Some(false))
        .collect();
    let panics: u64 = rows.iter().map(|r| r.panics as u64).sum();
    let mut v = match stats_json(rows) {
        v @ Value::Obj(_) => v,
        _ => unreachable!(),
    };
    v = v
        .with("categories", category_counts(rows))
        .with("injection_jamais_appliquee", na.len() as u64)
        .with("hors_injections_non_appliquees", stats_json(&applied))
        .with("paniques_firmware_comptees", panics);
    if matches!(class, Some(FaultClass::F1 | FaultClass::F2)) {
        let mut by = Value::obj();
        for k in KINDS {
            let sub: Vec<&Row> = rows
                .iter()
                .copied()
                .filter(|r| r.region_kind == k)
                .collect();
            by = by.with(k, stats_json(&sub));
        }
        let without: Vec<&Row> = rows
            .iter()
            .copied()
            .filter(|r| r.region_kind != "Unused")
            .collect();
        v = v
            .with("par_type_de_region", by)
            .with("avec_region_unused", stats_json(rows))
            .with("sans_region_unused", stats_json(&without));
    }
    v
}

/// `summary.json`. `git` et `elapsed_s` sont informatifs (hors `runs.csv`, donc hors identité).
pub fn summary(opts: &Options, rows: &[Row], git: &str, elapsed_s: f64) -> Value {
    let ok: Vec<&Row> = rows.iter().filter(|r| r.harness.is_none()).collect();
    let excluded: Vec<Value> = rows
        .iter()
        .filter(|r| r.harness.is_some())
        .map(|r| {
            Value::obj()
                .with("run_id", r.run_id)
                .with("seed", r.seed)
                .with("classe", r.class.label())
                .with("message", r.harness.clone())
        })
        .collect();
    let mut classes = Value::obj();
    let mut seen: Vec<FaultClass> = rows.iter().map(|r| r.class).collect();
    seen.sort();
    seen.dedup();
    for c in seen {
        let sub: Vec<&Row> = ok.iter().copied().filter(|r| r.class == c).collect();
        classes = classes.with(c.label(), class_json(&sub, Some(c)));
    }
    let variant = if PROTECTED_BUILD {
        "protected"
    } else {
        "unprotected"
    };
    Value::obj()
        .with("variante", variant)
        .with("graine_campagne", opts.seed)
        .with("git", git)
        .with(
            "options",
            Value::obj()
                .with("seed", opts.seed)
                .with("runs", opts.runs)
                .with(
                    "class",
                    opts.class.map_or("mixte (F1..F13, D1 cycliques)", |c| c.label()),
                )
                .with("unprotected", opts.unprotected)
                .with("threads", opts.threads),
        )
        .with("runs_total", rows.len() as u64)
        .with("runs_e0_exclus", excluded.len() as u64)
        .with("e0_exclus", excluded)
        .with(
            "global",
            Value::obj()
                .with(
                    "avertissement",
                    "jamais de taux global sans le detail par classe ; la ponderation entre classes est arbitraire",
                )
                .with("stats", class_json(&ok, None)),
        )
        .with("par_classe", classes)
        .with(
            "debit",
            Value::obj()
                .with("duree_s", elapsed_s)
                .with(
                    "runs_par_s",
                    if elapsed_s > 0.0 {
                        rows.len() as f64 / elapsed_s
                    } else {
                        f64::NAN
                    },
                ),
        )
}

// ---------------------------------------------------------------------------------------------
// Rejeu
// ---------------------------------------------------------------------------------------------

/// Retrouve `(run_id, classe)` d'une graine de run : classe imposée, sinon balayage de
/// `0..opts.runs` pour la campagne `opts.seed`.
pub fn locate(opts: &Options, run_seed_value: u64) -> Option<(u64, FaultClass)> {
    if let Some(c) = opts.class {
        let id = (0..opts.runs).find(|&i| run_seed(opts.seed, i) == run_seed_value);
        return Some((id.unwrap_or(0), c));
    }
    (0..opts.runs)
        .find(|&i| run_seed(opts.seed, i) == run_seed_value)
        .map(|i| (i, class_of(opts, i)))
}

/// Rejoue un run : ligne CSV identique à celle de la campagne et trace lisible.
pub fn replay(opts: &Options, run_seed_value: u64) -> Result<(Row, String), String> {
    let (id, class) = locate(opts, run_seed_value).ok_or_else(|| {
        format!(
            "graine {run_seed_value} introuvable parmi les {} runs de la campagne --seed {} : \
             donner --class C, ou --seed/--runs de la campagne d'origine",
            opts.runs, opts.seed
        )
    })?;
    let (row, res) = execute(opts, id, run_seed_value, class);
    let mut t = String::new();
    t.push_str(&format!(
        "run_id={id} seed={run_seed_value} classe={} variante={}\n",
        class.label(),
        if opts.unprotected {
            "unprotected"
        } else {
            "protected"
        }
    ));
    if let Some(m) = &row.harness {
        t.push_str(&format!("E0 HARNESS : {m}\n"));
    }
    if let Some(r) = &res {
        for i in &r.injections {
            t.push_str(&format!(
                "injection {} params=[{}] t_inj={} appliquee_a={} touche=[{}]\n",
                i.fault,
                i.params,
                i.t_inj,
                opt(i.applied_t),
                i.touched
            ));
        }
        t.push_str(&format!(
            "resets : {:?} (journal {:?})\n",
            r.resets_by_cause, r.reset_log
        ));
        t.push_str(&format!(
            "balises recues : {} ; modes annonces (t,mode,boot) : {:?}\n",
            r.ground.beacons.len(),
            r.ground.beacons
        ));
        t.push_str(&format!("TC executees (t,seq,op) : {:?}\n", r.executed_tcs));
        t.push_str(&format!(
            "criteres S1..S6 : {:?} ; violations S5 : {:?}\n",
            r.outcome.criteria, r.outcome.s5_violations
        ));
        t.push_str(&format!("derniere balise : {:?}\n", r.last_beacon));
    }
    t.push_str(&format!(
        "verdict={} categories={}\n",
        row.verdict,
        row.categories
            .iter()
            .map(|c| c.code())
            .collect::<Vec<_>>()
            .join("+")
    ));
    t.push_str(&format!("{CSV_HEADER}\n{}\n", csv_line(&row)));
    Ok((row, t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_50_of_100() {
        let (lo, hi) = wilson(50, 100);
        assert!((lo - 0.404).abs() < 0.001, "{lo}");
        assert!((hi - 0.596).abs() < 0.001, "{hi}");
    }

    #[test]
    fn wilson_edges() {
        assert_eq!(wilson(0, 0), (0.0, 1.0));
        let (lo, hi) = wilson(100, 100);
        assert!(lo > 0.95 && (hi - 1.0).abs() < 1e-12);
        let (lo, hi) = wilson(0, 100);
        assert!(lo.abs() < 1e-12 && hi < 0.05);
    }

    #[test]
    fn csv_escaping() {
        let mut s = String::new();
        csv_field("a,b\"c", &mut s);
        assert_eq!(s, "\"a,b\"\"c\"");
    }

    #[test]
    fn classes_cycle_and_cover() {
        let o = Options {
            seed: 1,
            runs: 20_000,
            class: None,
            unprotected: false,
            threads: 1,
        };
        let mut n = std::collections::BTreeMap::new();
        for i in 0..o.runs {
            *n.entry(class_of(&o, i)).or_insert(0u32) += 1;
        }
        assert_eq!(n.len(), 14);
        assert!(n.values().all(|&c| c >= 1000), "{n:?}");
    }
}
