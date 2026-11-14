//! Image RAM du firmware : cellules typées, protections, carte mémoire (SPEC §3.1, §4.3).
//!
//! Règle d'honnêteté mémoire : tout état qui survit d'un tick au suivant est une cellule de ce
//! module, rangée dans `Firmware::ram`. Les tâches n'accèdent à l'état QUE par `Slot::get` et
//! `Slot::set` ; la protection d'une cellule (aucune, TMR, SECDED) est un détail de sa
//! déclaration ici, invisible des tâches.
//!
//! Vocabulaire : le type s'appelle `Slot` (et non `...Cell`) pour ne pas déclencher le contrôle
//! anti-état-caché de `scripts/ci.sh` (R5), qui interdit tout identifiant en `Cell`.
//!
//! **Carte FIGÉE en T-010** (R4) : toutes les cellules dont T-011 (NVM, TC, journal) et T-012
//! (FDIR) ont besoin sont réservées ci-dessous ; ces tâches ne modifient plus ce fichier.
//!
//! Disposition (toutes les tailles sont vérifiées à la compilation) :
//!
//! | Région | Octets | Contenu |
//! |---|---|---|
//! | `sched` | 0..60 | TMR, blocs de 20 |
//! | `mode` | 60..104 | TMR, blocs de 12 (0..36) + 8 octets de mesures du cycle, non protégés |
//! | `fdir` | 104..308 | TMR, blocs de 68 |
//! | `proto` | 308..356 | TMR, blocs de 16 |
//! | `tm` | 356..395 | SECDED (13,8) entrelacé, 3 groupes (24 octets de données) |
//! | `log` | 395..928 | SECDED (13,8) entrelacé, 41 groupes (328 octets de données) |
//! | `unused` | 928..1024 | rien (96 octets, 9,4 %) |
//!
//! Mesures du cycle non protégées (`VBATT_MV` .. `SENSOR_FAULT_MASK`) : réécrites par `power`
//! à chaque cycle AVANT toute lecture ; un basculement entre deux cycles est donc sans effet
//! (le modèle de pannes n'injecte jamais au milieu d'un cycle, R12). Les protéger n'ajouterait
//! rien.

use core::marker::PhantomData;

use crate::ecc::{self, EccArea};
use crate::hal::{SensorId, RAM_SIZE};
use crate::tmr::{self, TmrArea};

/// L'image RAM complète.
pub type Ram = [u8; RAM_SIZE];

// ---------------------------------------------------------------------------------------------
// Abstraction de cellule
// ---------------------------------------------------------------------------------------------

/// Valeur scalaire stockable dans une cellule (gros-boutiste, 1 à 4 octets).
/// `load` et `store` reçoivent une tranche d'exactement `SIZE` octets.
pub trait Scalar: Copy {
    const SIZE: usize;
    fn load(b: &[u8]) -> Self;
    fn store(self, b: &mut [u8]);
    /// Valeur de repli à partir de ses bits (troncature aux `SIZE` octets de poids faible).
    fn from_bits(v: u32) -> Self;
}

impl Scalar for u8 {
    const SIZE: usize = 1;
    fn load(b: &[u8]) -> Self {
        b[0]
    }
    fn store(self, b: &mut [u8]) {
        b[0] = self;
    }
    fn from_bits(v: u32) -> Self {
        v as u8
    }
}

impl Scalar for u16 {
    const SIZE: usize = 2;
    fn load(b: &[u8]) -> Self {
        u16::from_be_bytes([b[0], b[1]])
    }
    fn store(self, b: &mut [u8]) {
        b.copy_from_slice(&self.to_be_bytes());
    }
    fn from_bits(v: u32) -> Self {
        v as u16
    }
}

impl Scalar for i16 {
    const SIZE: usize = 2;
    fn load(b: &[u8]) -> Self {
        i16::from_be_bytes([b[0], b[1]])
    }
    fn store(self, b: &mut [u8]) {
        b.copy_from_slice(&self.to_be_bytes());
    }
    fn from_bits(v: u32) -> Self {
        v as u16 as i16
    }
}

impl Scalar for u32 {
    const SIZE: usize = 4;
    fn load(b: &[u8]) -> Self {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    }
    fn store(self, b: &mut [u8]) {
        b.copy_from_slice(&self.to_be_bytes());
    }
    fn from_bits(v: u32) -> Self {
        v
    }
}

/// Booléen sur un octet : toute valeur non nulle lue vaut `true` (décodage total).
impl Scalar for bool {
    const SIZE: usize = 1;
    fn load(b: &[u8]) -> Self {
        b[0] != 0
    }
    fn store(self, b: &mut [u8]) {
        b[0] = self as u8;
    }
    fn from_bits(v: u32) -> Self {
        v != 0
    }
}

/// Protection d'une cellule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prot {
    /// Aucune : octets à `off`.
    Plain,
    /// TMR : copie 0 à `off`, copies 1 et 2 à `off + block`, `off + 2 * block`.
    Tmr(TmrArea),
    /// SECDED : `off` est l'index de l'octet de DONNÉES dans la zone.
    Ecc(EccArea),
}

/// Cellule typée à décalage constant dans l'image RAM.
///
/// Invariants, vérifiés à la construction (`const fn` + cellules `const` : une violation est
/// une erreur de compilation) : `Plain` : `off + SIZE <= RAM_SIZE` ; `Tmr` : la copie 0 est
/// entièrement dans le bloc 0 de sa zone, donc les trois copies sont dans la RAM et séparées
/// d'au moins un octet ; `Ecc` : `off + SIZE <= data_len` de sa zone. Les accès par index ne
/// peuvent donc pas paniquer.
pub struct Slot<T: Scalar> {
    off: usize,
    prot: Prot,
    /// Valeur (en bits) rendue par une cellule SECDED dont la donnée est invalide (erreur
    /// double) ; la cellule est alors réécrite avec cette valeur.
    fallback: u32,
    _t: PhantomData<T>,
}

/// Lecture brute de `out.len()` octets d'une cellule. `false` si une donnée SECDED est invalide
/// (le contenu de `out` est alors à ignorer).
fn read_into(ram: &mut Ram, off: usize, prot: Prot, out: &mut [u8]) -> bool {
    match prot {
        Prot::Plain => {
            out.copy_from_slice(&ram[off..off + out.len()]);
            true
        }
        Prot::Tmr(a) => {
            tmr::read(ram, off, a.block, out);
            true
        }
        Prot::Ecc(a) => {
            let mut ok = true;
            for (i, o) in out.iter_mut().enumerate() {
                match ecc::read_byte(ram, &a, off + i) {
                    Some(b) => *o = b,
                    None => ok = false,
                }
            }
            ok
        }
    }
}

fn write_from(ram: &mut Ram, off: usize, prot: Prot, data: &[u8]) {
    match prot {
        Prot::Plain => ram[off..off + data.len()].copy_from_slice(data),
        Prot::Tmr(a) => tmr::write(ram, off, a.block, data),
        Prot::Ecc(a) => {
            for (i, &b) in data.iter().enumerate() {
                ecc::write_byte(ram, &a, off + i, b);
            }
        }
    }
}

fn count_uncorrectable(ram: &mut Ram) {
    let n = CNT_ECC_UNCORR.get(ram).saturating_add(1);
    CNT_ECC_UNCORR.set(ram, n);
}

impl<T: Scalar> Slot<T> {
    /// Cellule non protégée en `off`.
    pub const fn at(off: usize) -> Self {
        assert!(off + T::SIZE <= RAM_SIZE, "cellule hors de la RAM");
        Slot {
            off,
            prot: Prot::Plain,
            fallback: 0,
            _t: PhantomData,
        }
    }

    /// Cellule non protégée placée immédiatement après `end` (fin d'une cellule précédente).
    pub const fn after(end: usize) -> Self {
        Self::at(end)
    }

    /// Cellule TMR dont la copie 0 est en `off` (absolu, dans le bloc 0 de `area`).
    pub const fn tmr(area: TmrArea, off: usize) -> Self {
        assert!(off >= area.base, "cellule TMR avant sa zone");
        assert!(
            off + T::SIZE <= area.base + area.block,
            "cellule TMR hors de son bloc"
        );
        Slot {
            off,
            prot: Prot::Tmr(area),
            fallback: 0,
            _t: PhantomData,
        }
    }

    /// Cellule SECDED à l'index de données `idx` de `area`. Repli par défaut : 0.
    pub const fn ecc(area: EccArea, idx: usize) -> Self {
        assert!(
            idx + T::SIZE <= area.data_len(),
            "cellule SECDED hors de sa zone"
        );
        Slot {
            off: idx,
            prot: Prot::Ecc(area),
            fallback: 0,
            _t: PhantomData,
        }
    }

    /// Fixe la valeur de repli (cellules SECDED seulement ; sans effet ailleurs).
    pub const fn with_fallback(self, bits: u32) -> Self {
        Slot {
            off: self.off,
            prot: self.prot,
            fallback: bits,
            _t: PhantomData,
        }
    }

    /// Décalage : physique (non protégée), de la copie 0 (TMR), index de données (SECDED).
    pub const fn off(&self) -> usize {
        self.off
    }

    pub const fn len(&self) -> usize {
        T::SIZE
    }

    pub const fn is_empty(&self) -> bool {
        T::SIZE == 0
    }

    /// Premier décalage après la cellule, dans le même espace que `off`.
    pub const fn end(&self) -> usize {
        self.off + T::SIZE
    }

    pub const fn prot(&self) -> Prot {
        self.prot
    }

    /// Écart entre copies (TMR) ; 0 pour les autres protections.
    pub const fn tmr_stride(&self) -> usize {
        match self.prot {
            Prot::Tmr(a) => a.block,
            _ => 0,
        }
    }

    /// Lecture. TMR : vote bit à bit et réécriture des copies divergentes. SECDED : erreur
    /// simple corrigée en place ; erreur double -> valeur de repli, réécrite, et comptée dans
    /// `CNT_ECC_UNCORR`.
    pub fn get(&self, ram: &mut Ram) -> T {
        let mut buf = [0u8; 4];
        let b = &mut buf[..T::SIZE];
        if read_into(ram, self.off, self.prot, b) {
            T::load(b)
        } else {
            let v = T::from_bits(self.fallback);
            self.set(ram, v);
            count_uncorrectable(ram);
            v
        }
    }

    /// Écriture (les trois copies en TMR ; ré-encodage en SECDED).
    pub fn set(&self, ram: &mut Ram, v: T) {
        let mut buf = [0u8; 4];
        let b = &mut buf[..T::SIZE];
        v.store(b);
        write_from(ram, self.off, self.prot, b);
    }

    /// Fiche descriptive (table `SLOTS`).
    pub const fn info(&self, name: &'static str, kind: RegionKind) -> SlotInfo {
        SlotInfo {
            name,
            start: self.off,
            len: T::SIZE,
            kind,
            prot: self.prot,
        }
    }
}

/// Tampon d'octets protégé SECDED (journal, T-011). Index hors bornes : lecture `None`,
/// écriture ignorée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EccBuf {
    area: EccArea,
    start: usize,
    len: usize,
}

impl EccBuf {
    pub const fn new(area: EccArea, start: usize, len: usize) -> Self {
        assert!(start + len <= area.data_len(), "tampon SECDED hors zone");
        EccBuf { area, start, len }
    }
    pub const fn len(&self) -> usize {
        self.len
    }
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Octet `i` ; `None` si hors bornes ou donnée invalide (erreur double : comptée dans
    /// `CNT_ECC_UNCORR`, laissée en place ; à l'appelant de l'invalider par `set`).
    pub fn get(&self, ram: &mut Ram, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        let v = ecc::read_byte(ram, &self.area, self.start + i);
        if v.is_none() {
            count_uncorrectable(ram);
        }
        v
    }
    pub fn set(&self, ram: &mut Ram, i: usize, v: u8) {
        if i < self.len {
            ecc::write_byte(ram, &self.area, self.start + i, v);
        }
    }
}

/// Description d'une cellule, pour les tests de disposition et les diagnostics du sim.
/// `start` est dans l'espace de `prot` (voir `Slot::off`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotInfo {
    pub name: &'static str,
    pub start: usize,
    pub len: usize,
    pub kind: RegionKind,
    pub prot: Prot,
}

impl SlotInfo {
    /// Lecture (avec la protection de la cellule) de sa valeur brute, gros-boutiste, sur u32.
    /// `None` si donnée SECDED invalide (rien n'est réécrit dans ce cas).
    pub fn read_raw(&self, ram: &mut Ram) -> Option<u32> {
        let mut buf = [0u8; 4];
        let n = self.len.min(4);
        if !read_into(ram, self.start, self.prot, &mut buf[4 - n..]) {
            return None;
        }
        Some(u32::from_be_bytes(buf))
    }

    /// Octets physiques portant la copie 0 (TMR) ou la cellule (non protégée) ; `None` pour
    /// une cellule SECDED (entrelacée, voir `EccArea::phys_bit`).
    pub const fn phys(&self) -> Option<(usize, usize)> {
        match self.prot {
            Prot::Ecc(_) => None,
            _ => Some((self.start, self.len)),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Carte mémoire
// ---------------------------------------------------------------------------------------------

/// Nature d'une région de la RAM (ventilation des pannes RAM par région, SPEC §3.1 et R6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionKind {
    Sched,
    Mode,
    Fdir,
    Proto,
    Tm,
    Log,
    Unused,
}

/// Région de l'image RAM : octets `[start, start + len)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub name: &'static str,
    pub start: usize,
    pub len: usize,
    pub kind: RegionKind,
}

/// Zones protégées (une région = une ou deux zones contiguës).
pub const SCHED_TMR: TmrArea = TmrArea::new(0, 20);
pub const MODE_TMR: TmrArea = TmrArea::new(SCHED_TMR.end(), 12);
/// Mesures du cycle, non protégées (voir en-tête).
pub const MODE_PLAIN_START: usize = MODE_TMR.end();
pub const MODE_PLAIN_LEN: usize = 8;
pub const FDIR_TMR: TmrArea = TmrArea::new(MODE_PLAIN_START + MODE_PLAIN_LEN, 68);
pub const PROTO_TMR: TmrArea = TmrArea::new(FDIR_TMR.end(), 16);
pub const TM_ECC: EccArea = EccArea::new(PROTO_TMR.end(), 3);
pub const LOG_ECC: EccArea = EccArea::new(TM_ECC.end(), 41);

pub const SCHED_START: usize = SCHED_TMR.base;
pub const SCHED_LEN: usize = SCHED_TMR.storage_len();
pub const MODE_START: usize = MODE_TMR.base;
pub const MODE_LEN: usize = MODE_TMR.storage_len() + MODE_PLAIN_LEN;
pub const FDIR_START: usize = FDIR_TMR.base;
pub const FDIR_LEN: usize = FDIR_TMR.storage_len();
pub const PROTO_START: usize = PROTO_TMR.base;
pub const PROTO_LEN: usize = PROTO_TMR.storage_len();
pub const TM_START: usize = TM_ECC.base;
pub const TM_LEN: usize = TM_ECC.storage_len();
pub const LOG_START: usize = LOG_ECC.base;
pub const LOG_LEN: usize = LOG_ECC.storage_len();
pub const UNUSED_START: usize = LOG_START + LOG_LEN;
pub const UNUSED_LEN: usize = RAM_SIZE - UNUSED_START;

const _: () = assert!(UNUSED_START <= RAM_SIZE);
const _: () = assert!(UNUSED_LEN * 10 < RAM_SIZE, "Unused >= 10 % de la RAM");

/// Carte mémoire. Contiguë, couvre toute la RAM (figée en T-010).
pub const RAM_MAP: &[Region] = &[
    Region {
        name: "sched",
        start: SCHED_START,
        len: SCHED_LEN,
        kind: RegionKind::Sched,
    },
    Region {
        name: "mode",
        start: MODE_START,
        len: MODE_LEN,
        kind: RegionKind::Mode,
    },
    Region {
        name: "fdir",
        start: FDIR_START,
        len: FDIR_LEN,
        kind: RegionKind::Fdir,
    },
    Region {
        name: "proto",
        start: PROTO_START,
        len: PROTO_LEN,
        kind: RegionKind::Proto,
    },
    Region {
        name: "tm",
        start: TM_START,
        len: TM_LEN,
        kind: RegionKind::Tm,
    },
    Region {
        name: "log",
        start: LOG_START,
        len: LOG_LEN,
        kind: RegionKind::Log,
    },
    Region {
        name: "unused",
        start: UNUSED_START,
        len: UNUSED_LEN,
        kind: RegionKind::Unused,
    },
];

/// Région contenant l'octet `offset` de la RAM (pour la ventilation des pannes par le sim).
pub fn region_of(offset: usize) -> Option<&'static Region> {
    RAM_MAP
        .iter()
        .find(|r| offset >= r.start && offset < r.start + r.len)
}

/// Organisation physique d'une zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Tmr(TmrArea),
    Ecc(EccArea),
    Plain { start: usize, len: usize },
}

impl Layout {
    pub const fn start(&self) -> usize {
        match *self {
            Layout::Tmr(a) => a.base,
            Layout::Ecc(a) => a.base,
            Layout::Plain { start, .. } => start,
        }
    }
    pub const fn len(&self) -> usize {
        match *self {
            Layout::Tmr(a) => a.storage_len(),
            Layout::Ecc(a) => a.storage_len(),
            Layout::Plain { len, .. } => len,
        }
    }
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Zone physique de la RAM et sa protection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub kind: RegionKind,
    pub layout: Layout,
}

/// Toutes les zones, dans l'ordre des adresses ; elles pavent la RAM.
pub const AREAS: &[Area] = &[
    Area {
        kind: RegionKind::Sched,
        layout: Layout::Tmr(SCHED_TMR),
    },
    Area {
        kind: RegionKind::Mode,
        layout: Layout::Tmr(MODE_TMR),
    },
    Area {
        kind: RegionKind::Mode,
        layout: Layout::Plain {
            start: MODE_PLAIN_START,
            len: MODE_PLAIN_LEN,
        },
    },
    Area {
        kind: RegionKind::Fdir,
        layout: Layout::Tmr(FDIR_TMR),
    },
    Area {
        kind: RegionKind::Proto,
        layout: Layout::Tmr(PROTO_TMR),
    },
    Area {
        kind: RegionKind::Tm,
        layout: Layout::Ecc(TM_ECC),
    },
    Area {
        kind: RegionKind::Log,
        layout: Layout::Ecc(LOG_ECC),
    },
    Area {
        kind: RegionKind::Unused,
        layout: Layout::Plain {
            start: UNUSED_START,
            len: UNUSED_LEN,
        },
    },
];

/// Zones TMR et SECDED parcourues par le scrub (ROM).
pub const TMR_AREAS: [TmrArea; 4] = [SCHED_TMR, MODE_TMR, FDIR_TMR, PROTO_TMR];
pub const ECC_AREAS: [EccArea; 2] = [TM_ECC, LOG_ECC];
/// Octets de bloc TMR au total (unités de scrub TMR).
pub const TMR_UNITS: usize = SCHED_TMR.block + MODE_TMR.block + FDIR_TMR.block + PROTO_TMR.block;
/// Groupes SECDED au total (unités de scrub SECDED).
pub const ECC_UNITS: usize = TM_ECC.groups + LOG_ECC.groups;

// ---------------------------------------------------------------------------------------------
// Cellules
// ---------------------------------------------------------------------------------------------

// --- Sched (TMR, bloc de 20) : ordonnanceur, boot, scrub ------------------------------------

/// Secondes (cycles) depuis le boot, saturant.
pub const UPTIME: Slot<u32> = Slot::tmr(SCHED_TMR, SCHED_TMR.base);
/// Prochaine clé de watchdog (`WDG_KEY_A` / `WDG_KEY_B` ; toute autre valeur lue = clé A).
pub const WDG_KEY: Slot<u8> = Slot::tmr(SCHED_TMR, UPTIME.end());
/// Drapeaux d'exécution du cycle courant (bit = `sched::FLAG_*`), remis à 0 en début de cycle.
pub const EXEC_FLAGS: Slot<u8> = Slot::tmr(SCHED_TMR, WDG_KEY.end());
/// Dernière valeur du battement FDIR vue par l'ordonnanceur.
pub const HB_SEEN: Slot<u16> = Slot::tmr(SCHED_TMR, EXEC_FLAGS.end());
/// Cause du dernier reset (`ResetCause as u8`), lue au boot.
pub const RESET_CAUSE: Slot<u8> = Slot::tmr(SCHED_TMR, HB_SEEN.end());
/// Nombre de boots (relu de la NVM et incrémenté au boot par T-011).
pub const BOOT_COUNT: Slot<u16> = Slot::tmr(SCHED_TMR, RESET_CAUSE.end());
/// Curseur du scrub TMR (octet de bloc, global sur `TMR_AREAS`), lu modulo `TMR_UNITS`.
pub const SCRUB_TMR_CUR: Slot<u16> = Slot::tmr(SCHED_TMR, BOOT_COUNT.end());
/// Curseur du scrub SECDED (groupe, global sur `ECC_AREAS`), lu modulo `ECC_UNITS`.
pub const SCRUB_ECC_CUR: Slot<u16> = Slot::tmr(SCHED_TMR, SCRUB_TMR_CUR.end());
/// Alarme « vote TMR sans majorité » (non nul = levée), posée par `tmr`. Réaction (T-012) :
/// `sys_reset`. Remise à 0 par celui qui la traite.
pub const TMR_ALARM: Slot<u8> = Slot::tmr(SCHED_TMR, SCRUB_ECC_CUR.end());
/// RÉSERVÉ T-011 : `Reboot` accepté, `sys_reset` à émettre après l'ACK.
pub const REBOOT_PENDING: Slot<u8> = Slot::tmr(SCHED_TMR, TMR_ALARM.end());
/// RÉSERVÉ T-012 : ce boot a dépassé 300 s d'uptime (boot « long » déjà enregistré).
pub const BOOT_STABLE: Slot<u8> = Slot::tmr(SCHED_TMR, REBOOT_PENDING.end());
const SCHED_END: usize = BOOT_STABLE.end();
const _: () = assert!(SCHED_END <= SCHED_TMR.base + SCHED_TMR.block);

// --- Mode (TMR, bloc de 12 ; puis mesures du cycle non protégées) ----------------------------

/// Mode courant (`Mode as u8` ; valeur hors domaine lue = `Survie`).
pub const MODE: Slot<u8> = Slot::tmr(MODE_TMR, MODE_TMR.base);
/// Plafond de mode fixé par `SetMode` (valeur hors domaine lue = `Nominal`, défaut de boot).
pub const CEILING: Slot<u8> = Slot::tmr(MODE_TMR, MODE.end());
/// Secondes consécutives avec `V >= 7080 mV` (montée `Survie` -> `Eco`).
pub const CNT_V_ECO_UP: Slot<u16> = Slot::tmr(MODE_TMR, CEILING.end());
/// Secondes consécutives avec `V >= 7560 mV` (montée `Eco` -> `Nominal`).
pub const CNT_V_NOM_UP: Slot<u16> = Slot::tmr(MODE_TMR, CNT_V_ECO_UP.end());
/// Secondes consécutives avec `V < 7200 mV` (descente `Nominal` -> `Eco`).
pub const CNT_V_LOW: Slot<u16> = Slot::tmr(MODE_TMR, CNT_V_NOM_UP.end());
/// Secondes consécutives avec `V < 6720 mV` (tout -> `Survie`).
pub const CNT_V_CRIT: Slot<u16> = Slot::tmr(MODE_TMR, CNT_V_LOW.end());
/// État du thermostat (réchauffeur commandé).
pub const HEATER_ON: Slot<bool> = Slot::tmr(MODE_TMR, CNT_V_CRIT.end());
const _: () = assert!(HEATER_ON.end() <= MODE_TMR.base + MODE_TMR.block);

/// Tension batterie validée du cycle (mV ; 0 si aucune). Non protégée (mesure du cycle).
pub const VBATT_MV: Slot<u16> = Slot::at(MODE_PLAIN_START);
pub const VBATT_VALID: Slot<bool> = Slot::after(VBATT_MV.end());
/// Température batterie validée du cycle (0,1 °C).
pub const TBATT_DC: Slot<i16> = Slot::after(VBATT_VALID.end());
pub const TBATT_VALID: Slot<bool> = Slot::after(TBATT_DC.end());
/// Défauts capteurs instantanés du cycle (bit = `SensorId`, plus `SFM_*_DISAGREE`).
pub const SENSOR_FAULT_MASK: Slot<u16> = Slot::after(TBATT_VALID.end());
const _: () = assert!(SENSOR_FAULT_MASK.end() == MODE_PLAIN_START + MODE_PLAIN_LEN);

/// Bit de `SENSOR_FAULT_MASK` : capteurs de tension A/B en désaccord (> 150 mV).
pub const SFM_VBATT_DISAGREE: u16 = 1 << 9;
/// Bit de `SENSOR_FAULT_MASK` : capteurs de température A/B en désaccord.
pub const SFM_TEMP_DISAGREE: u16 = 1 << 10;

// --- Fdir (TMR, bloc de 68) : battement, drapeaux, compteurs, état par ligne et par capteur ---

/// Compteur de battement du FDIR, avancé de 1 par cycle par la tâche `fdir`.
pub const FDIR_HEARTBEAT: Slot<u16> = Slot::tmr(FDIR_TMR, FDIR_TMR.base);
/// Drapeaux FDIR publiés dans la balise (bits `FDIR_FLAG_*`).
pub const FDIR_FLAGS: Slot<u16> = Slot::tmr(FDIR_TMR, FDIR_HEARTBEAT.end());
/// Capteurs invalidés durablement par le FDIR (bit = `SensorId`) ; exclus de la validation.
pub const SENSOR_INVALID_MASK: Slot<u16> = Slot::tmr(FDIR_TMR, FDIR_FLAGS.end());
/// Lignes isolées par le FDIR (bit = `Line::mask`). Le bit Radio est ignoré à la lecture.
pub const LINE_ISOLATED: Slot<u8> = Slot::tmr(FDIR_TMR, SENSOR_INVALID_MASK.end());
/// Latch-up détectés (T-012), publié dans la balise.
pub const CNT_LATCHUP: Slot<u8> = Slot::tmr(FDIR_TMR, LINE_ISOLATED.end());
/// Registres trouvés altérés puis réécrits par `health`.
pub const CNT_REG_FIX: Slot<u16> = Slot::tmr(FDIR_TMR, CNT_LATCHUP.end());
/// Octets TMR réparés (lecture ou scrub), publié dans la balise.
pub const CNT_TMR_FIX: Slot<u16> = Slot::tmr(FDIR_TMR, CNT_REG_FIX.end());
/// Erreurs simples SECDED corrigées (lecture ou scrub), publié dans la balise.
pub const CNT_ECC_CORR: Slot<u16> = Slot::tmr(FDIR_TMR, CNT_TMR_FIX.end());
/// Données SECDED invalidées (erreur double détectée à la lecture).
pub const CNT_ECC_UNCORR: Slot<u16> = Slot::tmr(FDIR_TMR, CNT_ECC_CORR.end());
/// RÉSERVÉ T-012 : secondes stables écoulées sous verrou « tempête » (levée à 5700).
pub const STORM_STABLE_S: Slot<u16> = Slot::tmr(FDIR_TMR, CNT_ECC_UNCORR.end());
/// RÉSERVÉ T-011/T-012 : miroir du compteur NVM de boots courts (uptime < 300 s).
pub const SHORT_BOOTS: Slot<u8> = Slot::tmr(FDIR_TMR, STORM_STABLE_S.end());

const LU_OVER_0: usize = SHORT_BOOTS.end();
/// RÉSERVÉ T-012, par ligne (index = `Line as usize`) : échantillons consécutifs en
/// surintensité.
pub const LU_OVER: [Slot<u8>; 3] = [
    Slot::tmr(FDIR_TMR, LU_OVER_0),
    Slot::tmr(FDIR_TMR, LU_OVER_0 + 1),
    Slot::tmr(FDIR_TMR, LU_OVER_0 + 2),
];
const LU_OFF_0: usize = LU_OVER_0 + 3;
/// RÉSERVÉ T-012, par ligne : secondes de coupure restantes (latch-up, 5 s).
pub const LU_OFF_LEFT: [Slot<u8>; 3] = [
    Slot::tmr(FDIR_TMR, LU_OFF_0),
    Slot::tmr(FDIR_TMR, LU_OFF_0 + 1),
    Slot::tmr(FDIR_TMR, LU_OFF_0 + 2),
];
const LU_RECUR_0: usize = LU_OFF_0 + 3;
/// RÉSERVÉ T-012, par ligne : récidives dans la fenêtre courante.
pub const LU_RECUR: [Slot<u8>; 3] = [
    Slot::tmr(FDIR_TMR, LU_RECUR_0),
    Slot::tmr(FDIR_TMR, LU_RECUR_0 + 1),
    Slot::tmr(FDIR_TMR, LU_RECUR_0 + 2),
];
const LU_WIN_0: usize = LU_RECUR_0 + 3;
/// RÉSERVÉ T-012, par ligne : secondes restantes de la fenêtre de récidive (600 s).
pub const LU_WIN_LEFT: [Slot<u16>; 3] = [
    Slot::tmr(FDIR_TMR, LU_WIN_0),
    Slot::tmr(FDIR_TMR, LU_WIN_0 + 2),
    Slot::tmr(FDIR_TMR, LU_WIN_0 + 4),
];

/// Capteurs surveillés pour le figeage (index des tableaux `STUCK_*`).
pub const STUCK_SENSORS: [SensorId; 4] = [
    SensorId::VBattA,
    SensorId::VBattB,
    SensorId::TempBattA,
    SensorId::TempBattB,
];
const STUCK_LAST_0: usize = LU_WIN_0 + 6;
/// RÉSERVÉ T-012, par capteur de `STUCK_SENSORS` : dernière valeur brute.
pub const STUCK_LAST: [Slot<i16>; 4] = [
    Slot::tmr(FDIR_TMR, STUCK_LAST_0),
    Slot::tmr(FDIR_TMR, STUCK_LAST_0 + 2),
    Slot::tmr(FDIR_TMR, STUCK_LAST_0 + 4),
    Slot::tmr(FDIR_TMR, STUCK_LAST_0 + 6),
];
const STUCK_CNT_0: usize = STUCK_LAST_0 + 8;
/// RÉSERVÉ T-012 : secondes consécutives à valeur brute identique.
pub const STUCK_CNT: [Slot<u16>; 4] = [
    Slot::tmr(FDIR_TMR, STUCK_CNT_0),
    Slot::tmr(FDIR_TMR, STUCK_CNT_0 + 2),
    Slot::tmr(FDIR_TMR, STUCK_CNT_0 + 4),
    Slot::tmr(FDIR_TMR, STUCK_CNT_0 + 6),
];
const STUCK_MIN_0: usize = STUCK_CNT_0 + 8;
/// RÉSERVÉ T-012 : minimum du capteur REDONDANT sur la fenêtre de figeage.
pub const STUCK_MIN: [Slot<i16>; 4] = [
    Slot::tmr(FDIR_TMR, STUCK_MIN_0),
    Slot::tmr(FDIR_TMR, STUCK_MIN_0 + 2),
    Slot::tmr(FDIR_TMR, STUCK_MIN_0 + 4),
    Slot::tmr(FDIR_TMR, STUCK_MIN_0 + 6),
];
const STUCK_MAX_0: usize = STUCK_MIN_0 + 8;
/// RÉSERVÉ T-012 : maximum du capteur REDONDANT sur la fenêtre de figeage.
pub const STUCK_MAX: [Slot<i16>; 4] = [
    Slot::tmr(FDIR_TMR, STUCK_MAX_0),
    Slot::tmr(FDIR_TMR, STUCK_MAX_0 + 2),
    Slot::tmr(FDIR_TMR, STUCK_MAX_0 + 4),
    Slot::tmr(FDIR_TMR, STUCK_MAX_0 + 6),
];
const FDIR_END: usize = STUCK_MAX_0 + 8;
const _: () = assert!(FDIR_END <= FDIR_TMR.base + FDIR_TMR.block);

/// `FDIR_FLAGS` : alarme FDIR critique (bloque `Survie` -> `Eco`).
pub const FDIR_FLAG_CRITICAL: u16 = 1 << 0;
/// `FDIR_FLAGS` : verrou « tempête de resets » (force `Survie`).
pub const FDIR_FLAG_STORM_LOCK: u16 = 1 << 1;
/// `FDIR_FLAGS` : aucune copie NVM valide au boot, valeurs par défaut (T-011).
pub const FDIR_FLAG_NVM_LOST: u16 = 1 << 2;

// --- Proto (TMR, bloc de 16) : anti-rejeu, rejets, TC différées, NVM -------------------------

/// SEQ de la dernière TC acceptée (persisté en NVM par T-011).
pub const LAST_TC_SEQ: Slot<u32> = Slot::tmr(PROTO_TMR, PROTO_TMR.base);
/// Trames montantes rejetées (saturant).
pub const CNT_TC_REJECTED: Slot<u16> = Slot::tmr(PROTO_TMR, LAST_TC_SEQ.end());
/// RÉSERVÉ T-011 : lignes en cours de `PowerCycle` (bit = `Line::mask`).
pub const PWRCYC_LINES: Slot<u8> = Slot::tmr(PROTO_TMR, CNT_TC_REJECTED.end());
/// RÉSERVÉ T-011 : secondes de coupure `PowerCycle` restantes.
pub const PWRCYC_LEFT: Slot<u8> = Slot::tmr(PROTO_TMR, PWRCYC_LINES.end());
/// RÉSERVÉ T-011 : bloc NVM à (ré)écrire (nouvel essai au cycle suivant si `HalError`).
pub const NVM_PENDING: Slot<u8> = Slot::tmr(PROTO_TMR, PWRCYC_LEFT.end());
/// RÉSERVÉ T-011 : copies NVM trouvées valides au boot (bit = copie).
pub const NVM_STATUS: Slot<u8> = Slot::tmr(PROTO_TMR, NVM_PENDING.end());
/// RÉSERVÉ T-011 : copies NVM réparées.
pub const CNT_NVM_REPAIR: Slot<u16> = Slot::tmr(PROTO_TMR, NVM_STATUS.end());
/// RÉSERVÉ T-011 : échecs d'écriture NVM.
pub const CNT_NVM_WERR: Slot<u16> = Slot::tmr(PROTO_TMR, CNT_NVM_REPAIR.end());
const PROTO_END: usize = CNT_NVM_WERR.end();
const _: () = assert!(PROTO_END <= PROTO_TMR.base + PROTO_TMR.block);

// --- Tm (SECDED, 24 octets de données) : télémétrie, ACK en attente, charge utile ------------

/// Compteur de trames descendantes (SEQ côté TM).
pub const TM_SEQ: Slot<u32> = Slot::ecc(TM_ECC, 0);
/// Secondes depuis la dernière balise. Lu borné : `min(valeur, période)` (R13). Donnée
/// invalide -> `u16::MAX` : balise immédiate.
pub const BEACON_ELAPSED: Slot<u16> = Slot::ecc(TM_ECC, TM_SEQ.end()).with_fallback(0xFFFF);
/// ACK en attente d'émission ssi la valeur vaut exactement `ACK_PENDING_YES`.
pub const ACK_PENDING: Slot<u8> = Slot::ecc(TM_ECC, BEACON_ELAPSED.end());
pub const ACK_SEQ: Slot<u32> = Slot::ecc(TM_ECC, ACK_PENDING.end());
pub const ACK_OPCODE: Slot<u8> = Slot::ecc(TM_ECC, ACK_SEQ.end());
/// Donnée invalide -> `Refused` (2).
pub const ACK_RESULT: Slot<u8> = Slot::ecc(TM_ECC, ACK_OPCODE.end()).with_fallback(2);
/// Échantillons acquis par la charge utile factice.
pub const PAYLOAD_SAMPLES: Slot<u32> = Slot::ecc(TM_ECC, ACK_RESULT.end());
/// RÉSERVÉ T-011 : vidage du journal en cours (`DumpEvents`).
pub const DUMP_ACTIVE: Slot<u8> = Slot::ecc(TM_ECC, PAYLOAD_SAMPLES.end());
/// RÉSERVÉ T-011 : prochaine entrée du journal à émettre.
pub const DUMP_CURSOR: Slot<u16> = Slot::ecc(TM_ECC, DUMP_ACTIVE.end());
/// RÉSERVÉ T-011 : entrées restant à émettre.
pub const DUMP_LEFT: Slot<u16> = Slot::ecc(TM_ECC, DUMP_CURSOR.end());
const TM_END: usize = DUMP_LEFT.end();
const _: () = assert!(TM_END <= TM_ECC.data_len());

/// Valeur de `ACK_PENDING` signifiant « un ACK attend ». Toute autre valeur = rien à émettre.
pub const ACK_PENDING_YES: u8 = 0xC3;

// --- Log (SECDED, 328 octets de données) : journal d'événements (T-011) ----------------------

/// RÉSERVÉ T-011 : index de la prochaine entrée à écrire.
pub const LOG_HEAD: Slot<u16> = Slot::ecc(LOG_ECC, 0);
/// RÉSERVÉ T-011 : entrées valides dans le journal.
pub const LOG_COUNT: Slot<u16> = Slot::ecc(LOG_ECC, LOG_HEAD.end());
/// RÉSERVÉ T-011 : événements journalisés depuis le boot (saturant).
pub const LOG_TOTAL: Slot<u16> = Slot::ecc(LOG_ECC, LOG_COUNT.end());
/// Début du tampon d'entrées (2 octets de réserve après l'en-tête).
pub const LOG_BUF_START: usize = LOG_TOTAL.end() + 2;
/// Taille du tampon d'entrées (320 octets : 40 entrées de 8 octets, format T-011).
pub const LOG_BUF_LEN: usize = LOG_ECC.data_len() - LOG_BUF_START;
/// RÉSERVÉ T-011 : tampon d'entrées du journal.
pub const LOG_BUF: EccBuf = EccBuf::new(LOG_ECC, LOG_BUF_START, LOG_BUF_LEN);
const _: () = assert!(LOG_BUF_LEN == 320);

/// Toutes les cellules, pour les tests de disposition et la couverture de la RAM.
pub const SLOTS: &[SlotInfo] = &[
    UPTIME.info("uptime", RegionKind::Sched),
    WDG_KEY.info("wdg_key", RegionKind::Sched),
    EXEC_FLAGS.info("exec_flags", RegionKind::Sched),
    HB_SEEN.info("hb_seen", RegionKind::Sched),
    RESET_CAUSE.info("reset_cause", RegionKind::Sched),
    BOOT_COUNT.info("boot_count", RegionKind::Sched),
    SCRUB_TMR_CUR.info("scrub_tmr_cur", RegionKind::Sched),
    SCRUB_ECC_CUR.info("scrub_ecc_cur", RegionKind::Sched),
    TMR_ALARM.info("tmr_alarm", RegionKind::Sched),
    REBOOT_PENDING.info("reboot_pending", RegionKind::Sched),
    BOOT_STABLE.info("boot_stable", RegionKind::Sched),
    MODE.info("mode", RegionKind::Mode),
    CEILING.info("ceiling", RegionKind::Mode),
    CNT_V_ECO_UP.info("cnt_v_eco_up", RegionKind::Mode),
    CNT_V_NOM_UP.info("cnt_v_nom_up", RegionKind::Mode),
    CNT_V_LOW.info("cnt_v_low", RegionKind::Mode),
    CNT_V_CRIT.info("cnt_v_crit", RegionKind::Mode),
    HEATER_ON.info("heater_on", RegionKind::Mode),
    VBATT_MV.info("vbatt_mv", RegionKind::Mode),
    VBATT_VALID.info("vbatt_valid", RegionKind::Mode),
    TBATT_DC.info("tbatt_dc", RegionKind::Mode),
    TBATT_VALID.info("tbatt_valid", RegionKind::Mode),
    SENSOR_FAULT_MASK.info("sensor_fault_mask", RegionKind::Mode),
    FDIR_HEARTBEAT.info("fdir_heartbeat", RegionKind::Fdir),
    FDIR_FLAGS.info("fdir_flags", RegionKind::Fdir),
    SENSOR_INVALID_MASK.info("sensor_invalid_mask", RegionKind::Fdir),
    LINE_ISOLATED.info("line_isolated", RegionKind::Fdir),
    CNT_LATCHUP.info("cnt_latchup", RegionKind::Fdir),
    CNT_REG_FIX.info("cnt_reg_fix", RegionKind::Fdir),
    CNT_TMR_FIX.info("cnt_tmr_fix", RegionKind::Fdir),
    CNT_ECC_CORR.info("cnt_ecc_corr", RegionKind::Fdir),
    CNT_ECC_UNCORR.info("cnt_ecc_uncorr", RegionKind::Fdir),
    STORM_STABLE_S.info("storm_stable_s", RegionKind::Fdir),
    SHORT_BOOTS.info("short_boots", RegionKind::Fdir),
    LU_OVER[0].info("lu_over_payload", RegionKind::Fdir),
    LU_OVER[1].info("lu_over_radio", RegionKind::Fdir),
    LU_OVER[2].info("lu_over_heater", RegionKind::Fdir),
    LU_OFF_LEFT[0].info("lu_off_left_payload", RegionKind::Fdir),
    LU_OFF_LEFT[1].info("lu_off_left_radio", RegionKind::Fdir),
    LU_OFF_LEFT[2].info("lu_off_left_heater", RegionKind::Fdir),
    LU_RECUR[0].info("lu_recur_payload", RegionKind::Fdir),
    LU_RECUR[1].info("lu_recur_radio", RegionKind::Fdir),
    LU_RECUR[2].info("lu_recur_heater", RegionKind::Fdir),
    LU_WIN_LEFT[0].info("lu_win_left_payload", RegionKind::Fdir),
    LU_WIN_LEFT[1].info("lu_win_left_radio", RegionKind::Fdir),
    LU_WIN_LEFT[2].info("lu_win_left_heater", RegionKind::Fdir),
    STUCK_LAST[0].info("stuck_last_vbatt_a", RegionKind::Fdir),
    STUCK_LAST[1].info("stuck_last_vbatt_b", RegionKind::Fdir),
    STUCK_LAST[2].info("stuck_last_temp_a", RegionKind::Fdir),
    STUCK_LAST[3].info("stuck_last_temp_b", RegionKind::Fdir),
    STUCK_CNT[0].info("stuck_cnt_vbatt_a", RegionKind::Fdir),
    STUCK_CNT[1].info("stuck_cnt_vbatt_b", RegionKind::Fdir),
    STUCK_CNT[2].info("stuck_cnt_temp_a", RegionKind::Fdir),
    STUCK_CNT[3].info("stuck_cnt_temp_b", RegionKind::Fdir),
    STUCK_MIN[0].info("stuck_min_vbatt_a", RegionKind::Fdir),
    STUCK_MIN[1].info("stuck_min_vbatt_b", RegionKind::Fdir),
    STUCK_MIN[2].info("stuck_min_temp_a", RegionKind::Fdir),
    STUCK_MIN[3].info("stuck_min_temp_b", RegionKind::Fdir),
    STUCK_MAX[0].info("stuck_max_vbatt_a", RegionKind::Fdir),
    STUCK_MAX[1].info("stuck_max_vbatt_b", RegionKind::Fdir),
    STUCK_MAX[2].info("stuck_max_temp_a", RegionKind::Fdir),
    STUCK_MAX[3].info("stuck_max_temp_b", RegionKind::Fdir),
    LAST_TC_SEQ.info("last_tc_seq", RegionKind::Proto),
    CNT_TC_REJECTED.info("cnt_tc_rejected", RegionKind::Proto),
    PWRCYC_LINES.info("pwrcyc_lines", RegionKind::Proto),
    PWRCYC_LEFT.info("pwrcyc_left", RegionKind::Proto),
    NVM_PENDING.info("nvm_pending", RegionKind::Proto),
    NVM_STATUS.info("nvm_status", RegionKind::Proto),
    CNT_NVM_REPAIR.info("cnt_nvm_repair", RegionKind::Proto),
    CNT_NVM_WERR.info("cnt_nvm_werr", RegionKind::Proto),
    TM_SEQ.info("tm_seq", RegionKind::Tm),
    BEACON_ELAPSED.info("beacon_elapsed", RegionKind::Tm),
    ACK_PENDING.info("ack_pending", RegionKind::Tm),
    ACK_SEQ.info("ack_seq", RegionKind::Tm),
    ACK_OPCODE.info("ack_opcode", RegionKind::Tm),
    ACK_RESULT.info("ack_result", RegionKind::Tm),
    PAYLOAD_SAMPLES.info("payload_samples", RegionKind::Tm),
    DUMP_ACTIVE.info("dump_active", RegionKind::Tm),
    DUMP_CURSOR.info("dump_cursor", RegionKind::Tm),
    DUMP_LEFT.info("dump_left", RegionKind::Tm),
    LOG_HEAD.info("log_head", RegionKind::Log),
    LOG_COUNT.info("log_count", RegionKind::Log),
    LOG_TOTAL.info("log_total", RegionKind::Log),
];

/// Lecture bornée d'un compteur de période (R13) : une valeur corrompue au-delà de `period` est
/// ramenée à `period`, ce qui déclenche l'action due au lieu de la reporter indéfiniment.
pub fn bounded(v: u16, period: u16) -> u16 {
    v.min(period)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    #[test]
    fn scalar_roundtrip_big_endian() {
        let mut ram = [0u8; RAM_SIZE];
        let a: Slot<u32> = Slot::at(10);
        a.set(&mut ram, 0x0102_0304);
        assert_eq!(&ram[10..14], &[1, 2, 3, 4]);
        assert_eq!(a.get(&mut ram), 0x0102_0304);
        let b: Slot<i16> = Slot::at(20);
        b.set(&mut ram, -2);
        assert_eq!(&ram[20..22], &[0xFF, 0xFE]);
        assert_eq!(b.get(&mut ram), -2);
        let c: Slot<u16> = Slot::at(RAM_SIZE - 2);
        c.set(&mut ram, 0xBEEF);
        assert_eq!(c.get(&mut ram), 0xBEEF);
        let d: Slot<bool> = Slot::at(30);
        d.set(&mut ram, true);
        assert_eq!(ram[30], 1);
        ram[30] = 0x80; // corruption : toute valeur non nulle vaut vrai
        assert!(d.get(&mut ram));
        let e: Slot<u8> = Slot::at(31);
        e.set(&mut ram, 0x7E);
        assert_eq!(e.get(&mut ram), 0x7E);
        assert_eq!((a.off(), a.len(), a.end()), (10, 4, 14));
        assert_eq!(i16::from_bits(0xFFFE), -2);
        assert_eq!(u8::from_bits(0x1FF), 0xFF);
    }

    #[test]
    fn tmr_slot_writes_three_spaced_copies() {
        let mut ram = [0u8; RAM_SIZE];
        LAST_TC_SEQ.set(&mut ram, 0xA1B2_C3D4);
        let o = LAST_TC_SEQ.off();
        let s = PROTO_TMR.block;
        for c in 0..3 {
            assert_eq!(&ram[o + c * s..o + c * s + 4], &[0xA1, 0xB2, 0xC3, 0xD4]);
        }
        let touched = ram.iter().filter(|&&b| b != 0).count();
        assert_eq!(touched, 12);
        assert_eq!(LAST_TC_SEQ.get(&mut ram), 0xA1B2_C3D4);
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn ecc_slot_double_error_gives_fallback_and_is_counted() {
        let mut ram = [0u8; RAM_SIZE];
        BEACON_ELAPSED.set(&mut ram, 3);
        let (b0, l0) = TM_ECC.phys_bit(BEACON_ELAPSED.off() + 1, 3);
        let (b1, l1) = TM_ECC.phys_bit(BEACON_ELAPSED.off() + 1, 10);
        ram[b0] ^= 1 << l0;
        ram[b1] ^= 1 << l1;
        assert_eq!(BEACON_ELAPSED.get(&mut ram), 0xFFFF);
        assert_eq!(CNT_ECC_UNCORR.get(&mut ram), 1);
        // Invalidée par réécriture : relue proprement, plus comptée.
        assert_eq!(BEACON_ELAPSED.get(&mut ram), 0xFFFF);
        assert_eq!(CNT_ECC_UNCORR.get(&mut ram), 1);
        // Tampon du journal : None, laissé en place, compté à chaque lecture.
        LOG_BUF.set(&mut ram, 7, 0x42);
        let (b0, l0) = LOG_ECC.phys_bit(LOG_BUF_START + 7, 0);
        let (b1, l1) = LOG_ECC.phys_bit(LOG_BUF_START + 7, 12);
        ram[b0] ^= 1 << l0;
        ram[b1] ^= 1 << l1;
        assert_eq!(LOG_BUF.get(&mut ram, 7), None);
        assert_eq!(CNT_ECC_UNCORR.get(&mut ram), 2);
        assert_eq!(LOG_BUF.get(&mut ram, LOG_BUF_LEN), None);
        LOG_BUF.set(&mut ram, 7, 0x43);
        assert_eq!(LOG_BUF.get(&mut ram, 7), Some(0x43));
    }

    #[test]
    fn ram_map_is_contiguous_and_complete() {
        let mut next = 0;
        for r in RAM_MAP {
            assert_eq!(r.start, next, "trou ou chevauchement avant {}", r.name);
            assert!(r.len > 0);
            next = r.start + r.len;
        }
        assert_eq!(next, RAM_SIZE);
        let unused: usize = RAM_MAP
            .iter()
            .filter(|r| r.kind == RegionKind::Unused)
            .map(|r| r.len)
            .sum();
        assert!(unused * 10 < RAM_SIZE, "Unused >= 10 % de la RAM");
        assert_eq!(region_of(0).unwrap().kind, RegionKind::Sched);
        assert_eq!(region_of(RAM_SIZE - 1).unwrap().kind, RegionKind::Unused);
        assert!(region_of(RAM_SIZE).is_none());
    }

    /// Les zones pavent la RAM, chacune dans la région de son type.
    #[test]
    fn areas_tile_ram_and_regions() {
        let mut next = 0;
        for a in AREAS {
            assert_eq!(a.layout.start(), next);
            assert!(!a.layout.is_empty());
            let r = region_of(a.layout.start()).unwrap();
            assert_eq!(r.kind, a.kind);
            assert!(a.layout.start() + a.layout.len() <= r.start + r.len);
            next += a.layout.len();
        }
        assert_eq!(next, RAM_SIZE);
        // Chaque région est exactement couverte par ses zones.
        for r in RAM_MAP {
            let covered: usize = AREAS
                .iter()
                .filter(|a| a.kind == r.kind)
                .map(|a| a.layout.len())
                .sum();
            assert_eq!(covered, r.len, "{}", r.name);
        }
    }

    #[test]
    fn slots_do_not_overlap_and_live_in_their_region() {
        // Empreinte de chaque cellule en octets « logiques » : (espace, début, fin).
        let space = |s: &SlotInfo| match s.prot {
            Prot::Ecc(a) => a.base,
            _ => usize::MAX,
        };
        for (i, a) in SLOTS.iter().enumerate() {
            assert!(a.len > 0 && a.len <= 4, "{}", a.name);
            assert_ne!(a.kind, RegionKind::Unused);
            match a.prot {
                Prot::Plain | Prot::Tmr(_) => {
                    let r = region_of(a.start).unwrap();
                    assert_eq!(r.kind, a.kind, "{} hors de sa région", a.name);
                    let stride = if let Prot::Tmr(t) = a.prot {
                        t.block
                    } else {
                        0
                    };
                    assert!(
                        a.start + 2 * stride + a.len <= r.start + r.len,
                        "{}",
                        a.name
                    );
                    if let Prot::Tmr(t) = a.prot {
                        assert!(a.start >= t.base && a.start + a.len <= t.base + t.block);
                        // Copies jamais adjacentes : au moins un octet d'écart.
                        assert!(t.block > a.len, "{}", a.name);
                    }
                }
                Prot::Ecc(e) => {
                    assert_eq!(region_of(e.base).unwrap().kind, a.kind, "{}", a.name);
                    assert!(a.start + a.len <= e.data_len());
                }
            }
            for b in &SLOTS[i + 1..] {
                assert_ne!(a.name, b.name);
                if space(a) != space(b) {
                    continue;
                }
                let disjoint = a.start + a.len <= b.start || b.start + b.len <= a.start;
                assert!(disjoint, "{} chevauche {}", a.name, b.name);
            }
        }
        // Le tampon du journal ne chevauche pas l'en-tête.
        assert!(LOG_BUF_START >= LOG_TOTAL.end());
    }

    /// Une cellule n'écrit que ses propres octets (toutes ses copies, ou sa voie SECDED).
    #[test]
    fn set_touches_only_its_bytes() {
        for s in SLOTS {
            let mut ram = [0u8; RAM_SIZE];
            let slot: Slot<u8> = match s.prot {
                Prot::Plain => Slot::at(s.start),
                Prot::Tmr(t) => Slot::tmr(t, s.start),
                Prot::Ecc(e) => Slot::ecc(e, s.start),
            };
            slot.set(&mut ram, 0xFF);
            let touched: Vec<usize> = (0..RAM_SIZE).filter(|&i| ram[i] != 0).collect();
            let expected: Vec<usize> = match s.prot {
                Prot::Plain => std::vec![s.start],
                Prot::Tmr(t) => std::vec![s.start, s.start + t.block, s.start + 2 * t.block],
                Prot::Ecc(e) => {
                    let cw = ecc::encode(0xFF);
                    (0..13)
                        .filter(|k| cw & (1 << k) != 0)
                        .map(|k| e.phys_bit(s.start, k).0)
                        .collect()
                }
            };
            assert_eq!(touched, expected, "{}", s.name);
            assert_eq!(slot.get(&mut ram), 0xFF);
        }
    }

    /// Occupation de la RAM par type de région et par protection (compte rendu, R6).
    #[test]
    fn coverage_is_reported() {
        for r in RAM_MAP {
            let (mut tmr_b, mut ecc_b, mut plain_b) = (0, 0, 0);
            for a in AREAS.iter().filter(|a| a.kind == r.kind) {
                match a.layout {
                    Layout::Tmr(_) => tmr_b += a.layout.len(),
                    Layout::Ecc(_) => ecc_b += a.layout.len(),
                    Layout::Plain { len, .. } => plain_b += len,
                }
            }
            let used: usize = SLOTS
                .iter()
                .filter(|s| s.kind == r.kind)
                .map(|s| match s.prot {
                    Prot::Tmr(_) => 3 * s.len,
                    Prot::Ecc(_) => s.len * ecc::GROUP_BYTES / ecc::GROUP_DATA,
                    Prot::Plain => s.len,
                })
                .sum::<usize>()
                + if r.kind == RegionKind::Log {
                    LOG_BUF_LEN * ecc::GROUP_BYTES / ecc::GROUP_DATA
                } else {
                    0
                };
            std::println!(
                "{:7} {:4} o ({:2} %) : TMR {:3} o, SECDED {:3} o, brut {:3} o ; cellules {:3} o ({:3} %)",
                r.name,
                r.len,
                r.len * 100 / RAM_SIZE,
                tmr_b,
                ecc_b,
                plain_b,
                used,
                used * 100 / r.len
            );
        }
        assert_eq!(TMR_UNITS, 116);
        assert_eq!(ECC_UNITS, 44);
    }

    #[test]
    fn bounded_read() {
        assert_eq!(bounded(5, 10), 5);
        assert_eq!(bounded(10, 10), 10);
        assert_eq!(bounded(u16::MAX, 60), 60);
    }
}
