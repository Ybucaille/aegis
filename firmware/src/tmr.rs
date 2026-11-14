//! TMR : trois copies, vote BIT À BIT, réécriture à la lecture (SPEC §4.3 ; choix T-010).
//!
//! Disposition : une zone TMR est faite de trois BLOCS consécutifs de même taille `block`
//! (`TmrArea`). La copie `c` d'une cellule de décalage `o` dans le bloc est à
//! `base + c * block + o`. Deux copies d'une même cellule sont donc séparées par au moins
//! `block - taille >= 1` octet (vérifié à la construction des cellules) : un MBU de 2 à 4 bits
//! adjacents (au plus 2 octets consécutifs) ne touche jamais deux copies d'une même cellule.
//!
//! Vote bit à bit : `v = (a & b) | (a & c) | (b & c)`, octet par octet. Il corrige toute
//! altération d'une seule copie, et aussi deux copies altérées sur des bits DIFFÉRENTS. Il se
//! trompe seulement si deux copies ont le même bit basculé (indétectable par construction).
//!
//! « Absence de majorité » (incohérence, SPEC §4.4) : pour un octet, les trois copies sont deux à
//! deux différentes (au moins deux copies touchées : le vote a pu se tromper). Elle est
//! SIGNALÉE en posant `ram::TMR_ALARM` (écrit directement sur ses trois copies) ; la réaction
//! (`sys_reset`) appartient au FDIR (T-012). La valeur votée est utilisée et réécrite.
//!
//! Comptage : chaque octet réparé (lecture ou scrub) ajoute 1 à `ram::CNT_TMR_FIX` (saturant).
//!
//! Témoin `unprotected` (R8) : UN seul point, la constante `PROTECTED`. Sans protection, la
//! lecture renvoie la copie 0, sans vote, sans réparation ni signalement ; l'écriture écrit
//! toujours les trois copies (sans effet observable).

use crate::ram::{self, Ram};

/// Seul point de la feature `unprotected` dans ce module.
const PROTECTED: bool = !cfg!(feature = "unprotected");

/// Zone TMR : trois blocs de `block` octets à partir de `base`.
/// Invariant (vérifié à la compilation, les zones étant des `const`) :
/// `base + 3 * block <= RAM_SIZE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TmrArea {
    pub base: usize,
    pub block: usize,
}

impl TmrArea {
    pub const fn new(base: usize, block: usize) -> Self {
        assert!(
            base + 3 * block <= crate::hal::RAM_SIZE,
            "zone TMR hors RAM"
        );
        // Une cellule fait au plus 4 octets : bloc >= 5 garantit un octet d'écart entre copies.
        assert!(block >= 5, "bloc TMR trop petit");
        TmrArea { base, block }
    }
    pub const fn end(&self) -> usize {
        self.base + 3 * self.block
    }
    pub const fn storage_len(&self) -> usize {
        3 * self.block
    }
}

/// Vote majoritaire bit à bit.
pub const fn vote(a: u8, b: u8, c: u8) -> u8 {
    (a & b) | (a & c) | (b & c)
}

/// Résultat d'un vote sur un octet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteVote {
    pub value: u8,
    /// Les copies divergeaient : elles ont été réécrites.
    pub fixed: bool,
    /// Trois copies deux à deux différentes.
    pub no_majority: bool,
}

/// Vote et répare l'octet dont la copie 0 est en `off` (copies à `off + stride`,
/// `off + 2 * stride`). Précondition : `off + 2 * stride < RAM_SIZE` (garantie par la
/// construction des cellules et des zones). Sans protection : copie 0, rien d'autre.
pub fn vote_byte(ram: &mut Ram, off: usize, stride: usize) -> ByteVote {
    let a = ram[off];
    if !PROTECTED {
        return ByteVote {
            value: a,
            fixed: false,
            no_majority: false,
        };
    }
    let b = ram[off + stride];
    let c = ram[off + 2 * stride];
    if a == b && b == c {
        return ByteVote {
            value: a,
            fixed: false,
            no_majority: false,
        };
    }
    let v = vote(a, b, c);
    ram[off] = v;
    ram[off + stride] = v;
    ram[off + 2 * stride] = v;
    ByteVote {
        value: v,
        fixed: true,
        no_majority: a != b && b != c && a != c,
    }
}

/// Écrit `data` dans les trois copies.
pub fn write(ram: &mut Ram, off: usize, stride: usize, data: &[u8]) {
    for (i, &v) in data.iter().enumerate() {
        ram[off + i] = v;
        ram[off + stride + i] = v;
        ram[off + 2 * stride + i] = v;
    }
}

/// Comptabilise `fixed` octets réparés et, le cas échéant, l'absence de majorité.
/// Le compteur est lu par vote SANS comptage (pas de récursion) ; ses propres réparations sont
/// ajoutées au total.
pub fn account(ram: &mut Ram, fixed: u16, no_majority: bool) {
    if fixed > 0 {
        let s = &ram::CNT_TMR_FIX;
        let (off, stride) = (s.off(), s.tmr_stride());
        let mut buf = [0u8; 2];
        let mut extra = 0u16;
        for (i, b) in buf.iter_mut().enumerate() {
            let r = vote_byte(ram, off + i, stride);
            *b = r.value;
            extra += r.fixed as u16;
        }
        let n = u16::from_be_bytes(buf)
            .saturating_add(fixed)
            .saturating_add(extra);
        write(ram, off, stride, &n.to_be_bytes());
    }
    if no_majority {
        let s = &ram::TMR_ALARM;
        write(ram, s.off(), s.tmr_stride(), &[1]);
    }
}

/// Lecture d'une cellule TMR de `out.len()` octets : vote, réparation, comptage, signalement.
pub fn read(ram: &mut Ram, off: usize, stride: usize, out: &mut [u8]) {
    let mut fixed = 0u16;
    let mut nm = false;
    for (i, o) in out.iter_mut().enumerate() {
        let r = vote_byte(ram, off + i, stride);
        *o = r.value;
        fixed += r.fixed as u16;
        nm |= r.no_majority;
    }
    account(ram, fixed, nm);
}

/// Scrub d'un octet de zone (position `pos` dans le bloc). Renvoie vrai s'il a été réparé.
pub fn scrub_byte(ram: &mut Ram, area: &TmrArea, pos: usize) -> bool {
    if pos >= area.block {
        return false;
    }
    let r = vote_byte(ram, area.base + pos, area.block);
    account(ram, r.fixed as u16, r.no_majority);
    r.fixed
}

/// Vrai si les trois copies de chaque octet de la zone sont identiques (diagnostic, tests).
pub fn area_consistent(ram: &Ram, area: &TmrArea) -> bool {
    (0..area.block).all(|i| {
        let o = area.base + i;
        ram[o] == ram[o + area.block] && ram[o] == ram[o + 2 * area.block]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::RAM_SIZE;

    // Zone de test dans la région `unused` (les compteurs vivent dans `fdir`).
    const AREA: TmrArea = TmrArea::new(940, 8);

    #[test]
    fn vote_truth_table() {
        assert_eq!(vote(0xF0, 0xF0, 0x0F), 0xF0);
        assert_eq!(vote(0xF0, 0x0F, 0xF0), 0xF0);
        assert_eq!(vote(0x0F, 0xF0, 0xF0), 0xF0);
        // Bit à bit : deux copies altérées sur des bits différents -> valeur juste.
        assert_eq!(vote(0x5A ^ 0x01, 0x5A ^ 0x80, 0x5A), 0x5A);
        for a in 0..=255u8 {
            assert_eq!(vote(a, a, a), a);
        }
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn one_copy_altered_gives_right_value_and_is_repaired() {
        for copy in 0..3 {
            for bit in 0..32 {
                let mut ram = [0u8; RAM_SIZE];
                write(&mut ram, AREA.base + 2, AREA.block, &[1, 2, 3, 4]);
                ram[AREA.base + copy * AREA.block + 2 + bit / 8] ^= 1 << (bit % 8);
                let mut out = [0u8; 4];
                read(&mut ram, AREA.base + 2, AREA.block, &mut out);
                assert_eq!(out, [1, 2, 3, 4]);
                assert!(area_consistent(&ram, &AREA));
                assert_eq!(ram::CNT_TMR_FIX.get(&mut ram), 1);
                assert_eq!(ram::TMR_ALARM.get(&mut ram), 0);
            }
        }
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn two_copies_altered_differently_is_signalled() {
        let mut ram = [0u8; RAM_SIZE];
        let off = AREA.base + 1;
        write(&mut ram, off, AREA.block, &[0x5A]);
        ram[off] ^= 0x01;
        ram[off + AREA.block] ^= 0x02;
        let mut out = [0u8; 1];
        read(&mut ram, off, AREA.block, &mut out);
        // Bit à bit, la valeur reste juste ici ; l'incohérence est quand même signalée.
        assert_eq!(out, [0x5A]);
        assert_eq!(ram::TMR_ALARM.get(&mut ram), 1);
        assert!(area_consistent(&ram, &AREA));
        // Même bit basculé sur deux copies : limite connue, vote faux, pas d'alarme possible
        // (deux copies identiques = majorité apparente).
        let mut ram = [0u8; RAM_SIZE];
        write(&mut ram, off, AREA.block, &[0x5A]);
        ram[off] ^= 0x01;
        ram[off + 2 * AREA.block] ^= 0x01;
        read(&mut ram, off, AREA.block, &mut out);
        assert_eq!(out, [0x5B]);
        assert_eq!(ram::TMR_ALARM.get(&mut ram), 0);
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn counter_saturates_and_counts_its_own_repair() {
        let mut ram = [0u8; RAM_SIZE];
        let s = &ram::CNT_TMR_FIX;
        s.set(&mut ram, 10);
        ram[s.off() + 1] ^= 0x04; // copie 0 du compteur altérée
        account(&mut ram, 3, false);
        assert_eq!(s.get(&mut ram), 14);
        s.set(&mut ram, u16::MAX - 1);
        account(&mut ram, 5, false);
        assert_eq!(s.get(&mut ram), u16::MAX);
    }

    #[cfg(not(feature = "unprotected"))]
    #[test]
    fn scrub_byte_repairs_every_bit_of_every_copy() {
        for bit in AREA.base * 8..AREA.end() * 8 {
            let mut ram = [0u8; RAM_SIZE];
            for i in 0..AREA.block {
                write(&mut ram, AREA.base + i, AREA.block, &[0xA0 | i as u8]);
            }
            let clean = ram;
            ram[bit / 8] ^= 1 << (bit % 8);
            let fixed: usize = (0..AREA.block)
                .filter(|&p| scrub_byte(&mut ram, &AREA, p))
                .count();
            assert_eq!(fixed, 1);
            assert_eq!(&ram[AREA.base..AREA.end()], &clean[AREA.base..AREA.end()]);
        }
    }

    #[cfg(feature = "unprotected")]
    #[test]
    fn unprotected_reads_copy_zero_without_repair() {
        let mut ram = [0u8; RAM_SIZE];
        write(&mut ram, AREA.base, AREA.block, &[7]);
        ram[AREA.base] = 9;
        let mut out = [0u8; 1];
        read(&mut ram, AREA.base, AREA.block, &mut out);
        assert_eq!(out, [9]);
        assert!(!area_consistent(&ram, &AREA));
        assert!(!scrub_byte(&mut ram, &AREA, 0));
    }
}
