//! Vol nominal de `T_RUN` (3 orbites), sans panne, graine fixe : le satellite doit rester nominal,
//! commandable à chaque passe, et redémarrer seul d'un démarrage à froid (T-008, PRE-MORTEM R1/R2).
//! Les `println!` (à lire avec `-- --nocapture`) sont des MESURES sans assertion.

use sim::world::{Cpu, SimConfig, Snapshot, World};

fn fly(cfg: SimConfig) -> (Vec<Snapshot>, sim::world::RunResult) {
    let n = cfg.duration_s;
    let mut w = World::new(cfg);
    let snaps: Vec<Snapshot> = (0..n).map(|_| w.step()).collect();
    (snaps, w.finish())
}

/// Passes `(début, fin exclue, balises reçues, ACK reçus, contenue entièrement dans le run)`.
fn passes(snaps: &[Snapshot]) -> Vec<(u32, u32, u32, u32, bool)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < snaps.len() {
        if snaps[i].in_pass {
            let start = i;
            while i < snaps.len() && snaps[i].in_pass {
                i += 1;
            }
            let b0 = if start == 0 {
                0
            } else {
                snaps[start - 1].beacons_rx
            };
            let a0 = if start == 0 {
                0
            } else {
                snaps[start - 1].acks_rx
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

#[test]
fn nominal_flight_seed1() {
    let cfg = SimConfig {
        seed: 1,
        ..SimConfig::default()
    };
    assert_eq!(cfg.duration_s, 17_100);
    let (snaps, r) = fly(cfg);
    let last = snaps.last().unwrap();

    assert_eq!(r.resets_by_cause, [0; 6], "aucun reset");
    assert_eq!(r.boot_count, 1);
    let ps = passes(&snaps);
    let full: Vec<_> = ps.iter().filter(|p| p.4).collect();
    assert!(full.len() >= 2, "passes entières : {ps:?}");
    for p in &full {
        assert!(p.2 >= 1, "au moins une balise par passe : {p:?}");
        assert!(p.3 >= 1, "au moins un Ping acquitté par passe : {p:?}");
    }
    // Dernière balise reçue : mode Nominal (Mode::Nominal = 2).
    let (_, mode, _) = last.last_beacon.expect("aucune balise");
    assert_eq!(mode, firmware::Mode::Nominal as u8);
    assert_eq!(last.mode, Some(firmware::Mode::Nominal));
    assert!(last.soc_pct >= 20.0, "SoC final {}", last.soc_pct);
    assert!(
        r.temp_min_c >= -10.0 && r.temp_max_c <= 50.0,
        "T batterie [{}, {}]",
        r.temp_min_c,
        r.temp_max_c
    );
    assert_eq!(
        r.ground.rejects, 0,
        "aucune trame rejetée par le sol sans panne"
    );
    assert!(last.obc_powered && last.cpu == Cpu::Running);

    // --- Mesures (sans assertion) ---
    println!("MESURES graine 1 : passes (debut, fin, balises, acks, entiere) = {ps:?}");
    println!(
        "SoC min {:.1} %, final {:.1} % ; T [{:.1}, {:.1}] C ; balises {} acks {}",
        r.min_soc_pct,
        last.soc_pct,
        r.temp_min_c,
        r.temp_max_c,
        r.ground.beacons.len(),
        r.ground.acks.len()
    );
    measure(&snaps);
}

/// Mesures R1 : réchauffeur en éclipse, SoC début/fin d'éclipse, bilan Wh par orbite.
fn measure(snaps: &[Snapshot]) {
    let mut i = 0;
    let mut k = 0;
    while i < snaps.len() {
        if snaps[i].in_eclipse {
            let s = i;
            while i < snaps.len() && snaps[i].in_eclipse {
                i += 1;
            }
            let heater = snaps[s..i].iter().filter(|x| x.lines & 0b100 != 0).count();
            let tmin = snaps[s..i]
                .iter()
                .map(|x| x.temp_c)
                .fold(f64::MAX, f64::min);
            println!(
                "eclipse {k} [{s},{i}) entiere={} : SoC {:.1} -> {:.1} %, rapport cyclique rechauffeur {:.1} %, T_min {:.1} C",
                s > 0 && i < snaps.len(),
                snaps[s].soc_pct,
                snaps[i - 1].soc_pct,
                100.0 * heater as f64 / (i - s) as f64,
                tmin
            );
            k += 1;
        } else {
            i += 1;
        }
    }
    // Bilan par fenêtre de 5700 s (pas de saturation visible si SoC < 100 % ; sinon le surplus est perdu).
    for (n, w) in snaps.chunks(5700).enumerate() {
        let prod: f64 = w.iter().map(|x| x.solar_w).sum::<f64>() / 3600.0;
        let cons: f64 = w.iter().map(|x| x.load_w).sum::<f64>() / 3600.0;
        println!(
            "orbite {n} : produit {prod:.2} Wh, consomme {cons:.2} Wh, bilan {:+.2} Wh, SoC {:.1} -> {:.1} %",
            prod - cons,
            w[0].soc_pct,
            w[w.len() - 1].soc_pct
        );
    }
}

#[test]
fn cold_start_recovers_alone() {
    // SoC 3 % en début d'éclipse : l'OBC est coupé, il redémarre seul au soleil (SoC > 15 %).
    let mut cfg = SimConfig {
        seed: 1,
        ..SimConfig::default()
    };
    cfg.params.soc_init_pct = Some(3.0);
    cfg.params.phase_init_s = Some(cfg.params.orbit_period_s - cfg.params.eclipse_s);
    let (snaps, r) = fly(cfg);

    let first_on = snaps
        .iter()
        .position(|s| s.obc_powered)
        .expect("l'OBC ne redémarre jamais");
    assert!(first_on > 2000, "coupé pendant l'éclipse : t={first_on}");
    assert!(snaps[..first_on].iter().all(|s| s.beacons_rx == 0));
    assert_eq!(
        r.resets_by_cause[firmware::ResetCause::Brownout as usize],
        1
    );
    // Une balise est reçue à la passe suivant le redémarrage.
    let ps = passes(&snaps);
    let after = ps
        .iter()
        .find(|p| p.0 as usize > first_on)
        .expect("pas de passe après le redémarrage");
    assert!(
        after.2 >= 1 && after.3 >= 1,
        "passe après redémarrage : {after:?}"
    );
    assert!(snaps.last().unwrap().obc_powered);

    // Mesures R2 : secondes consécutives sous -10 C, OBC éteint.
    let (mut run, mut best) = (0u32, 0u32);
    let (mut run_any, mut best_any) = (0u32, 0u32);
    for s in &snaps {
        if s.temp_c < -10.0 {
            run_any += 1;
            best_any = best_any.max(run_any);
            if !s.obc_powered {
                run += 1;
                best = best.max(run);
            } else {
                run = 0;
            }
        } else {
            run = 0;
            run_any = 0;
        }
    }
    let tmin = snaps.iter().map(|s| s.temp_c).fold(f64::MAX, f64::min);
    println!(
        "MESURES demarrage a froid : OBC rallume a t={first_on} ; T_min {tmin:.1} C ; secondes consecutives < -10 C : {best_any} (dont OBC eteint : {best}) -> S4 (300 s) {}",
        if best_any > 300 { "VIOLE" } else { "respecte" }
    );
    println!(
        "passes apres redemarrage : {:?}",
        ps.iter()
            .filter(|p| p.0 as usize > first_on)
            .collect::<Vec<_>>()
    );
}
