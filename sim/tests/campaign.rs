//! T-015 : identité octet à octet de `runs.csv` quel que soit le nombre de fils, rejeu, summary.

use sim::campaign::{self, Options};
use sim::fault::FaultClass;

fn opts(threads: usize) -> Options {
    Options {
        seed: 7,
        runs: 28,
        class: None,
        unprotected: !sim::world::PROTECTED_BUILD,
        threads,
    }
}

#[test]
fn csv_identical_across_thread_counts() {
    let a = campaign::csv(&campaign::run_campaign(&opts(1)));
    let b = campaign::csv(&campaign::run_campaign(&opts(4)));
    assert_eq!(a, b);
    assert_eq!(a.lines().count(), 29);
}

#[test]
fn replay_reproduces_the_csv_line() {
    let o = opts(2);
    let rows = campaign::run_campaign(&o);
    for r in [&rows[3], &rows[13], &rows[20]] {
        let (row, trace) = campaign::replay(&o, r.seed).expect("replay");
        assert_eq!(campaign::csv_line(&row), campaign::csv_line(r));
        assert!(trace.contains(&format!("seed={}", r.seed)));
    }
    assert!(campaign::replay(&o, 12345).is_err());
}

#[test]
fn summary_has_region_breakdown_and_no_e0() {
    let mut o = opts(2);
    o.class = Some(FaultClass::F1);
    o.runs = 12;
    let rows = campaign::run_campaign(&o);
    assert!(rows.iter().all(|r| r.harness.is_none()));
    let s = campaign::summary(&o, &rows, "test", 1.0).to_pretty();
    for k in [
        "par_type_de_region",
        "sans_region_unused",
        "Unused",
        "wilson95",
        "\"variante\"",
    ] {
        assert!(s.contains(k), "{k} absent");
    }
    // classe None : témoin, aucune injection.
    o.class = Some(FaultClass::None);
    let rows = campaign::run_campaign(&o);
    assert!(rows
        .iter()
        .all(|r| r.applied.is_none() && r.t_inj.is_none()));
}
