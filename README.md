# Aegis

*[Version française](README.fr.md)*

**A radiation-tolerant CubeSat firmware, tested against tens of thousands of injected faults — in simulation.**

Aegis is a PC simulation: a `no_std` Rust flight firmware, an environment simulator (orbit, power,
thermal, fault injection), a simulated ground station and a small web dashboard. It answers one
question with numbers: *when radiation corrupts the electronics, does the satellite survive?*

> **This is a simulation.** Firmware, hardware, orbit, power, thermal behaviour and faults are all
> software models written by the same team. No real hardware, no real radiation environment, no
> certification or qualification claim. The survival rates describe a program facing a fault model;
> they do not predict the survival of a spacecraft.

## Headline result

Validation campaign: 20,000 runs, one fault per run, 14 fault classes of ~1,428 runs each, on a seed
never used before (`990274663`). Intervals are 95 % Wilson intervals.

| Fault class | Survival | 95 % CI | Unprotected firmware (control) |
|---|---|---|---|
| 12 of 13 single-fault classes | **100 %** each | [99.73 ; 100] | ≥ 99.9 % each, except the three rows below |
| F3 — register bit flip | 100 % | [99.73 ; 100] | **74.8 %** |
| F4 — latch-up (radiation-induced short circuit) | 100 % | [99.73 ; 100] | **1.1 %** |
| F5 — stuck sensor | 100 % *(after a fix, see below)* | [99.73 ; 100] | **94.2 %** |
| F10 — battery drained during eclipse | **70.9 %** | [68.5 ; 73.2] | 72.0 % (no difference) |
| D1 — dead watchdog + CPU hang (double fault) | **0 %** | [0 ; 0.27] | 0 % |

What this means, honestly:

- **The protections only make a measurable difference for three classes** (F3, F4, F5). For every
  other class the unprotected control survives just as well — the fault is mostly harmless *in this
  model*. A 100 % without a control would have proved nothing.
- **F5 reaches 100 % only after a fix decided by looking at the first campaign's failures.** Before
  the fix: 97.2 %. The bug was the fault-management logic itself: an out-of-range current reading
  was mistaken for a latch-up, the heater got switched off, and the battery froze. The unprotected
  control survived those runs; the protection was killing the satellite.
- **F10 stays broken by design**: during a hardware brown-out the firmware is not running, so nothing
  can heat the battery. The satellite always reboots on its own and becomes commandable again, but the
  battery spends more than 300 s below −10 °C. The thermal model was deliberately *not* recalibrated to
  rescue this class.
- **D1 is an architecture limit**: with the watchdog dead, nothing can restart a hung CPU.
- A 100 % over 1,429 runs does not mean "never fails": the lower bound is 99.73 %.
- An exhaustive RAM sweep (each of the 8,192 bits flipped once, three worlds) found **0 fatal bits with
  protections, 1 fatal + 3 degrading bits without** — which is also why a campaign of ~1,400 runs per
  class cannot tell protected and unprotected apart on RAM faults.

Full report, failure categories, replayable examples and known biases: [`docs/REPORT.md`](docs/REPORT.md)
(in French).

## What "survives" means

Defined in writing **before** the campaign ([`docs/SPEC.md`](docs/SPEC.md) §8). A run lasts three
orbits; the survival oracle lives in the simulator and only reads physical truth, what the ground
received and which commands were executed — never the firmware's internal state.

| | Criterion |
|---|---|
| S1 | No vital hardware loss (radio and battery not destroyed) |
| S2 | Alive and commandable at the end: valid beacon received and a `Ping` acknowledged on the last pass |
| S3 | On-board computer powered and battery state of charge ≥ 20 % at the end |
| S4 | Battery temperature never outside [−10, +50] °C for more than 300 consecutive seconds |
| S5 | Command integrity: only intact ground frames executed, each exactly once; no corrupted telemetry accepted |
| S6 | Stability: at most 2 resets during the last orbit |

## How the firmware defends itself

- **TMR** (triple modular redundancy): critical state (mode, fault counters, last command sequence
  number, isolated power lines, scheduler flags) is stored in three copies, majority-voted and
  rewritten on read.
- **SECDED Hamming (13,8), bit-interleaved**: event log and telemetry buffers. Single-bit errors are
  corrected, double-bit errors detected. Interleaving spreads 8 data bytes over 13 storage bytes so that
  a burst of up to 8 adjacent flipped bits hits each codeword at most once, and is therefore corrected.
- **Memory scrubbing**: the whole image is re-checked in under 60 s.
- **Non-volatile storage**: three copies, each with a CRC-32; the first valid copy repairs the others.
- **Register refresh**: control registers are rewritten every cycle.
- **FDIR** (fault detection, isolation and recovery): sensor plausibility and stuck-value detection,
  latch-up detection with power-line cut-off, degraded modes.
- **Independent hardware watchdog**, kicked only when the FDIR heartbeat is alive.
- **Power modes** nominal / eco / survival, chosen by available energy.
- **Radio protocol** with CRC-32 and sequence numbers: corrupted, truncated or replayed commands are
  silently rejected.

The firmware crate is `#![no_std]`, `#![forbid(unsafe_code)]`, allocation-free, and talks to the
(simulated) hardware only through a `Hal` trait.

## Architecture

```
+---------------------- sim (std) -----------------------+
|  World: orbit, power, thermal, radio channel,          |
|  simulated hardware (registers, NVM, watchdog,         |
|  sensors), fault injector, survival oracle             |
|        |  impl firmware::Hal                           |
|        v                                               |
|  firmware (no_std, lib)  <--bytes-->  ground (lib)     |
+--------------------------------------------------------+
             ^ lib                       ^ lib
        bin `campaign`              dashboard (HTTP bin)
```

| Crate | Content |
|---|---|
| `firmware` | scheduler, modes, FDIR, TMR/ECC, protocol, `Hal` trait |
| `sim` | world model, simulated hardware, faults, oracle, `campaign` binary |
| `ground` | telemetry decoding, automatic operator, command queue |
| `dashboard` | `std::net` HTTP server + embedded static page |

Single process, one thread per run, discrete time (1 tick = 1 simulated second). Everything is
deterministic from a `u64` seed, and every run's seed is logged so any failure can be replayed.
**Zero external dependencies**: PRNG, CRC, JSON and HTTP are hand-written.

## Quick start

Requires Rust (tested with 1.99). Works offline. Run from the repository root.

```sh
# Firmware tests (full local CI: ./scripts/ci.sh)
cargo test --release -p firmware

# Nominal flight, no faults, text output
cargo run --release -p sim --example fly -- --seed 1

# A small fault-injection campaign
cargo run --release -p sim --bin campaign -- --seed 42 --runs 14 --out results/try/run

# Same campaign against the unprotected control firmware
cargo run --release --target-dir target/unprotected -p sim --features unprotected \
  --bin campaign -- --unprotected --seed 42 --runs 14 --out results/try/unprotected

# Replay one run (seed taken from the `seed` column of runs.csv)
cargo run --release -p sim --bin campaign -- --replay 13679457532755275413 --seed 42 --runs 14

# Web dashboard on http://127.0.0.1:8080
cargo run --release -p dashboard -- --port 8080
```

Campaign options: `--class` (`none`, `double`, `f1`..`f13`, `d1`; omitted = mixed), `--threads N`.
Each campaign writes `runs.csv` (one line per run, with its seed and verdict) and `summary.json`
(rates with Wilson intervals, per class). The full campaign is run by `scripts/campaign.sh`.

## Repository map

| Path | What |
|---|---|
| `docs/SPEC.md` | Frozen specification: architecture, interfaces, fault model, survival definition |
| `docs/REPORT.md` | Final survival report |
| `docs/decisions.md` | Decision log, in order |
| `docs/risks.md` | Pre-mortem and risk tracking |
| `docs/triage.md` | Failure analysis after the first campaign |
| `tasks/` | One file per task, with the agent's own report |
| `results/` | Raw campaign outputs cited by the report |

Documentation is in French; code identifiers and this README are in English.

## How it was built

Aegis was built in three days as an experiment in **AI-orchestrated engineering**, with
[Claude Code](https://claude.com/claude-code). A manager agent followed the brief in
[`CLAUDE.md`](CLAUDE.md): a strategist model wrote the spec, the task plan and a pre-mortem; other
agents wrote the low-level firmware, the simulator and tools, and the test campaigns; every task has a
written acceptance criterion and an end-of-day gate that had to be verified by running commands, not
just declared. The `tasks/` and `docs/` folders are the full paper trail, including what failed.

## License

[MIT](LICENSE)
