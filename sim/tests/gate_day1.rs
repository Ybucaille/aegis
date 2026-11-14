//! GATE FIN DE JOUR 1 (T-009) : le satellite tourne sans panne, sur 20 graines, sur une grille de
//! 24 coins (SoC initial x phase initiale), avec calibration énergie / thermique et déterminisme.
//!
//! Règle : les contrôles ne sont PAS retouchés ici. Chaque échec est collecté (graine ou coin, critère,
//! valeurs) et le test échoue à la fin avec la liste complète. Les `println!` (lire avec `-- --nocapture`)
//! donnent les lignes `seed=N resets=R mode=M` (20 lignes) et `coin soc=.. phase=.. ...` (24 lignes).

use sim::world::{Cpu, RunResult, SimConfig, Snapshot, World};

const SEEDS: std::ops::RangeInclusive<u64> = 1..=20;
const SOC_COINS: [f64; 3] = [60.0, 80.0, 100.0];
const PHASE_COINS: [u32; 8] = [0, 712, 1425, 2137, 2850, 3562, 4275, 4987];
/// Bit de la ligne Payload dans `Snapshot::lines` (`Line::Payload = 0`).
const LINE_PAYLOAD: u8 = 1 << 0;

fn fly(cfg: SimConfig) -> (Vec<Snapshot>, RunResult) {
    let n = cfg.duration_s;
    let mut w = World::new(cfg);
    let snaps: Vec<Snapshot> = (0..n).map(|_| w.step()).collect();
    (snaps, w.finish())
}

/// Passes `(début, fin exclue, balises reçues, ACK reçus, entièrement dans le run)`.
/// Copie de la logique de `sim/tests/nominal.rs` (ce fichier n'importe pas les autres tests).
fn passes(snaps: &[Snapshot]) -> Vec<(u32, u32, u32, u32, bool)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < snaps.len() {
        if snaps[i].in_pass {
            let start = i;
            while i < snaps.len() && snaps[i].in_pass {
                i += 1;
            }
            let (b0, a0) = if start == 0 {
                (0, 0)
            } else {
                (snaps[start - 1].beacons_rx, snaps[start - 1].acks_rx)
            };
            let last = &snaps[i - 1];
            out.push((
                start as u32,
                i as u32,
                last.beacons_rx - b0,
                last.acks_rx - a0,
                start > 0 && i < snaps.len(),
            ));
        } else {
            i += 1;
        }
    }
    out
}

fn default_cfg(seed: u64) -> SimConfig {
    SimConfig {
        seed,
        ..SimConfig::default()
    }
}

/// Contrôles de la passe (a) pour une graine : même critères que `nominal_flight_seed1`,
/// plus mode Nominal et ligne Payload alimentée à T_RUN. Retourne les échecs (vide = OK).
fn check_nominal_flight(seed: u64, snaps: &[Snapshot], r: &RunResult) -> Vec<String> {
    let mut f = Vec::new();
    let last = snaps.last().expect("run vide");
    if r.resets_by_cause != [0; 6] {
        f.push(format!(
            "seed={seed} : resets_by_cause={:?} (attendu [0;6])",
            r.resets_by_cause
        ));
    }
    if r.boot_count != 1 {
        f.push(format!(
            "seed={seed} : boot_count={} (attendu 1)",
            r.boot_count
        ));
    }
    let ps = passes(snaps);
    let full: Vec<_> = ps.iter().filter(|p| p.4).collect();
    if full.len() < 2 {
        f.push(format!(
            "seed={seed} : passes entières={} (attendu >= 2) : {ps:?}",
            full.len()
        ));
    }
    for p in &full {
        if p.2 < 1 {
            f.push(format!("seed={seed} : passe sans balise : {p:?}"));
        }
        if p.3 < 1 {
            f.push(format!("seed={seed} : passe sans ACK : {p:?}"));
        }
    }
    match last.last_beacon {
        None => f.push(format!("seed={seed} : aucune balise reçue")),
        Some((_, mode, _)) => {
            if mode != firmware::Mode::Nominal as u8 {
                f.push(format!(
                    "seed={seed} : mode annoncé par la dernière balise={mode} (attendu {})",
                    firmware::Mode::Nominal as u8
                ));
            }
        }
    }
    if last.mode != Some(firmware::Mode::Nominal) {
        f.push(format!(
            "seed={seed} : mode final={:?} (attendu Nominal)",
            last.mode
        ));
    }
    if last.soc_pct < 20.0 {
        f.push(format!(
            "seed={seed} : SoC final={:.2} % (attendu >= 20)",
            last.soc_pct
        ));
    }
    if !(r.temp_min_c >= -10.0 && r.temp_max_c <= 50.0) {
        f.push(format!(
            "seed={seed} : T batterie=[{:.2}, {:.2}] C (attendu dans [-10, 50])",
            r.temp_min_c, r.temp_max_c
        ));
    }
    if r.ground.rejects != 0 {
        f.push(format!(
            "seed={seed} : trames rejetees par le sol={} (attendu 0)",
            r.ground.rejects
        ));
    }
    if !(last.obc_powered && last.cpu == Cpu::Running) {
        f.push(format!(
            "seed={seed} : OBC alimenté={} cpu={:?} à T_RUN (attendu true / Running)",
            last.obc_powered, last.cpu
        ));
    }
    if last.lines & LINE_PAYLOAD == 0 {
        f.push(format!(
            "seed={seed} : ligne Payload non alimentée à T_RUN (lines={:#05b})",
            last.lines
        ));
    }
    f
}

#[test]
fn gate_day1() {
    let mut failures: Vec<String> = Vec::new();

    // ---------------------------------------------------------------------------------------------
    // (a) 20 graines, vol de T_RUN sans panne, config par défaut (SoC et phase tirés par la graine).
    // (b, thermostat) : T_batt >= 0 °C sur 3 orbites, vérifié sur les mêmes 20 vols.
    // ---------------------------------------------------------------------------------------------
    println!("== (a) 20 graines, vol nominal ==");
    for seed in SEEDS {
        let cfg = default_cfg(seed);
        assert_eq!(cfg.duration_s, 17_100, "T_RUN");
        let (snaps, r) = fly(cfg);
        let resets: u32 = r.resets_by_cause.iter().sum();
        let mode = match snaps.last().and_then(|s| s.mode) {
            Some(m) => format!("{m:?}"),
            None => "aucun".to_string(),
        };
        println!("seed={seed} resets={resets} mode={mode}");
        failures.extend(check_nominal_flight(seed, &snaps, &r));
        if r.temp_min_c < 0.0 {
            failures.push(format!(
                "seed={seed} : thermostat : T_batt min={:.2} C (attendu >= 0 sur 3 orbites)",
                r.temp_min_c
            ));
        }
    }

    // ---------------------------------------------------------------------------------------------
    // (b, énergie) : bilan positif sur une orbite en Nominal, départ à 80 % de SoC.
    // Fenêtre = première orbite (orbit_period_s) du run ; on exige SoC fin >= SoC début,
    // bilan (produit - consommé) > 0 Wh, et mode Nominal à la fin de la fenêtre.
    // ---------------------------------------------------------------------------------------------
    println!("== (b) calibration énergie, orbite 1, SoC initial 80 % ==");
    for seed in SEEDS {
        let mut cfg = default_cfg(seed);
        cfg.params.soc_init_pct = Some(80.0);
        let period = cfg.params.orbit_period_s;
        cfg.duration_s = period;
        let (snaps, _r) = fly(cfg);
        let first = &snaps[0];
        let last = snaps.last().unwrap();
        let prod: f64 = snaps.iter().map(|x| x.solar_w).sum::<f64>() / 3600.0;
        let cons: f64 = snaps.iter().map(|x| x.load_w).sum::<f64>() / 3600.0;
        println!(
            "calib seed={seed} : SoC {:.2} -> {:.2} %, produit {prod:.3} Wh, consomme {cons:.3} Wh, bilan {:+.3} Wh, mode fin={:?}",
            first.soc_pct,
            last.soc_pct,
            prod - cons,
            last.mode
        );
        if last.soc_pct < first.soc_pct {
            failures.push(format!(
                "calib seed={seed} : SoC fin {:.2} % < SoC début {:.2} % (attendu fin >= début)",
                last.soc_pct, first.soc_pct
            ));
        }
        if prod - cons <= 0.0 {
            failures.push(format!(
                "calib seed={seed} : bilan {:+.3} Wh (attendu > 0)",
                prod - cons
            ));
        }
        if last.mode != Some(firmware::Mode::Nominal) {
            failures.push(format!(
                "calib seed={seed} : mode fin d'orbite={:?} (attendu Nominal)",
                last.mode
            ));
        }
    }

    // ---------------------------------------------------------------------------------------------
    // (c) déterminisme : même graine, deux vols -> mêmes métriques (bits des flottants compris).
    // ---------------------------------------------------------------------------------------------
    println!("== (c) déterminisme, graine 1 deux fois ==");
    let (s1, r1) = fly(default_cfg(1));
    let (s2, r2) = fly(default_cfg(1));
    let key = |s: &[Snapshot], r: &RunResult| {
        (
            r.resets_by_cause,
            r.boot_count,
            r.min_soc_pct.to_bits(),
            r.temp_min_c.to_bits(),
            r.temp_max_c.to_bits(),
            r.tx_frames,
            r.rx_frames,
            r.ground.beacons.len(),
            r.ground.acks.len(),
            s.last().map(|x| (x.soc_pct.to_bits(), x.temp_c.to_bits())),
        )
    };
    let (k1, k2) = (key(&s1, &r1), key(&s2, &r2));
    println!("determinisme seed=1 : identique={}", k1 == k2);
    if k1 != k2 {
        failures.push(format!(
            "determinisme seed=1 : métriques différentes {k1:?} vs {k2:?}"
        ));
    }

    // ---------------------------------------------------------------------------------------------
    // (d) grille de coins : SoC initial {60, 80, 100} x phase initiale (8 valeurs) = 24 vols, graine 1.
    // Métrique : mode final, SoC final, balises et ACK de la dernière passe entièrement dans le run.
    // ---------------------------------------------------------------------------------------------
    println!("== (d) grille de coins, graine 1 ==");
    let mut coin_fail = 0;
    for soc in SOC_COINS {
        for phase in PHASE_COINS {
            let mut cfg = default_cfg(1);
            cfg.params.soc_init_pct = Some(soc);
            cfg.params.phase_init_s = Some(phase);
            let (snaps, _r) = fly(cfg);
            let last = snaps.last().unwrap();
            let full = passes(&snaps).into_iter().rfind(|p| p.4);
            let (b, a) = match full {
                Some(p) => (p.2.to_string(), p.3.to_string()),
                None => ("na".to_string(), "na".to_string()),
            };
            println!(
                "coin soc={soc} phase={phase} mode_final={:?} soc_final={:.2} balises_derniere_passe={b} acks_derniere_passe={a}",
                last.mode, last.soc_pct
            );
            if last.mode != Some(firmware::Mode::Nominal) {
                coin_fail += 1;
                failures.push(format!(
                    "coin soc={soc} phase={phase} : mode final={:?} (attendu Nominal), SoC final={:.2} %",
                    last.mode, last.soc_pct
                ));
            }
        }
    }
    println!(
        "coins : {} / {} en Nominal",
        24 - coin_fail,
        SOC_COINS.len() * PHASE_COINS.len()
    );

    // ---------------------------------------------------------------------------------------------
    // Verdict.
    // ---------------------------------------------------------------------------------------------
    if failures.is_empty() {
        println!("GATE J1 : 0 échec");
    } else {
        println!("GATE J1 : {} échec(s)", failures.len());
        for f in &failures {
            println!("ECHEC {f}");
        }
        panic!("{} échec(s) : voir ECHEC ci-dessus", failures.len());
    }
}
