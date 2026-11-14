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

#[test]
fn csv_has_beacon_counter_columns() {
    let csv = campaign::csv(&campaign::run_campaign(&opts(2)));
    let mut lines = csv.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    let first = header.iter().position(|c| *c == "cnt_ecc_corr").unwrap();
    let names = [
        "cnt_ecc_corr",
        "cnt_tmr_fix",
        "cnt_tc_rejected",
        "cnt_latchup",
        "fdir_flags",
        "sensor_fault_mask",
        "line_state",
    ];
    assert_eq!(&header[first..first + 7], &names);
    assert_eq!(header[first - 1], "last_mode");
    assert_eq!(header[first + 7], "harness_msg");
    // Les champs `params` peuvent contenir des guillemets : on compte depuis la fin.
    let mut with_beacon = 0;
    for l in lines {
        let f: Vec<&str> = l.rsplitn(9, ',').collect();
        // f[0]=harness_msg, f[1..=7]=compteurs (ordre inversé), f[8]=reste
        let cnt: Vec<&str> = f[1..=7].to_vec();
        let empty = cnt.iter().filter(|c| c.is_empty()).count();
        assert!(empty == 0 || empty == 7, "ligne incohérente : {l}");
        if empty == 0 {
            with_beacon += 1;
        }
    }
    assert!(with_beacon > 0);
}
