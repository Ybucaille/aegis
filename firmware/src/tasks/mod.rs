//! Tâches de l'exécutif cyclique (SPEC §4.1), dans l'ordre d'exécution = priorité.
//!
//! Chaque tâche est une fonction `run(ram, hal)` sans état propre : tout ce qui survit au cycle
//! est dans les cellules de `crate::ram`. Une tâche pose son drapeau d'exécution
//! (`sched::FLAG_*`) en fin de travail ; l'ordonnanceur ne recharge le watchdog que si les 4
//! tâches critiques l'ont posé.

pub mod comms_rx;
pub mod fdir;
pub mod health;
pub mod payload;
pub mod power;
pub mod scrub;
pub mod telemetry;
pub mod thermal;
