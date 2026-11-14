//! Modèle de pannes (SPEC §7) : classes F1–F13 et D1, tirage reproductible depuis la graine du run,
//! sérialisation texte stable des paramètres, et fonctions pures de mutation des trames (canal).
//!
//! Une panne = `(classe, paramètres, t_inj)` entièrement tirée d'un PRNG dérivé de la graine du run
//! (`splitmix64(graine ^ SALT)`), indépendant des PRNG du monde. Même graine, même panne.
//! L'application physique est dans `World::inject` (world.rs) : ici, que des données et du pur.

use crate::orbit::Orbit;
use crate::params::Params;
use crate::prng::{splitmix64, Rng};
use crate::world::{FaultEvent, SimConfig};
use firmware::{Line, Reg, RegionKind, SensorId, NVM_SIZE, RAM_MAP, RAM_SIZE};

/// Sel de dérivation du PRNG des pannes (distinct de ceux du monde).
const FAULT_SALT: u64 = 0xFA17_5EED_0000_0014;
/// Fenêtre de `t_inj` (SPEC §7).
pub const T_INJ_MIN: u32 = 300;
pub const T_INJ_MAX: u32 = 6000;
/// Durée de la coupure de l'OBC par le limiteur matériel (F13), en ticks.
pub const OBC_CUT_S: u32 = 10;
/// Une ligne ne s'« efface » du latch-up que coupée au moins ce temps (annexe A / F4).
pub const SEL_CLEAR_OFF_S: u32 = 2;
/// Délai maximal entre `WdgDead` et le `CpuHang` de D1 (s).
const D1_MAX_GAP_S: u64 = 600;
/// Valeurs « hors plage » renvoyées par un capteur défaillant (F5).
const OUT_OF_RANGE: [i16; 3] = [-30_000, 30_000, i16::MAX];

/// Largeur utile (en bits) des registres matériels simulés : seuls ces bits ont un sens
/// physique (un basculement hors largeur serait sans objet, donc flatteur pour le firmware).
pub const fn reg_width_bits(r: Reg) -> u8 {
    match r {
        Reg::LoadSwitches => 3,
        Reg::WdgTimeout => 16,
        Reg::WdgCtrl => 1,
        Reg::RadioCfg => 2,
    }
}

/// Nature d'un défaut de capteur (F5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StuckKind {
    /// Figé à la valeur du dernier échantillon avant l'injection.
    Frozen,
    /// La lecture renvoie `Err`.
    Err,
    /// Renvoie une valeur hors plage.
    OutOfRange(i16),
}

/// Une panne concrète, paramètres inclus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// F1 : 1 bit de la RAM.
    RamSeu { byte: u16, bit: u8 },
    /// F2 : `n` (2 à 4) bits adjacents à partir du bit `byte*8+bit` (indice de bit linéaire).
    RamMbu { byte: u16, bit: u8, n: u8 },
    /// F3 : 1 bit dans un registre matériel.
    RegSeu { reg: Reg, bit: u8 },
    /// F4 : surintensité sur une ligne ; latente tant que la ligne n'est pas alimentée.
    LatchUp { line: Line, extra_ma: u16 },
    /// F5.
    SensorStuck { sensor: SensorId, kind: StuckKind },
    /// F6a : 1 à 8 bits basculés sur la prochaine trame montante (`bits` = entropie brute, 1 par bit).
    TcCorrupt { bits: Vec<u32> },
    /// F6b : réémission d'une trame déjà vue (`pick` choisit laquelle).
    TcReplay { pick: u32 },
    /// F6c : troncature de la prochaine trame montante.
    TcTruncate { frac: u32 },
    /// F7a : reset intempestif du watchdog.
    WdgSpurious,
    /// F7b : 1 bit basculé dans la configuration du watchdog (`WdgTimeout` ou `WdgCtrl`).
    WdgRegHit { reg: Reg, bit: u8 },
    /// F8a : bit dans la région RAM `Fdir`.
    FdirHit { byte: u16, bit: u8 },
    /// F8b : bit dans la région RAM `Sched`.
    SchedHit { byte: u16, bit: u8 },
    /// F9.
    CpuHang,
    /// F10 : état de charge forcé à `soc_x10 / 10` % (0 à 100 -> 0 à 10 %).
    BattDeplete { soc_x10: u16 },
    /// F11 : 1 à 8 bits de la NVM (indices de bit 0..NVM_SIZE*8).
    NvmCorrupt { bits: Vec<u16> },
    /// F12 : bits basculés sur la prochaine trame descendante reçue par le sol.
    TmCorrupt { bits: Vec<u32> },
    /// F13 : surintensité de l'OBC, coupé 10 s par le limiteur matériel.
    ObcLatchUp,
    /// D1 (première moitié) : le watchdog matériel meurt.
    WdgDead,
}

impl Fault {
    /// Étiquette stable de la classe (`F1`..`F13`, `D1` pour `WdgDead`).
    pub fn class(&self) -> FaultClass {
        match self {
            Fault::RamSeu { .. } => FaultClass::F1,
            Fault::RamMbu { .. } => FaultClass::F2,
            Fault::RegSeu { .. } => FaultClass::F3,
            Fault::LatchUp { .. } => FaultClass::F4,
            Fault::SensorStuck { .. } => FaultClass::F5,
            Fault::TcCorrupt { .. } | Fault::TcReplay { .. } | Fault::TcTruncate { .. } => {
                FaultClass::F6
            }
            Fault::WdgSpurious | Fault::WdgRegHit { .. } => FaultClass::F7,
            Fault::FdirHit { .. } | Fault::SchedHit { .. } => FaultClass::F8,
            Fault::CpuHang => FaultClass::F9,
            Fault::BattDeplete { .. } => FaultClass::F10,
            Fault::NvmCorrupt { .. } => FaultClass::F11,
            Fault::TmCorrupt { .. } => FaultClass::F12,
            Fault::ObcLatchUp => FaultClass::F13,
            Fault::WdgDead => FaultClass::D1,
        }
    }

    /// Nom de la variante (SPEC §7).
    pub fn name(&self) -> &'static str {
        match self {
            Fault::RamSeu { .. } => "RamSeu",
            Fault::RamMbu { .. } => "RamMbu",
            Fault::RegSeu { .. } => "RegSeu",
            Fault::LatchUp { .. } => "LatchUp",
            Fault::SensorStuck { .. } => "SensorStuck",
            Fault::TcCorrupt { .. } => "TcCorrupt",
            Fault::TcReplay { .. } => "TcReplay",
            Fault::TcTruncate { .. } => "TcTruncate",
            Fault::WdgSpurious => "WdgSpurious",
            Fault::WdgRegHit { .. } => "WdgRegHit",
            Fault::FdirHit { .. } => "FdirHit",
            Fault::SchedHit { .. } => "SchedHit",
            Fault::CpuHang => "CpuHang",
            Fault::BattDeplete { .. } => "BattDeplete",
            Fault::NvmCorrupt { .. } => "NvmCorrupt",
            Fault::TmCorrupt { .. } => "TmCorrupt",
            Fault::ObcLatchUp => "ObcLatchUp",
            Fault::WdgDead => "WdgDead",
        }
    }

    /// Paramètres en texte stable (champs `clé=valeur` séparés par des espaces, sans virgule ni
    /// point-virgule superflus : utilisable tel quel dans un CSV entre guillemets).
    pub fn params_text(&self) -> String {
        match self {
            Fault::RamSeu { byte, bit }
            | Fault::FdirHit { byte, bit }
            | Fault::SchedHit { byte, bit } => format!("byte={byte} bit={bit}"),
            Fault::RamMbu { byte, bit, n } => format!("byte={byte} bit={bit} n={n}"),
            Fault::RegSeu { reg, bit } | Fault::WdgRegHit { reg, bit } => {
                format!("reg={reg:?} bit={bit}")
            }
            Fault::LatchUp { line, extra_ma } => format!("line={line:?} extra_ma={extra_ma}"),
            Fault::SensorStuck { sensor, kind } => match kind {
                StuckKind::Frozen => format!("sensor={sensor:?} kind=frozen"),
                StuckKind::Err => format!("sensor={sensor:?} kind=err"),
                StuckKind::OutOfRange(v) => format!("sensor={sensor:?} kind=range value={v}"),
            },
            Fault::TcCorrupt { bits } | Fault::TmCorrupt { bits } => {
                format!("nbits={} raw={}", bits.len(), join(bits.iter()))
            }
            Fault::TcReplay { pick } => format!("pick={pick}"),
            Fault::TcTruncate { frac } => format!("frac={frac}"),
            Fault::WdgSpurious | Fault::CpuHang | Fault::ObcLatchUp | Fault::WdgDead => {
                String::new()
            }
            Fault::BattDeplete { soc_x10 } => format!("soc_x10={soc_x10}"),
            Fault::NvmCorrupt { bits } => {
                format!("nbits={} bits={}", bits.len(), join(bits.iter()))
            }
        }
    }
}

fn join<T: std::fmt::Display>(it: impl Iterator<Item = T>) -> String {
    it.map(|x| x.to_string()).collect::<Vec<_>>().join("+")
}

/// Classe de panne de la campagne (argument `--class`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FaultClass {
    /// Témoin sans panne.
    None,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    /// D1 : double panne `WdgDead` puis `CpuHang` (`--class double`).
    D1,
}

impl FaultClass {
    pub const ALL: [FaultClass; 14] = [
        FaultClass::F1,
        FaultClass::F2,
        FaultClass::F3,
        FaultClass::F4,
        FaultClass::F5,
        FaultClass::F6,
        FaultClass::F7,
        FaultClass::F8,
        FaultClass::F9,
        FaultClass::F10,
        FaultClass::F11,
        FaultClass::F12,
        FaultClass::F13,
        FaultClass::D1,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            FaultClass::None => "none",
            FaultClass::F1 => "F1",
            FaultClass::F2 => "F2",
            FaultClass::F3 => "F3",
            FaultClass::F4 => "F4",
            FaultClass::F5 => "F5",
            FaultClass::F6 => "F6",
            FaultClass::F7 => "F7",
            FaultClass::F8 => "F8",
            FaultClass::F9 => "F9",
            FaultClass::F10 => "F10",
            FaultClass::F11 => "F11",
            FaultClass::F12 => "F12",
            FaultClass::F13 => "F13",
            FaultClass::D1 => "D1",
        }
    }

    /// `none`, `double` (= D1), `d1`, `f1`..`f13` (insensible à la casse).
    pub fn parse(s: &str) -> Option<FaultClass> {
        let l = s.to_ascii_lowercase();
        if l == "none" {
            return Some(FaultClass::None);
        }
        if l == "double" {
            return Some(FaultClass::D1);
        }
        FaultClass::ALL
            .into_iter()
            .find(|c| c.label().to_ascii_lowercase() == l)
    }
}

/// Phase orbitale initiale du run : même tirage que `World::new` (SoC puis phase).
pub fn initial_phase(seed: u64, params: &Params) -> u32 {
    let mut r = Rng::new(seed);
    let _ = params.draw_soc_init(&mut r);
    params.draw_phase_init(&mut r)
}

/// Tire la ou les pannes de la classe pour le run `seed`. `None` : liste vide. D1 : deux événements
/// (`WdgDead` à `t_inj`, `CpuHang` entre 1 et 600 s plus tard). Triés par date.
pub fn draw(class: FaultClass, seed: u64, params: &Params) -> Vec<FaultEvent> {
    let mut rng = Rng::new(splitmix64(seed ^ FAULT_SALT));
    let span = (T_INJ_MAX - T_INJ_MIN + 1) as u64;
    let mut t = T_INJ_MIN + rng.below(span) as u32;
    if class == FaultClass::F10 {
        // « pendant une éclipse » : rejet jusqu'à tomber dans l'éclipse, puis balayage déterministe.
        let phase0 = initial_phase(seed, params);
        let orbit_at = |t: u32| Orbit::new(params, phase0 + t % params.orbit_period_s);
        let ok = |t: u32| orbit_at(t).in_eclipse();
        let mut tries = 0;
        while !ok(t) && tries < 64 {
            t = T_INJ_MIN + rng.below(span) as u32;
            tries += 1;
        }
        while !ok(t) {
            t = if t >= T_INJ_MAX { T_INJ_MIN } else { t + 1 };
        }
    }
    let ev = |t: u32, fault: Fault| FaultEvent { t, fault };
    match class {
        FaultClass::None => Vec::new(),
        FaultClass::F1 => {
            let (byte, bit) = ram_pos(&mut rng, 0, RAM_SIZE);
            vec![ev(t, Fault::RamSeu { byte, bit })]
        }
        FaultClass::F2 => {
            let n = 2 + rng.below(3) as u8;
            let start = rng.below((RAM_SIZE * 8) as u64 - n as u64 + 1) as usize;
            vec![ev(
                t,
                Fault::RamMbu {
                    byte: (start / 8) as u16,
                    bit: (start % 8) as u8,
                    n,
                },
            )]
        }
        FaultClass::F3 => {
            let reg = Reg::ALL[rng.below(4) as usize];
            let bit = rng.below(reg_width_bits(reg) as u64) as u8;
            vec![ev(t, Fault::RegSeu { reg, bit })]
        }
        FaultClass::F4 => {
            let line = Line::ALL[rng.below(3) as usize];
            let lo = 300u64;
            let extra_ma = (lo + rng.below(800 - lo + 1)) as u16;
            vec![ev(t, Fault::LatchUp { line, extra_ma })]
        }
        FaultClass::F5 => {
            let sensor = SensorId::ALL[rng.below(9) as usize];
            let kind = match rng.below(3) {
                0 => StuckKind::Frozen,
                1 => StuckKind::Err,
                _ => StuckKind::OutOfRange(OUT_OF_RANGE[rng.below(3) as usize]),
            };
            vec![ev(t, Fault::SensorStuck { sensor, kind })]
        }
        FaultClass::F6 => match rng.below(3) {
            0 => {
                let n = 1 + rng.below(8) as usize;
                let bits = (0..n).map(|_| rng.next_u64() as u32).collect();
                vec![ev(t, Fault::TcCorrupt { bits })]
            }
            1 => vec![ev(
                t,
                Fault::TcReplay {
                    pick: rng.next_u64() as u32,
                },
            )],
            _ => vec![ev(
                t,
                Fault::TcTruncate {
                    frac: rng.next_u64() as u32,
                },
            )],
        },
        FaultClass::F7 => {
            if rng.below(2) == 0 {
                vec![ev(t, Fault::WdgSpurious)]
            } else {
                let reg = if rng.below(2) == 0 {
                    Reg::WdgTimeout
                } else {
                    Reg::WdgCtrl
                };
                let bit = rng.below(reg_width_bits(reg) as u64) as u8;
                vec![ev(t, Fault::WdgRegHit { reg, bit })]
            }
        }
        FaultClass::F8 => {
            let sched = rng.below(2) == 0;
            let kind = if sched {
                RegionKind::Sched
            } else {
                RegionKind::Fdir
            };
            let r = RAM_MAP
                .iter()
                .find(|r| r.kind == kind)
                .expect("région Sched/Fdir absente de RAM_MAP");
            let (byte, bit) = ram_pos(&mut rng, r.start, r.len);
            vec![ev(
                t,
                if sched {
                    Fault::SchedHit { byte, bit }
                } else {
                    Fault::FdirHit { byte, bit }
                },
            )]
        }
        FaultClass::F9 => vec![ev(t, Fault::CpuHang)],
        FaultClass::F10 => vec![ev(
            t,
            Fault::BattDeplete {
                soc_x10: rng.below(101) as u16,
            },
        )],
        FaultClass::F11 => {
            let n = 1 + rng.below(8) as usize;
            let total = (NVM_SIZE * 8) as u64;
            let mut bits: Vec<u16> = Vec::new();
            while bits.len() < n {
                let b = rng.below(total) as u16;
                if !bits.contains(&b) {
                    bits.push(b);
                }
            }
            vec![ev(t, Fault::NvmCorrupt { bits })]
        }
        FaultClass::F12 => {
            let n = 1 + rng.below(8) as usize;
            let bits = (0..n).map(|_| rng.next_u64() as u32).collect();
            vec![ev(t, Fault::TmCorrupt { bits })]
        }
        FaultClass::F13 => vec![ev(t, Fault::ObcLatchUp)],
        FaultClass::D1 => {
            let gap = 1 + rng.below(D1_MAX_GAP_S) as u32;
            vec![ev(t, Fault::WdgDead), ev(t + gap, Fault::CpuHang)]
        }
    }
}

/// Octet uniforme dans `[start, start+len)` et bit uniforme.
fn ram_pos(rng: &mut Rng, start: usize, len: usize) -> (u16, u8) {
    let byte = start + rng.below(len as u64) as usize;
    (byte as u16, rng.below(8) as u8)
}

/// Configuration d'un run de campagne pour une classe : durée standard, graine du run, pannes tirées.
pub fn sim_config(class: FaultClass, seed: u64) -> SimConfig {
    let mut cfg = SimConfig {
        seed,
        ..SimConfig::default()
    };
    cfg.faults = draw(class, seed, &cfg.params);
    cfg
}

// ---------------------------------------------------------------------------------------------
// Mutation de trames (canal) : pures, testables seules
// ---------------------------------------------------------------------------------------------

/// Positions de bits distinctes `< nbits` dérivées de l'entropie brute (collision : bit suivant).
pub fn distinct_bit_positions(raw: &[u32], nbits: usize) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    if nbits == 0 {
        return out;
    }
    for r in raw.iter().take(nbits) {
        let mut p = *r as usize % nbits;
        while out.contains(&p) {
            p = (p + 1) % nbits;
        }
        out.push(p);
    }
    out
}

/// Bascule les bits dérivés de `raw` ; renvoie la trame altérée et les positions touchées.
pub fn flip_frame_bits(frame: &[u8], raw: &[u32]) -> (Vec<u8>, Vec<usize>) {
    let pos = distinct_bit_positions(raw, frame.len() * 8);
    let mut out = frame.to_vec();
    for p in &pos {
        out[p / 8] ^= 1 << (p % 8);
    }
    (out, pos)
}

/// Troncature : conserve `1 + frac % (len-1)` octets (au moins 1 octet retiré si `len >= 2`).
pub fn truncate_frame(frame: &[u8], frac: u32) -> Vec<u8> {
    if frame.len() < 2 {
        return frame.to_vec();
    }
    let keep = 1 + frac as usize % (frame.len() - 1);
    frame[..keep].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_classes() {
        assert_eq!(FaultClass::parse("none"), Some(FaultClass::None));
        assert_eq!(FaultClass::parse("double"), Some(FaultClass::D1));
        assert_eq!(FaultClass::parse("f10"), Some(FaultClass::F10));
        assert_eq!(FaultClass::parse("F1"), Some(FaultClass::F1));
        assert_eq!(FaultClass::parse("F14"), None);
        for c in FaultClass::ALL {
            assert_eq!(FaultClass::parse(c.label()), Some(c));
        }
    }

    #[test]
    fn flip_changes_exactly_the_requested_bits() {
        let f = [0u8; 12];
        let (g, pos) = flip_frame_bits(&f, &[3, 3, 3, 500]);
        assert_eq!(pos.len(), 4);
        let ones: u32 = g.iter().map(|b| b.count_ones()).sum();
        assert_eq!(ones, 4, "positions distinctes malgré les collisions");
    }

    #[test]
    fn truncate_always_shortens_and_keeps_one_byte() {
        for frac in [0u32, 1, 7, 1000, u32::MAX] {
            let t = truncate_frame(&[1, 2, 3, 4, 5], frac);
            assert!(!t.is_empty() && t.len() < 5);
        }
        assert_eq!(truncate_frame(&[9], 3), vec![9]);
    }

    #[test]
    fn draw_is_deterministic_and_in_window() {
        let p = Params::default();
        for c in FaultClass::ALL {
            for seed in 1..=30u64 {
                let a = draw(c, seed, &p);
                assert_eq!(a, draw(c, seed, &p), "{c:?} graine {seed}");
                assert!(!a.is_empty());
                assert!(a[0].t >= T_INJ_MIN && a[0].t <= T_INJ_MAX);
                assert_eq!(a[0].fault.class(), c);
            }
        }
        assert!(draw(FaultClass::None, 1, &p).is_empty());
        assert_ne!(
            draw(FaultClass::F1, 1, &p),
            draw(FaultClass::F1, 2, &p),
            "deux graines, deux pannes"
        );
    }

    #[test]
    fn f10_is_drawn_during_an_eclipse() {
        let p = Params::default();
        for seed in 1..=300u64 {
            let ev = draw(FaultClass::F10, seed, &p);
            let phase = (initial_phase(seed, &p) + ev[0].t) % p.orbit_period_s;
            assert!(phase >= p.orbit_period_s - p.eclipse_s, "graine {seed}");
            let Fault::BattDeplete { soc_x10 } = ev[0].fault else {
                panic!()
            };
            assert!(soc_x10 <= 100);
        }
    }

    #[test]
    fn d1_is_two_events_wdg_dead_then_hang() {
        let ev = draw(FaultClass::D1, 5, &Params::default());
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].fault, Fault::WdgDead);
        assert_eq!(ev[1].fault, Fault::CpuHang);
        assert!(ev[1].t > ev[0].t);
    }

    #[test]
    fn f1_ten_thousand_draws_cover_every_ram_region() {
        let p = Params::default();
        let mut seen = vec![false; RAM_MAP.len()];
        for seed in 0..10_000u64 {
            let Fault::RamSeu { byte, bit } = draw(FaultClass::F1, seed, &p)[0].fault else {
                panic!()
            };
            assert!(bit < 8 && (byte as usize) < RAM_SIZE);
            let r = RAM_MAP
                .iter()
                .position(|r| (byte as usize) >= r.start && (byte as usize) < r.start + r.len)
                .unwrap();
            seen[r] = true;
        }
        for (i, s) in seen.iter().enumerate() {
            assert!(*s, "région {} jamais tirée", RAM_MAP[i].name);
        }
    }

    #[test]
    fn params_text_is_stable() {
        assert_eq!(
            Fault::RamSeu { byte: 12, bit: 3 }.params_text(),
            "byte=12 bit=3"
        );
        assert_eq!(
            Fault::LatchUp {
                line: Line::Heater,
                extra_ma: 450
            }
            .params_text(),
            "line=Heater extra_ma=450"
        );
        assert_eq!(Fault::WdgDead.params_text(), "");
    }
}
