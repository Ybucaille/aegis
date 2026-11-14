//! Campagne d'injection de pannes Aegis.
//!
//! ```text
//! cargo run --release -p sim --bin campaign -- --seed S --runs N [--class C] [--unprotected]
//!     [--replay SEED] [--threads N] --out results/<nom>
//! ```
//! Témoin sans protections : compiler dans un répertoire cible séparé et passer `--unprotected` :
//! `cargo run --release --target-dir target/unprotected -p sim --features unprotected --bin campaign -- --unprotected ...`
//! `--class` : `none` (témoin sans panne), `double` (D1), `f1`..`f13`, `d1` ; absent = F1..F13 et D1
//! en alternance (run i = classe i mod 14, soit >= 1000 runs par classe pour 20 000 runs).
//! `--replay SEED` : graine d'une ligne de `runs.csv` ; sans `--class`, la classe est retrouvée en
//! balayant `--runs` runs de la campagne `--seed`.

use sim::campaign::{self, Options};
use sim::fault::FaultClass;
use sim::world::PROTECTED_BUILD;
use std::process::{exit, Command};
use std::time::Instant;

fn die(msg: &str) -> ! {
    eprintln!("campaign : {msg}");
    exit(2);
}

fn git_version() -> String {
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    match run(&["rev-parse", "--short", "HEAD"]) {
        Some(h) => {
            let dirty = run(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
            if dirty {
                format!("{h}-dirty")
            } else {
                h
            }
        }
        None => "inconnue".to_string(),
    }
}

fn main() {
    let mut opts = Options {
        seed: 1,
        runs: 1000,
        class: None,
        unprotected: false,
        threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
    };
    let mut out: Option<String> = None;
    let mut replay: Option<u64> = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut val = |name: &str| {
            it.next()
                .unwrap_or_else(|| die(&format!("{name} attend une valeur")))
        };
        let num = |name: &str, s: String| -> u64 {
            s.parse()
                .unwrap_or_else(|_| die(&format!("{name} : entier attendu, reçu {s:?}")))
        };
        match a.as_str() {
            "--seed" => opts.seed = num("--seed", val("--seed")),
            "--runs" => opts.runs = num("--runs", val("--runs")),
            "--threads" => opts.threads = num("--threads", val("--threads")).max(1) as usize,
            "--replay" => replay = Some(num("--replay", val("--replay"))),
            "--out" => out = Some(val("--out")),
            "--unprotected" => opts.unprotected = true,
            "--class" => {
                let s = val("--class");
                opts.class = Some(
                    FaultClass::parse(&s).unwrap_or_else(|| die(&format!("classe inconnue {s:?}"))),
                );
            }
            other => die(&format!("option inconnue {other:?}")),
        }
    }
    if opts.unprotected == PROTECTED_BUILD {
        die(&format!(
            "--unprotected={} mais ce binaire est compilé {} ; compiler le témoin avec : cargo run \
             --release --target-dir target/unprotected -p sim --features unprotected --bin campaign -- --unprotected ...",
            opts.unprotected,
            if PROTECTED_BUILD { "AVEC protections" } else { "SANS protections" }
        ));
    }
    campaign::install_quiet_panic_hook();

    if let Some(seed) = replay {
        match campaign::replay(&opts, seed) {
            Ok((_, trace)) => print!("{trace}"),
            Err(e) => die(&e),
        }
        return;
    }

    let out = out.unwrap_or_else(|| die("--out <répertoire> requis"));
    println!(
        "campagne : graine {} ; {} runs ; classe {} ; variante {} ; {} fils",
        opts.seed,
        opts.runs,
        opts.class.map_or("mixte", |c| c.label()),
        if opts.unprotected {
            "unprotected"
        } else {
            "protected"
        },
        opts.threads
    );
    let t0 = Instant::now();
    let rows = campaign::run_campaign(&opts);
    let elapsed = t0.elapsed().as_secs_f64();
    std::fs::create_dir_all(&out).unwrap_or_else(|e| die(&format!("création de {out} : {e}")));
    std::fs::write(format!("{out}/runs.csv"), campaign::csv(&rows))
        .unwrap_or_else(|e| die(&format!("écriture runs.csv : {e}")));
    let summary = campaign::summary(&opts, &rows, &git_version(), elapsed);
    std::fs::write(format!("{out}/summary.json"), summary.to_pretty())
        .unwrap_or_else(|e| die(&format!("écriture summary.json : {e}")));

    let surv = rows.iter().filter(|r| r.survived).count();
    let e0: Vec<_> = rows.iter().filter(|r| r.harness.is_some()).collect();
    println!(
        "{} runs en {:.1} s ({:.1} runs/s au total, {:.1} par fil) ; survie brute {}/{}",
        rows.len(),
        elapsed,
        rows.len() as f64 / elapsed,
        rows.len() as f64 / elapsed / opts.threads as f64,
        surv,
        rows.len() - e0.len()
    );
    if !e0.is_empty() {
        eprintln!(
            "ATTENTION : {} run(s) E0 HARNESS (exclus des taux) :",
            e0.len()
        );
        for r in e0.iter().take(20) {
            eprintln!(
                "  run {} graine {} classe {} : {}",
                r.run_id,
                r.seed,
                r.class.label(),
                r.harness.as_deref().unwrap_or("")
            );
        }
    }
    println!("écrit : {out}/runs.csv, {out}/summary.json");
}
