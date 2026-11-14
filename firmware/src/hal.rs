//! Interface firmware <-> matériel (SPEC §3). Implémentée par le simulateur.
//!
//! Les signatures du trait `Hal` sont celles de la SPEC, à l'identique. Les ajouts (conversions
//! `u8`, listes `ALL`) sont des aides sans effet sur le contrat.

/// Taille maximale d'une trame radio (en-tête 8 + charge utile 48 + CRC 4).
pub const MAX_FRAME: usize = 60;
/// Taille de la mémoire non volatile, en octets. Persiste aux resets.
pub const NVM_SIZE: usize = 256;
/// Taille de l'image RAM du firmware, en octets. Perdue à tout reset.
pub const RAM_SIZE: usize = 1024;

/// Capteurs. Unités : mV, mA, 0,1 °C.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SensorId {
    VBattA = 0,
    VBattB = 1,
    ISolar = 2,
    ILinePayload = 3,
    ILineRadio = 4,
    ILineHeater = 5,
    TempBattA = 6,
    TempBattB = 7,
    TempObc = 8,
}

impl SensorId {
    /// Tous les capteurs, dans l'ordre des discriminants (index = `id as usize`).
    pub const ALL: [SensorId; 9] = [
        SensorId::VBattA,
        SensorId::VBattB,
        SensorId::ISolar,
        SensorId::ILinePayload,
        SensorId::ILineRadio,
        SensorId::ILineHeater,
        SensorId::TempBattA,
        SensorId::TempBattB,
        SensorId::TempObc,
    ];
}

/// Registres matériels. Réécrits à chaque cycle par `health` (sauf `LoadSwitches`, relu et comparé).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Reg {
    LoadSwitches = 0,
    WdgTimeout = 1,
    WdgCtrl = 2,
    RadioCfg = 3,
}

impl Reg {
    pub const ALL: [Reg; 4] = [
        Reg::LoadSwitches,
        Reg::WdgTimeout,
        Reg::WdgCtrl,
        Reg::RadioCfg,
    ];
}

/// Lignes d'alimentation commutées : bit n de `Reg::LoadSwitches` (1 = alimentée).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Line {
    Payload = 0,
    Radio = 1,
    Heater = 2,
}

impl Line {
    pub const ALL: [Line; 3] = [Line::Payload, Line::Radio, Line::Heater];

    /// Masque du bit de la ligne dans `Reg::LoadSwitches`.
    pub const fn mask(self) -> u16 {
        1 << (self as u16)
    }

    /// Conversion depuis l'argument `u8` d'une TC (`None` si hors bornes).
    pub const fn from_u8(v: u8) -> Option<Line> {
        match v {
            0 => Some(Line::Payload),
            1 => Some(Line::Radio),
            2 => Some(Line::Heater),
            _ => None,
        }
    }
}

/// Cause du dernier reset, telle que vue par le firmware. Codée sur un octet dans la balise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ResetCause {
    PowerOn = 0,
    Watchdog = 1,
    Brownout = 2,
    Commanded = 3,
    Panic = 4,
    Unknown = 5,
}

impl ResetCause {
    /// Décodage total : toute valeur inconnue donne `Unknown`.
    pub const fn from_u8(v: u8) -> ResetCause {
        match v {
            0 => ResetCause::PowerOn,
            1 => ResetCause::Watchdog,
            2 => ResetCause::Brownout,
            3 => ResetCause::Commanded,
            4 => ResetCause::Panic,
            _ => ResetCause::Unknown,
        }
    }
}

/// Erreurs d'accès au matériel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HalError {
    NotPowered,
    OutOfRange,
    Busy,
    Io,
}

/// Accès au matériel (simulé). Signatures figées par la SPEC §3.
pub trait Hal {
    fn sensor(&mut self, id: SensorId) -> Result<i16, HalError>;
    fn reg_read(&mut self, r: Reg) -> u16;
    fn reg_write(&mut self, r: Reg, v: u16);
    fn wdg_kick(&mut self, key: u8);
    /// Une trame par appel ; renvoie sa longueur.
    fn radio_recv(&mut self, buf: &mut [u8; MAX_FRAME]) -> Option<usize>;
    fn radio_send(&mut self, frame: &[u8]) -> Result<(), HalError>;
    fn nvm_read(&mut self, off: u16, buf: &mut [u8]) -> Result<(), HalError>;
    fn nvm_write(&mut self, off: u16, data: &[u8]) -> Result<(), HalError>;
    fn reset_cause(&mut self) -> ResetCause;
    /// Prend effet à la fin du step courant.
    fn sys_reset(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensor_all_matches_discriminants() {
        for (i, s) in SensorId::ALL.iter().enumerate() {
            assert_eq!(*s as usize, i);
        }
    }

    #[test]
    fn line_masks_and_conversion() {
        assert_eq!(Line::Payload.mask(), 0b001);
        assert_eq!(Line::Radio.mask(), 0b010);
        assert_eq!(Line::Heater.mask(), 0b100);
        for l in Line::ALL {
            assert_eq!(Line::from_u8(l as u8), Some(l));
        }
        assert_eq!(Line::from_u8(3), None);
        assert_eq!(Line::from_u8(0xFF), None);
    }

    #[test]
    fn reset_cause_roundtrip_and_total_decoding() {
        for v in 0u8..=5 {
            assert_eq!(ResetCause::from_u8(v) as u8, v);
        }
        for v in 6u8..=255 {
            assert_eq!(ResetCause::from_u8(v), ResetCause::Unknown);
        }
    }
}
