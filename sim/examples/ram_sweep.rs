//! Balayage exhaustif F1 (T-030) : un run par bit de la RAM, `RamSeu` à t_inj = 3000.
//! `cargo run --release -p sim --example ram_sweep -- [--out results/sweep] [--seeds 5501,5502,5503]`
//! Variante non protégée : ajouter `--features unprotected --target-dir target/unprotected`.
//! Sortie : `<out>/{protected,unprotected}/sweep_<graine>.csv` (byte,bit,region,verdict,resets,differs).

use sim::fault::Fault;
use sim::world::{FaultEvent, SimConfig, World, PROTECTED_BUILD};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};

const T_INJ: u32 = 3000;

/// Empreinte comparée à la référence (champs listés par T-030).
fn signature(cfg: SimConfig) -> Result<(String, String), String> {
    let n = cfg.duration_s;
    let mut w = World::try_new(cfg)?;
    for _ in 0..n {
        w.step();
    }
    let r = w.finish();
    let verdict = r.verdict.name().to_string();
    let sig = format!(
        "{} {:?} {} {:?} {:?} {} {} {} {} {:?} {}",
        verdict,
        r.resets_by_cause,
        r.boot_count,
        r.min_soc_pct.to_bits(),
        r.temp_min_c.to_bits(),
        r.tx_frames,
        r.rx_frames,
        r.nvm_writes,
        r.executed_tcs.len(),
        r.last_beacon,
        r.ground.beacons.len()
    );
    let resets = r
        .resets_by_cause
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join("/");
    Ok((format!("{verdict},{resets}"), sig))
}

fn cfg_for(seed: u64, fault: Option<Fault>) -> SimConfig {
    let mut cfg = SimConfig {
        seed,
        ..SimConfig::default()
    };
    cfg.protections = PROTECTED_BUILD;
    if let Some(f) = fault {
        cfg.faults = vec![FaultEvent { t: T_INJ, fault: f }];
    }
    cfg
}

fn run_guarded(cfg: SimConfig) -> (String, String) {
    match catch_unwind(AssertUnwindSafe(|| signature(cfg))) {
        Ok(Ok(x)) => x,
        Ok(Err(e)) => ("HARNESS,".into(), format!("HARNESS {e}")),
        Err(_) => ("HARNESS,".into(), "HARNESS panic".into()),
    }
}

fn main() {
    let mut out = "results/sweep".to_string();
    let mut seeds: Vec<u64> = vec![5501, 5502, 5503];
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let val = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--out" => out = val,
            "--seeds" => {
                seeds = val
                    .split(',')
                    .map(|s| s.parse().expect("--seeds a,b,c"))
                    .collect()
            }
            other => {
                eprintln!("argument inconnu : {other}");
                std::process::exit(2);
            }
        }
        i += 2;
    }
    std::panic::set_hook(Box::new(|_| {}));
    let variant = if PROTECTED_BUILD {
        "protected"
    } else {
        "unprotected"
    };
    let dir = format!("{out}/{variant}");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let nbits = firmware::RAM_SIZE * 8;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    for seed in seeds {
        let (_, ref_sig) = run_guarded(cfg_for(seed, None));
        let next = AtomicUsize::new(0);
        let mut rows: Vec<Option<String>> = vec![None; nbits];
        let parts: Vec<Vec<(usize, String)>> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..threads)
                .map(|_| {
                    s.spawn(|| {
                        let mut v = Vec::new();
                        loop {
                            let k = next.fetch_add(1, Ordering::Relaxed);
                            if k >= nbits {
                                break;
                            }
                            let (byte, bit) = (k / 8, k % 8);
                            let region = firmware::region_of(byte).map_or("?", |r| r.name);
                            let f = Fault::RamSeu {
                                byte: byte as u16,
                                bit: bit as u8,
                            };
                            let (head, sig) = run_guarded(cfg_for(seed, Some(f)));
                            let differs = u8::from(sig != ref_sig);
                            let (verdict, resets) = head.split_once(',').unwrap();
                            v.push((
                                k,
                                format!("{byte},{bit},{region},{verdict},{resets},{differs}"),
                            ));
                        }
                        v
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for (k, line) in parts.into_iter().flatten() {
            rows[k] = Some(line);
        }
        let mut text = String::from("byte,bit,region,verdict,resets,differs\n");
        for r in rows {
            text.push_str(&r.expect("ligne manquante"));
            text.push('\n');
        }
        std::fs::write(format!("{dir}/sweep_{seed}.csv"), text).expect("write");
        eprintln!("{variant} graine {seed} : fait");
    }
}
