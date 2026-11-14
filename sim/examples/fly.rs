//! Vol nominal textuel : `cargo run -p sim --example fly -- --seed 1 [--soc 60] [--phase 0] [--duration 17100]`.
//! Une ligne toutes les 300 s : t, éclipse, SoC, V, T_batt, mode, balises reçues, ACK reçus.

use sim::world::{SimConfig, World};

fn main() {
    let mut cfg = SimConfig::default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let val = args.get(i + 1).map(String::as_str).unwrap_or("");
        match args[i].as_str() {
            "--seed" => cfg.seed = val.parse().expect("--seed <u64>"),
            "--soc" => cfg.params.soc_init_pct = Some(val.parse().expect("--soc <pct>")),
            "--phase" => cfg.params.phase_init_s = Some(val.parse().expect("--phase <s>")),
            "--duration" => cfg.duration_s = val.parse().expect("--duration <s>"),
            other => {
                eprintln!("argument inconnu : {other}");
                std::process::exit(2);
            }
        }
        i += 2;
    }
    println!(
        "graine={} duree={}s soc_init={:?} phase_init={:?}",
        cfg.seed, cfg.duration_s, cfg.params.soc_init_pct, cfg.params.phase_init_s
    );
    println!("   t  ecl pass   SoC%    V(mV)  Tbatt   mode  cpu       boots  beacons  acks");
    let n = cfg.duration_s;
    let mut w = World::new(cfg);
    for _ in 0..n {
        let s = w.step();
        if s.t.is_multiple_of(300) {
            let mode = s
                .last_beacon
                .map(|b| b.1.to_string())
                .unwrap_or_else(|| "-".into());
            println!(
                "{:5}  {}    {}   {:5.1}  {:7.0}  {:5.1}  fw={:?} bal={:>2} {:<9?} {:3}  {:7}  {:4}",
                s.t,
                if s.in_eclipse { "E" } else { "." },
                if s.in_pass { "P" } else { "." },
                s.soc_pct,
                s.vbatt_mv,
                s.temp_c,
                s.mode.map(|m| m as u8),
                mode,
                s.cpu,
                s.boot_count,
                s.beacons_rx,
                s.acks_rx
            );
        }
    }
    let r = w.finish();
    println!(
        "fin : resets={:?} SoC_min={:.1}% T=[{:.1},{:.1}] balises={} ack={} rejets_sol={}",
        r.resets_by_cause,
        r.min_soc_pct,
        r.temp_min_c,
        r.temp_max_c,
        r.ground.beacons.len(),
        r.ground.acks.len(),
        r.ground.rejects
    );
}
