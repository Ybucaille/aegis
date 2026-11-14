//! T-018 : déterminisme de la campagne (SPEC §7.1).
//! Deux campagnes de même graine (200 runs) produisent un `runs.csv` identique octet pour octet,
//! et 1 fil contre 4 fils donnent le même résultat.

use sim::campaign::{self, Options};

fn opts(seed: u64, threads: usize) -> Options {
    Options {
        seed,
        runs: 200,
        class: None,
        unprotected: !sim::world::PROTECTED_BUILD,
        threads,
    }
}

#[test]
fn same_seed_twice_gives_identical_csv() {
    let a = campaign::csv(&campaign::run_campaign(&opts(20260, 4)));
    let b = campaign::csv(&campaign::run_campaign(&opts(20260, 4)));
    assert_eq!(a.lines().count(), 201, "en-tête + 200 lignes attendues");
    assert_eq!(a.as_bytes(), b.as_bytes());
}

#[test]
fn one_thread_equals_four_threads() {
    let a = campaign::csv(&campaign::run_campaign(&opts(20260, 1)));
    let b = campaign::csv(&campaign::run_campaign(&opts(20260, 4)));
    assert_eq!(a.as_bytes(), b.as_bytes());
}
