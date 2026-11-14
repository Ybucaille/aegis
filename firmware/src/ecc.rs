//! SECDED Hamming (13,8) entrelacé bit à bit (SPEC §4.3 ; choix T-010).
//!
//! Code : Hamming étendu (13,8). Bits du mot de code numérotés 0..=12 :
//! - positions de Hamming 1..=12 : parités en 1, 2, 4, 8 ; données d0..d7 en 3, 5, 6, 7, 9, 10,
//!   11, 12 ;
//! - bit 0 : parité globale (paire) des 13 bits.
//!
//! Décodage : syndrome `s` (XOR des positions des bits à 1) et parité globale `p`.
//! `s = 0, p = 0` : propre ; `p = 1` : erreur simple (en `s`, ou sur le bit 0 si `s = 0`),
//! corrigée ; `p = 0, s != 0` : erreur double, détectée, jamais « corrigée ». `p = 1` avec
//! `s > 12` (impossible pour une erreur simple) : déclaré non corrigible.
//!
//! Pourquoi (13,8) et pas (39,32) : granularité d'un octet (toute cellule `u8/u16/u32`, et le
//! journal octet par octet), test exhaustif sur TOUS les mots, et surtout l'entrelacement
//! ci-dessous, qui rend les MBU de 2 à 4 bits adjacents corrigibles.
//!
//! Stockage entrelacé (« bit-sliced ») : un GROUPE de 8 octets de données occupe 13 octets de
//! RAM. Le bit `k` du mot de code de la voie `i` (octet de données `i` du groupe) est rangé dans
//! le bit `i` de l'octet de stockage `k`. Deux bits adjacents de la RAM (dans un octet, ou de
//! part et d'autre d'une frontière d'octets, quel que soit le sens de numérotation) appartiennent
//! donc à deux voies différentes : un MBU de 2 à 8 bits adjacents touche chaque mot de code au
//! plus une fois, et chaque erreur est simple, donc corrigée. Coût : 62,5 % de redondance.
//!
//! Témoin `unprotected` (R8) : UN seul point, la constante `PROTECTED`. Sans protection, les
//! bits de données sont lus tels quels (ni correction, ni détection) et le scrub ne fait rien.

use crate::ram::{self, Ram};

/// Seul point de la feature `unprotected` dans ce module.
const PROTECTED: bool = !cfg!(feature = "unprotected");

/// Octets de données par groupe entrelacé.
pub const GROUP_DATA: usize = 8;
/// Octets de stockage par groupe (bits par mot de code).
pub const GROUP_BYTES: usize = 13;

/// Position (dans le mot de code) de chaque bit de données d0..d7.
const DATA_POS: [u8; 8] = [3, 5, 6, 7, 9, 10, 11, 12];

/// Masque des positions couvertes par la parité de rang `k` (positions `p` avec le bit `k`).
const fn cover_mask(k: u32) -> u16 {
    let mut m = 0u16;
    let mut p = 1;
    while p <= 12 {
        if p & (1 << k) != 0 {
            m |= 1 << p;
        }
        p += 1;
    }
    m
}

const COVER: [u16; 4] = [cover_mask(0), cover_mask(1), cover_mask(2), cover_mask(3)];

const fn encode_const(d: u8) -> u16 {
    let mut cw = 0u16;
    let mut j = 0;
    while j < 8 {
        if d & (1 << j) != 0 {
            cw |= 1 << DATA_POS[j];
        }
        j += 1;
    }
    let mut k = 0;
    while k < 4 {
        if (cw & COVER[k]).count_ones() & 1 == 1 {
            cw |= 1 << (1u16 << k);
        }
        k += 1;
    }
    if cw.count_ones() & 1 == 1 {
        cw |= 1;
    }
    cw
}

const fn build_table() -> [u16; 256] {
    let mut t = [0u16; 256];
    let mut d = 0;
    while d < 256 {
        t[d] = encode_const(d as u8);
        d += 1;
    }
    t
}

/// Table d'encodage (ROM).
const ENC: [u16; 256] = build_table();

/// Mot de code (13 bits utiles) de l'octet `d`.
pub fn encode(d: u8) -> u16 {
    ENC[d as usize]
}

/// Bits de données d'un mot de code, sans aucune vérification.
pub fn data_bits(cw: u16) -> u8 {
    let mut d = 0u8;
    for (j, &p) in DATA_POS.iter().enumerate() {
        if cw & (1 << p) != 0 {
            d |= 1 << j;
        }
    }
    d
}

/// Résultat du décodage d'un mot de code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decoded {
    /// Mot propre.
    Clean(u8),
    /// Erreur simple corrigée : donnée et mot de code corrigé.
    Corrected(u8, u16),
    /// Erreur double (ou pire, détectable) : donnée invalide, à ne pas utiliser.
    Uncorrectable,
}

/// Décodage SECDED pur (indépendant de la feature `unprotected`).
pub fn decode(cw: u16) -> Decoded {
    let cw = cw & 0x1FFF;
    let mut s = 0u16;
    for (k, &m) in COVER.iter().enumerate() {
        if (cw & m).count_ones() & 1 == 1 {
            s |= 1 << k;
        }
    }
    let odd = cw.count_ones() & 1 == 1;
    match (s, odd) {
        (0, false) => Decoded::Clean(data_bits(cw)),
        (0, true) => {
            let fixed = cw ^ 1;
            Decoded::Corrected(data_bits(fixed), fixed)
        }
        (s, true) if s <= 12 => {
            let fixed = cw ^ (1 << s);
            Decoded::Corrected(data_bits(fixed), fixed)
        }
        _ => Decoded::Uncorrectable,
    }
}

// ---------------------------------------------------------------------------------------------
// Stockage entrelacé dans l'image RAM
// ---------------------------------------------------------------------------------------------

/// Zone SECDED : `groups` groupes entrelacés à partir de `base`.
/// Invariant (vérifié à la compilation, toutes les zones étant des `const`) :
/// `base + groups * GROUP_BYTES <= RAM_SIZE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EccArea {
    pub base: usize,
    pub groups: usize,
}

impl EccArea {
    pub const fn new(base: usize, groups: usize) -> Self {
        assert!(
            base + groups * GROUP_BYTES <= crate::hal::RAM_SIZE,
            "zone SECDED hors RAM"
        );
        assert!(groups > 0);
        EccArea { base, groups }
    }
    /// Octets de données utilisables.
    pub const fn data_len(&self) -> usize {
        self.groups * GROUP_DATA
    }
    /// Octets de RAM occupés.
    pub const fn storage_len(&self) -> usize {
        self.groups * GROUP_BYTES
    }
    pub const fn end(&self) -> usize {
        self.base + self.storage_len()
    }
    /// Début du groupe contenant l'octet de données `idx`, et voie de cet octet.
    const fn locate(&self, idx: usize) -> (usize, u8) {
        (
            self.base + (idx / GROUP_DATA) * GROUP_BYTES,
            (idx % GROUP_DATA) as u8,
        )
    }
    /// Emplacement physique `(octet, bit)` du bit `k` (0..13) du mot de code de l'octet de
    /// données `idx` (diagnostic et tests d'injection).
    pub const fn phys_bit(&self, idx: usize, k: usize) -> (usize, u8) {
        let (g, lane) = self.locate(idx);
        (g + k, lane)
    }
}

/// Lit le mot de code de la voie `lane` du groupe commençant en `g`.
/// Précondition : `g + GROUP_BYTES <= RAM_SIZE` (garantie par `EccArea::new`).
fn lane_read(ram: &Ram, g: usize, lane: u8) -> u16 {
    let mut cw = 0u16;
    for (k, &b) in ram[g..g + GROUP_BYTES].iter().enumerate() {
        cw |= (((b >> lane) & 1) as u16) << k;
    }
    cw
}

/// Écrit le mot de code de la voie `lane` sans toucher aux autres voies.
fn lane_write(ram: &mut Ram, g: usize, lane: u8, cw: u16) {
    let m = 1u8 << lane;
    for (k, b) in ram[g..g + GROUP_BYTES].iter_mut().enumerate() {
        let bit = ((cw >> k) & 1) as u8;
        *b = (*b & !m) | (bit << lane);
    }
}

fn bump(ram: &mut Ram, slot: &ram::Slot<u16>, n: u16) {
    let v = slot.get(ram).saturating_add(n);
    slot.set(ram, v);
}

/// Lecture de l'octet de données `idx` de la zone. Erreur simple : corrigée EN RAM (réécriture à
/// la lecture) et comptée dans `CNT_ECC_CORR`. Erreur double : `None`, rien n'est réécrit (à
/// l'appelant d'invalider) ; le comptage des erreurs non corrigibles est fait par l'appelant.
/// `idx >= data_len` : `None` (aucun accès hors zone).
pub fn read_byte(ram: &mut Ram, area: &EccArea, idx: usize) -> Option<u8> {
    if idx >= area.data_len() {
        return None;
    }
    let (g, lane) = area.locate(idx);
    let cw = lane_read(ram, g, lane);
    if !PROTECTED {
        return Some(data_bits(cw));
    }
    match decode(cw) {
        Decoded::Clean(d) => Some(d),
        Decoded::Corrected(d, fixed) => {
            lane_write(ram, g, lane, fixed);
            bump(ram, &ram::CNT_ECC_CORR, 1);
            Some(d)
        }
        Decoded::Uncorrectable => None,
    }
}

/// Écriture de l'octet de données `idx` (ré-encodé ; efface toute erreur de la voie).
/// `idx >= data_len` : ignoré.
pub fn write_byte(ram: &mut Ram, area: &EccArea, idx: usize, v: u8) {
    if idx >= area.data_len() {
        return;
    }
    let (g, lane) = area.locate(idx);
    lane_write(ram, g, lane, encode(v));
}

/// Scrub d'un groupe (index `group` dans la zone) : corrige toute voie à erreur simple.
/// Les voies à erreur double sont laissées en place (la lecture typée les invalidera avec la
/// valeur de repli de la cellule). Renvoie le nombre de corrections (déjà comptées).
pub fn scrub_group(ram: &mut Ram, area: &EccArea, group: usize) -> u16 {
    if !PROTECTED || group >= area.groups {
        return 0;
    }
    let g = area.base + group * GROUP_BYTES;
    let mut fixed = 0u16;
    for lane in 0..GROUP_DATA as u8 {
        if let Decoded::Corrected(_, cw) = decode(lane_read(ram, g, lane)) {
            lane_write(ram, g, lane, cw);
            fixed += 1;
        }
    }
    if fixed > 0 {
        bump(ram, &ram::CNT_ECC_CORR, fixed);
    }
    fixed
}

/// Vrai si toutes les voies du groupe sont propres (diagnostic et tests).
pub fn group_clean(ram: &Ram, area: &EccArea, group: usize) -> bool {
    let g = area.base + group * GROUP_BYTES;
    (0..GROUP_DATA as u8).all(|l| matches!(decode(lane_read(ram, g, l)), Decoded::Clean(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::RAM_SIZE;

    /// Exhaustif : tout mot, toute erreur simple corrigée, toute erreur double détectée et
    /// jamais « corrigée » en faux.
    #[test]
    fn secded_exhaustive() {
        let mut seen = [false; 1 << 13];
        for d in 0..=255u8 {
            let cw = encode(d);
            assert_eq!(cw >> 13, 0);
            assert!(!seen[cw as usize], "deux données, même mot");
            seen[cw as usize] = true;
            assert_eq!(data_bits(cw), d);
            assert_eq!(decode(cw), Decoded::Clean(d));
            for i in 0..13 {
                let e = cw ^ (1 << i);
                assert_eq!(decode(e), Decoded::Corrected(d, cw), "d={d} bit {i}");
                for j in i + 1..13 {
                    assert_eq!(
                        decode(e ^ (1 << j)),
                        Decoded::Uncorrectable,
                        "d={d} bits {i},{j}"
                    );
                }
            }
        }
    }

    /// Distance minimale 4 (SECDED) : deux mots distincts diffèrent d'au moins 4 bits.
    #[test]
    fn minimum_distance_is_four() {
        let mut min = 13;
        for a in 0..256u16 {
            for b in a + 1..256 {
                min = min.min((encode(a as u8) ^ encode(b as u8)).count_ones());
            }
        }
        assert_eq!(min, 4);
    }

    #[test]
    fn zero_encodes_to_zero() {
        // RAM effacée au boot = zone SECDED valide contenant des zéros.
        assert_eq!(encode(0), 0);
    }

    // Zone de test dans la région `unused` (les compteurs vivent dans `fdir`).
    const AREA: EccArea = EccArea::new(940, 3);

    #[test]
    fn interleaved_roundtrip_and_isolation() {
        let mut ram = [0u8; RAM_SIZE];
        for i in 0..AREA.data_len() {
            write_byte(&mut ram, &AREA, i, (i as u8).wrapping_mul(37) ^ 0x5A);
        }
        for i in 0..AREA.data_len() {
            assert_eq!(
                read_byte(&mut ram, &AREA, i),
                Some((i as u8).wrapping_mul(37) ^ 0x5A)
            );
        }
        // Rien hors de la zone.
        assert!(ram[..AREA.base].iter().all(|&b| b == 0));
        assert!(ram[AREA.end()..].iter().all(|&b| b == 0));
        assert_eq!(read_byte(&mut ram, &AREA, AREA.data_len()), None);
        write_byte(&mut ram, &AREA, AREA.data_len(), 1); // ignoré
        assert!(ram[AREA.end()..].iter().all(|&b| b == 0));
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn every_single_bit_of_area_is_corrected_on_read() {
        let mut clean = [0u8; RAM_SIZE];
        for i in 0..AREA.data_len() {
            write_byte(&mut clean, &AREA, i, 0xC0 | i as u8);
        }
        for bit in AREA.base * 8..AREA.end() * 8 {
            let mut ram = clean;
            ram[bit / 8] ^= 1 << (bit % 8);
            for i in 0..AREA.data_len() {
                assert_eq!(read_byte(&mut ram, &AREA, i), Some(0xC0 | i as u8));
            }
            assert_eq!(&ram[AREA.base..AREA.end()], &clean[AREA.base..AREA.end()]);
            assert_eq!(ram::CNT_ECC_CORR.get(&mut ram), 1, "bit {bit}");
        }
    }

    /// MBU de 2 à 8 bits adjacents (numérotation LSB d'abord et MSB d'abord) : corrigé.
    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn adjacent_mbu_up_to_8_bits_is_corrected() {
        let mut clean = [0u8; RAM_SIZE];
        for i in 0..AREA.data_len() {
            write_byte(&mut clean, &AREA, i, 0x3C ^ (i as u8));
        }
        for msb_first in [false, true] {
            for len in 2..=8 {
                for start in AREA.base * 8..AREA.end() * 8 - len + 1 {
                    let mut ram = clean;
                    for p in start..start + len {
                        let bit = if msb_first { 7 - p % 8 } else { p % 8 };
                        ram[p / 8] ^= 1 << bit;
                    }
                    for g in 0..AREA.groups {
                        scrub_group(&mut ram, &AREA, g);
                    }
                    assert_eq!(
                        &ram[AREA.base..AREA.end()],
                        &clean[AREA.base..AREA.end()],
                        "MBU {len} bits en {start}"
                    );
                }
            }
        }
    }

    /// Double erreur dans une même voie : détectée, jamais corrigée en faux, laissée en place
    /// par le scrub.
    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn double_error_in_lane_is_detected_and_left_alone() {
        let mut ram = [0u8; RAM_SIZE];
        write_byte(&mut ram, &AREA, 5, 0x77);
        let (b0, l0) = AREA.phys_bit(5, 2);
        let (b1, l1) = AREA.phys_bit(5, 9);
        ram[b0] ^= 1 << l0;
        ram[b1] ^= 1 << l1;
        let snapshot = ram;
        assert_eq!(read_byte(&mut ram, &AREA, 5), None);
        assert_eq!(scrub_group(&mut ram, &AREA, 0), 0);
        assert_eq!(ram, snapshot);
        assert!(!group_clean(&ram, &AREA, 0));
        // Les autres voies du groupe restent lisibles.
        assert_eq!(read_byte(&mut ram, &AREA, 4), Some(0));
        // Réécriture = invalidation propre.
        write_byte(&mut ram, &AREA, 5, 0);
        assert!(group_clean(&ram, &AREA, 0));
    }

    #[cfg(feature = "unprotected")]
    #[test]
    fn unprotected_reads_raw_data_bits() {
        let mut ram = [0u8; RAM_SIZE];
        write_byte(&mut ram, &AREA, 0, 0x10);
        let (b, l) = AREA.phys_bit(0, DATA_POS[0] as usize);
        ram[b] ^= 1 << l;
        assert_eq!(read_byte(&mut ram, &AREA, 0), Some(0x11));
        assert_eq!(scrub_group(&mut ram, &AREA, 0), 0);
        assert_eq!(read_byte(&mut ram, &AREA, 0), Some(0x11));
    }
}
