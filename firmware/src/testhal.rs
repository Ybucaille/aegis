//! `MockHal` : matériel factice pour les tests du firmware (compilé seulement en test).
//!
//! Capteurs scénarisables, registres, journal des rechargements du watchdog, files radio,
//! NVM en mémoire. Bruit optionnel déterministe (graine fournie par le test et affichée).
//! Les `#[cfg(test)]` redondants sur les imports `std` servent au contrôle textuel de
//! `scripts/ci.sh` (pas de `std` hors code de test).

#[cfg(test)]
extern crate std;
#[cfg(test)]
use std::collections::VecDeque;
#[cfg(test)]
use std::vec::Vec;

use crate::hal::{Hal, HalError, Reg, ResetCause, SensorId, MAX_FRAME, NVM_SIZE};
use crate::proto::{self, Ack, Beacon, Command, FrameType};

/// PRNG de test (xorshift64*), graine non nulle.
pub struct TestRng(u64);

impl TestRng {
    pub fn new(seed: u64) -> Self {
        TestRng(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Entier uniforme dans `[0, n)` (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

pub struct MockHal {
    /// Valeur renvoyée par chaque capteur (index = `SensorId as usize`).
    pub sensors: [Result<i16, HalError>; 9],
    /// Amplitude du bruit ajouté aux capteurs `Ok` (0 = aucun).
    pub noise: i16,
    pub rng: TestRng,
    pub regs: [u16; 4],
    pub reg_writes: usize,
    pub kicks: Vec<u8>,
    pub rx: VecDeque<Vec<u8>>,
    pub recv_calls: usize,
    pub sent: Vec<Vec<u8>>,
    pub send_result: Result<(), HalError>,
    pub nvm: [u8; NVM_SIZE],
    pub reset_cause: ResetCause,
    pub resets: usize,
}

impl MockHal {
    /// Satellite sain : batterie à 7800 mV, 20 °C, sans bruit.
    pub fn nominal() -> Self {
        let mut s = [Ok(0i16); 9];
        s[SensorId::VBattA as usize] = Ok(7800);
        s[SensorId::VBattB as usize] = Ok(7800);
        s[SensorId::ISolar as usize] = Ok(500);
        s[SensorId::ILinePayload as usize] = Ok(250);
        s[SensorId::ILineRadio as usize] = Ok(60);
        s[SensorId::ILineHeater as usize] = Ok(0);
        s[SensorId::TempBattA as usize] = Ok(200);
        s[SensorId::TempBattB as usize] = Ok(200);
        s[SensorId::TempObc as usize] = Ok(250);
        MockHal {
            sensors: s,
            noise: 0,
            rng: TestRng::new(1),
            regs: [0; 4],
            reg_writes: 0,
            kicks: Vec::new(),
            rx: VecDeque::new(),
            recv_calls: 0,
            sent: Vec::new(),
            send_result: Ok(()),
            nvm: [0xFF; NVM_SIZE],
            reset_cause: ResetCause::PowerOn,
            resets: 0,
        }
    }

    pub fn set(&mut self, id: SensorId, v: Result<i16, HalError>) {
        self.sensors[id as usize] = v;
    }

    pub fn set_vbatt(&mut self, mv: i16) {
        self.set(SensorId::VBattA, Ok(mv));
        self.set(SensorId::VBattB, Ok(mv));
    }

    pub fn set_temp(&mut self, dc: i16) {
        self.set(SensorId::TempBattA, Ok(dc));
        self.set(SensorId::TempBattB, Ok(dc));
    }

    /// Met en file une trame TC valide.
    pub fn push_tc(&mut self, seq: u32, cmd: &Command) -> Vec<u8> {
        let mut out = [0u8; MAX_FRAME];
        let n = proto::encode_tc(seq, cmd, &mut out);
        let v = out[..n].to_vec();
        self.rx.push_back(v.clone());
        v
    }

    /// ACK émis, dans l'ordre.
    pub fn acks(&self) -> Vec<Ack> {
        self.sent
            .iter()
            .filter_map(|f| {
                let fr = proto::decode_frame(f).ok()?;
                (fr.ftype == FrameType::Ack).then(|| Ack::decode(fr.payload).ok())?
            })
            .collect()
    }

    /// Balises émises, dans l'ordre.
    pub fn beacons(&self) -> Vec<Beacon> {
        self.sent
            .iter()
            .filter_map(|f| {
                let fr = proto::decode_frame(f).ok()?;
                (fr.ftype == FrameType::Beacon).then(|| Beacon::decode(fr.payload).ok())?
            })
            .collect()
    }

    pub fn line_on(&self, mask: u16) -> bool {
        self.regs[Reg::LoadSwitches as usize] & mask != 0
    }
}

impl Hal for MockHal {
    fn sensor(&mut self, id: SensorId) -> Result<i16, HalError> {
        let v = self.sensors[id as usize]?;
        if self.noise == 0 {
            return Ok(v);
        }
        let span = 2 * self.noise as u64 + 1;
        let d = self.rng.below(span) as i16 - self.noise;
        Ok(v.saturating_add(d))
    }
    fn reg_read(&mut self, r: Reg) -> u16 {
        self.regs[r as usize]
    }
    fn reg_write(&mut self, r: Reg, v: u16) {
        self.reg_writes += 1;
        self.regs[r as usize] = v;
    }
    fn wdg_kick(&mut self, key: u8) {
        self.kicks.push(key);
    }
    fn radio_recv(&mut self, buf: &mut [u8; MAX_FRAME]) -> Option<usize> {
        self.recv_calls += 1;
        let f = self.rx.pop_front()?;
        let n = f.len().min(MAX_FRAME);
        buf[..n].copy_from_slice(&f[..n]);
        Some(n)
    }
    fn radio_send(&mut self, frame: &[u8]) -> Result<(), HalError> {
        self.send_result?;
        self.sent.push(frame.to_vec());
        Ok(())
    }
    fn nvm_read(&mut self, off: u16, buf: &mut [u8]) -> Result<(), HalError> {
        let o = off as usize;
        let src = self.nvm.get(o..o + buf.len()).ok_or(HalError::OutOfRange)?;
        buf.copy_from_slice(src);
        Ok(())
    }
    fn nvm_write(&mut self, off: u16, data: &[u8]) -> Result<(), HalError> {
        let o = off as usize;
        let dst = self
            .nvm
            .get_mut(o..o + data.len())
            .ok_or(HalError::OutOfRange)?;
        dst.copy_from_slice(data);
        Ok(())
    }
    fn reset_cause(&mut self) -> ResetCause {
        self.reset_cause
    }
    fn sys_reset(&mut self) {
        self.resets += 1;
    }
}
