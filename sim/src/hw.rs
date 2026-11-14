//! Matériel simulé : registres, NVM, watchdog à clés alternées, files radio, cause de reset.
//! Implémente `firmware::Hal` (SPEC §3). Aucune physique ici : `World` pousse les valeurs
//! de capteurs échantillonnées et l'état de la liaison, `Hw` les restitue au firmware.

use firmware::{Hal, HalError, Reg, ResetCause, SensorId, MAX_FRAME, NVM_SIZE};
use std::collections::VecDeque;

/// Première clé acceptée après un reset (décision T-006), puis alternance.
pub const KEY_FIRST: u8 = 0xA5;
pub const KEY_SECOND: u8 = 0x5A;
/// Taille maximale de la file montante (trames au-delà : refusées).
pub const UPLINK_CAP: usize = 16;
/// Valeur de `WdgTimeout` au reset matériel (annexe A : 10 s).
pub const WDG_TIMEOUT_DEFAULT: u16 = 10;
/// `LoadSwitches` au reset matériel : seule la ligne Radio est alimentée.
pub const LOAD_SWITCHES_DEFAULT: u16 = 0b010;
/// `WdgCtrl` au reset matériel : bit 0 = watchdog actif (valeur de `firmware::WDG_CTRL_VALUE`).
pub const WDG_CTRL_DEFAULT: u16 = 0x0001;
/// `RadioCfg` au reset matériel : bit 0 = RX, bit 1 = TX (valeur de `firmware::RADIO_CFG_VALUE`).
pub const RADIO_CFG_DEFAULT: u16 = 0x0003;
pub const WDG_CTRL_ENABLE: u16 = 0x0001;
pub const RADIO_CFG_RX: u16 = 0x0001;
pub const RADIO_CFG_TX: u16 = 0x0002;

/// Défaut d'un capteur (F5) : le matériel renvoie autre chose que la mesure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorFault {
    /// Figé à une valeur (permanent).
    Frozen(i16),
    /// La lecture échoue (`HalError::Io`).
    Err,
    /// Valeur hors plage renvoyée à la place de la mesure.
    Value(i16),
}

#[derive(Clone, Debug)]
pub struct Hw {
    regs: [u16; 4],
    nvm: [u8; NVM_SIZE],
    wdg_counter: u32,
    wdg_next_key: u8,
    cause: ResetCause,
    pending_reset: Option<ResetCause>,
    sensors: [i16; 9],
    link_up: bool,
    uplink: VecDeque<Vec<u8>>,
    tx_staged: Vec<Vec<u8>>,
    nvm_writes: u32,
    rx_delivered: u32,
    /// Trames montantes réellement remises au firmware (instrument de l'oracle S5, vidé par `World`).
    delivered: Vec<Vec<u8>>,
    /// Watchdog mort (D1) : ne décompte plus, n'expire jamais. Matériel : survit aux resets.
    wdg_dead: bool,
    sensor_faults: [Option<SensorFault>; 9],
    /// Bit `Line as u8` : ligne détruite (circuit ouvert, ne rend plus aucun service).
    dead_lines: u8,
}

impl Default for Hw {
    fn default() -> Self {
        Self::new()
    }
}

impl Hw {
    /// NVM à 0xFF (mémoire vierge), reste en état de reset matériel, cause `PowerOn`.
    pub fn new() -> Hw {
        let mut hw = Hw {
            regs: [0; 4],
            nvm: [0xFF; NVM_SIZE],
            wdg_counter: 0,
            wdg_next_key: KEY_FIRST,
            cause: ResetCause::PowerOn,
            pending_reset: None,
            sensors: [0; 9],
            link_up: false,
            uplink: VecDeque::new(),
            tx_staged: Vec::new(),
            nvm_writes: 0,
            rx_delivered: 0,
            delivered: Vec::new(),
            wdg_dead: false,
            sensor_faults: [None; 9],
            dead_lines: 0,
        };
        hw.hw_reset(ResetCause::PowerOn);
        hw
    }

    /// Reset matériel : registres par défaut, watchdog réarmé, NVM conservée.
    /// (La RAM du firmware est effacée par `Firmware::boot`.)
    pub fn hw_reset(&mut self, cause: ResetCause) {
        self.regs = [0; 4];
        self.regs[Reg::LoadSwitches as usize] = LOAD_SWITCHES_DEFAULT;
        self.regs[Reg::WdgTimeout as usize] = WDG_TIMEOUT_DEFAULT;
        self.regs[Reg::WdgCtrl as usize] = WDG_CTRL_DEFAULT;
        self.regs[Reg::RadioCfg as usize] = RADIO_CFG_DEFAULT;
        self.wdg_counter = WDG_TIMEOUT_DEFAULT as u32;
        self.wdg_next_key = KEY_FIRST;
        self.cause = cause;
        self.pending_reset = None;
        self.tx_staged.clear();
    }

    /// Un tick de watchdog ; `true` si le compteur atteint 0 (expiration).
    /// Watchdog désactivé (bit 0 de `WdgCtrl` à 0) : le compteur est gelé, jamais d'expiration.
    pub fn wdg_tick(&mut self) -> bool {
        if self.wdg_dead {
            return false;
        }
        if self.regs[Reg::WdgCtrl as usize] & WDG_CTRL_ENABLE == 0 {
            return false;
        }
        self.wdg_counter = self.wdg_counter.saturating_sub(1);
        self.wdg_counter == 0
    }

    /// Tue le watchdog (D1) : plus aucune expiration, y compris après un reset.
    pub fn set_wdg_dead(&mut self, dead: bool) {
        self.wdg_dead = dead;
    }

    pub fn wdg_dead(&self) -> bool {
        self.wdg_dead
    }

    /// Défaut capteur permanent (F5).
    pub fn set_sensor_fault(&mut self, id: SensorId, f: Option<SensorFault>) {
        self.sensor_faults[id as usize] = f;
    }

    pub fn sensor_fault(&self, id: SensorId) -> Option<SensorFault> {
        self.sensor_faults[id as usize]
    }

    /// Dernière mesure vraie poussée par `World` (hors défaut capteur).
    pub fn sensor_raw(&self, id: SensorId) -> i16 {
        self.sensors[id as usize]
    }

    /// Détruit une ligne (latch-up non coupé) : elle ne rend plus de service, définitivement.
    pub fn set_line_dead(&mut self, l: firmware::Line) {
        self.dead_lines |= 1 << (l as u8);
    }

    pub fn line_dead(&self, l: firmware::Line) -> bool {
        self.dead_lines & (1 << (l as u8)) != 0
    }

    /// Première trame montante en attente (instrument de test).
    pub fn uplink_peek(&self) -> Option<&[u8]> {
        self.uplink.front().map(|v| v.as_slice())
    }

    pub fn wdg_counter(&self) -> u32 {
        self.wdg_counter
    }

    /// Reset demandé par le firmware (`sys_reset`) ou le simulateur (panique), traité en fin de step.
    pub fn take_pending_reset(&mut self) -> Option<ResetCause> {
        self.pending_reset.take()
    }

    pub fn set_pending_reset(&mut self, c: ResetCause) {
        self.pending_reset = Some(c);
    }

    pub fn set_sensors(&mut self, v: [i16; 9]) {
        self.sensors = v;
    }

    pub fn set_link_up(&mut self, up: bool) {
        self.link_up = up;
    }

    /// Valeur brute d'un registre (vue du simulateur, hors Hal).
    pub fn reg(&self, r: Reg) -> u16 {
        self.regs[r as usize]
    }

    /// Écriture directe d'un registre par le simulateur (injection : F3/F7).
    pub fn set_reg(&mut self, r: Reg, v: u16) {
        self.regs[r as usize] = v;
    }

    pub fn nvm(&self) -> &[u8; NVM_SIZE] {
        &self.nvm
    }

    pub fn nvm_mut(&mut self) -> &mut [u8; NVM_SIZE] {
        &mut self.nvm
    }

    pub fn nvm_writes(&self) -> u32 {
        self.nvm_writes
    }

    pub fn rx_delivered(&self) -> u32 {
        self.rx_delivered
    }

    /// Dépose une trame montante (du sol). `false` si trop longue ou file pleine.
    pub fn push_uplink(&mut self, frame: &[u8]) -> bool {
        if frame.len() > MAX_FRAME || frame.is_empty() || self.uplink.len() >= UPLINK_CAP {
            return false;
        }
        self.uplink.push_back(frame.to_vec());
        true
    }

    pub fn uplink_len(&self) -> usize {
        self.uplink.len()
    }

    /// Trames émises pendant le tick (vidées par `World`).
    pub fn drain_tx(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.tx_staged)
    }

    /// Une émission a-t-elle eu lieu ce tick (charge 3,0 W) ?
    /// Vide le journal des trames montantes remises au firmware.
    pub fn drain_delivered(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.delivered)
    }

    pub fn tx_this_tick(&self) -> bool {
        !self.tx_staged.is_empty()
    }

    fn line_powered(&self, line: firmware::Line) -> bool {
        self.regs[Reg::LoadSwitches as usize] & line.mask() != 0 && !self.line_dead(line)
    }
}

impl Hal for Hw {
    fn sensor(&mut self, id: SensorId) -> Result<i16, HalError> {
        match self.sensor_faults[id as usize] {
            None => Ok(self.sensors[id as usize]),
            Some(SensorFault::Frozen(v)) | Some(SensorFault::Value(v)) => Ok(v),
            Some(SensorFault::Err) => Err(HalError::Io),
        }
    }

    fn reg_read(&mut self, r: Reg) -> u16 {
        self.regs[r as usize]
    }

    fn reg_write(&mut self, r: Reg, v: u16) {
        self.regs[r as usize] = v;
    }

    /// Clé attendue → rechargement à `WdgTimeout` et alternance ; clé fausse ou répétée → rien.
    fn wdg_kick(&mut self, key: u8) {
        if key == self.wdg_next_key {
            self.wdg_counter = self.regs[Reg::WdgTimeout as usize] as u32;
            self.wdg_next_key = if key == KEY_FIRST {
                KEY_SECOND
            } else {
                KEY_FIRST
            };
        }
    }

    /// Reçoit une trame montante, seulement pendant la passe et ligne Radio alimentée.
    fn radio_recv(&mut self, buf: &mut [u8; MAX_FRAME]) -> Option<usize> {
        if !self.link_up
            || !self.line_powered(firmware::Line::Radio)
            || self.regs[Reg::RadioCfg as usize] & RADIO_CFG_RX == 0
        {
            return None;
        }
        let f = self.uplink.pop_front()?;
        buf[..f.len()].copy_from_slice(&f);
        self.rx_delivered += 1;
        self.delivered.push(f.clone());
        Some(f.len())
    }

    /// Une trame par tick (3,0 W pendant 1 s) ; la seconde du même tick renvoie `Busy`.
    fn radio_send(&mut self, frame: &[u8]) -> Result<(), HalError> {
        if !self.line_powered(firmware::Line::Radio) {
            return Err(HalError::NotPowered);
        }
        if self.regs[Reg::RadioCfg as usize] & RADIO_CFG_TX == 0 {
            return Err(HalError::Io);
        }
        if frame.is_empty() || frame.len() > MAX_FRAME {
            return Err(HalError::OutOfRange);
        }
        if !self.tx_staged.is_empty() {
            return Err(HalError::Busy);
        }
        self.tx_staged.push(frame.to_vec());
        Ok(())
    }

    fn nvm_read(&mut self, off: u16, buf: &mut [u8]) -> Result<(), HalError> {
        let o = off as usize;
        let end = o.checked_add(buf.len()).ok_or(HalError::OutOfRange)?;
        if end > NVM_SIZE {
            return Err(HalError::OutOfRange);
        }
        buf.copy_from_slice(&self.nvm[o..end]);
        Ok(())
    }

    fn nvm_write(&mut self, off: u16, data: &[u8]) -> Result<(), HalError> {
        let o = off as usize;
        let end = o.checked_add(data.len()).ok_or(HalError::OutOfRange)?;
        if end > NVM_SIZE {
            return Err(HalError::OutOfRange);
        }
        self.nvm[o..end].copy_from_slice(data);
        self.nvm_writes += 1;
        Ok(())
    }

    fn reset_cause(&mut self) -> ResetCause {
        self.cause
    }

    fn sys_reset(&mut self) {
        // Une panique déjà enregistrée garde la priorité.
        if self.pending_reset.is_none() {
            self.pending_reset = Some(ResetCause::Commanded);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchdog_keys_alternate_and_bad_keys_are_ignored() {
        let mut hw = Hw::new();
        for _ in 0..5 {
            hw.wdg_tick();
        }
        assert_eq!(hw.wdg_counter(), 5);
        hw.wdg_kick(KEY_SECOND); // fausse en premier
        assert_eq!(hw.wdg_counter(), 5);
        hw.wdg_kick(0x00);
        assert_eq!(hw.wdg_counter(), 5);
        hw.wdg_kick(KEY_FIRST);
        assert_eq!(hw.wdg_counter(), 10);
        hw.wdg_tick();
        hw.wdg_kick(KEY_FIRST); // répétée
        assert_eq!(hw.wdg_counter(), 9);
        hw.wdg_kick(KEY_SECOND);
        assert_eq!(hw.wdg_counter(), 10);
    }

    #[test]
    fn kick_reloads_from_timeout_register() {
        let mut hw = Hw::new();
        hw.reg_write(Reg::WdgTimeout, 25);
        hw.wdg_kick(KEY_FIRST);
        assert_eq!(hw.wdg_counter(), 25);
    }

    #[test]
    fn nvm_bounds_and_persistence_over_reset() {
        let mut hw = Hw::new();
        assert_eq!(hw.nvm_write(250, &[1; 7]), Err(HalError::OutOfRange));
        hw.nvm_write(10, &[1, 2, 3]).unwrap();
        hw.hw_reset(ResetCause::Watchdog);
        let mut b = [0u8; 3];
        hw.nvm_read(10, &mut b).unwrap();
        assert_eq!(b, [1, 2, 3]);
        assert_eq!(hw.reset_cause(), ResetCause::Watchdog);
        assert_eq!(hw.reg(Reg::WdgTimeout), WDG_TIMEOUT_DEFAULT);
    }

    #[test]
    fn wdg_ctrl_and_radio_cfg_are_enforced() {
        let mut hw = Hw::new();
        hw.reg_write(Reg::WdgCtrl, 0);
        for _ in 0..50 {
            assert!(!hw.wdg_tick(), "watchdog désactivé : jamais d'expiration");
        }
        hw.reg_write(Reg::WdgCtrl, WDG_CTRL_ENABLE);
        let mut expired = false;
        for _ in 0..50 {
            expired |= hw.wdg_tick();
        }
        assert!(expired);
        hw.reg_write(Reg::RadioCfg, RADIO_CFG_RX);
        assert_eq!(hw.radio_send(&[1]), Err(HalError::Io));
        hw.reg_write(Reg::RadioCfg, RADIO_CFG_TX);
        hw.set_link_up(true);
        hw.push_uplink(&[1]);
        let mut b = [0u8; MAX_FRAME];
        assert!(hw.radio_recv(&mut b).is_none());
        hw.reg_write(Reg::RadioCfg, RADIO_CFG_RX);
        assert!(hw.radio_recv(&mut b).is_some());
    }

    #[test]
    fn radio_send_needs_line_and_is_one_per_tick() {
        let mut hw = Hw::new();
        assert_eq!(hw.radio_send(&[1]), Ok(()));
        assert_eq!(hw.radio_send(&[2]), Err(HalError::Busy));
        assert_eq!(hw.drain_tx().len(), 1);
        hw.reg_write(Reg::LoadSwitches, 0);
        assert_eq!(hw.radio_send(&[1]), Err(HalError::NotPowered));
    }
}
