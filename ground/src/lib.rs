//! Ground station simulée : décodage de la télémétrie, opérateur automatique, file de TC.
//!
//! Tout le codage/décodage passe par `firmware::proto` (aucune réimplémentation du CRC ni du
//! format). Le sol ne contient volontairement aucune logique de récupération : en politique
//! `Auto`, il se contente de `Ping` (début de passe puis toutes les 60 s).

use std::collections::VecDeque;

use firmware::proto::{self, Ack, Beacon, Command, DecodeError, FrameType, ANTI_REPLAY_WINDOW};
use firmware::MAX_FRAME;

/// Période des `Ping` en passe (s).
pub const PING_PERIOD_S: u32 = 60;
/// Taille maximale de la file de TC.
pub const MAX_QUEUE: usize = 32;
/// Historique borné des balises.
pub const BEACON_HISTORY: usize = 256;
/// Historique borné des ACK.
pub const ACK_HISTORY: usize = 64;
/// Historique borné des charges EVENTS.
pub const EVENTS_HISTORY: usize = 16;

/// Politique de l'opérateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// `Ping` au début de chaque passe puis toutes les 60 s ; aucune commande de récupération.
    /// Les commandes mises en file via `queue` (dashboard) sont tout de même émises.
    Auto,
    /// File seulement.
    Manual,
}

/// Compteurs de trames rejetées, par motif.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RejectCounts {
    pub bad_sync: u32,
    pub bad_type: u32,
    pub bad_len: u32,
    pub bad_crc: u32,
    /// Trame valide mais de type inattendu en descente (TC) ou charge utile invalide.
    pub bad_payload: u32,
}

impl RejectCounts {
    pub fn total(&self) -> u32 {
        self.bad_sync + self.bad_type + self.bad_len + self.bad_crc + self.bad_payload
    }
}

/// Vue de la station sol (lue par le dashboard et l'oracle).
#[derive(Debug, Clone, Default)]
pub struct GroundView {
    /// Dernière balise valide et sa date de réception.
    pub last_beacon: Option<(u32, Beacon)>,
    /// Historique borné des balises `(t, balise)`, la plus ancienne en tête.
    pub beacons: VecDeque<(u32, Beacon)>,
    /// ACK reçus `(t, ack)`, historique borné.
    pub acks: VecDeque<(u32, Ack)>,
    /// Charges utiles EVENTS reçues `(t, octets)`, historique borné.
    pub events: VecDeque<(u32, Vec<u8>)>,
    pub rejects: RejectCounts,
    pub frames_ok: u32,
    pub tc_sent: u32,
    /// Prochain numéro de séquence TC.
    pub next_tc_seq: u32,
}

/// Station sol.
#[derive(Debug, Clone)]
pub struct Ground {
    policy: Policy,
    view: GroundView,
    queue: VecDeque<Command>,
    was_in_view: bool,
    next_ping_t: u32,
}

impl Ground {
    pub fn new(policy: Policy) -> Ground {
        Ground {
            policy,
            view: GroundView {
                next_tc_seq: 1,
                ..GroundView::default()
            },
            queue: VecDeque::new(),
            was_in_view: false,
            next_ping_t: 0,
        }
    }

    /// Traite une trame descendante ; toute trame invalide est comptée et ignorée.
    pub fn on_frame(&mut self, t: u32, bytes: &[u8]) {
        let frame = match proto::decode_frame(bytes) {
            Ok(f) => f,
            Err(e) => {
                let r = &mut self.view.rejects;
                match e {
                    DecodeError::BadSync => r.bad_sync += 1,
                    DecodeError::BadType => r.bad_type += 1,
                    DecodeError::BadLen => r.bad_len += 1,
                    DecodeError::BadCrc => r.bad_crc += 1,
                }
                return;
            }
        };
        match frame.ftype {
            FrameType::Beacon => match Beacon::decode(frame.payload) {
                Ok(b) => {
                    self.view.frames_ok += 1;
                    self.view.last_beacon = Some((t, b));
                    push_bounded(&mut self.view.beacons, (t, b), BEACON_HISTORY);
                    self.resync(b.last_tc_seq);
                }
                Err(_) => self.view.rejects.bad_payload += 1,
            },
            FrameType::Ack => match Ack::decode(frame.payload) {
                Ok(a) => {
                    self.view.frames_ok += 1;
                    push_bounded(&mut self.view.acks, (t, a), ACK_HISTORY);
                }
                Err(_) => self.view.rejects.bad_payload += 1,
            },
            FrameType::Events => {
                self.view.frames_ok += 1;
                push_bounded(
                    &mut self.view.events,
                    (t, frame.payload.to_vec()),
                    EVENTS_HISTORY,
                );
            }
            // Une TC n'a rien à faire en descente.
            FrameType::Tc => self.view.rejects.bad_payload += 1,
        }
    }

    /// Resynchronise le SEQ sur le `last_tc_seq` annoncé : vers le haut si le satellite a
    /// avancé ; vers le bas seulement si notre SEQ sortirait de la fenêtre anti-rejeu (NVM du
    /// satellite revenue en arrière) — sinon toutes nos TC seraient rejetées indéfiniment.
    fn resync(&mut self, last_tc_seq: u32) {
        let next = &mut self.view.next_tc_seq;
        if last_tc_seq >= *next {
            *next = last_tc_seq.saturating_add(1);
        } else if *next > last_tc_seq.saturating_add(ANTI_REPLAY_WINDOW) {
            *next = last_tc_seq + 1;
        }
    }

    /// Trame montante à émettre à l'instant `t` (au plus une par appel), seulement en visibilité.
    pub fn poll_tx(&mut self, t: u32, in_view: bool) -> Option<([u8; MAX_FRAME], usize)> {
        let pass_start = in_view && !self.was_in_view;
        self.was_in_view = in_view;
        if !in_view {
            return None;
        }
        if pass_start {
            self.next_ping_t = t;
        }
        let cmd = if let Some(c) = self.queue.pop_front() {
            Some(c)
        } else if self.policy == Policy::Auto && t >= self.next_ping_t {
            let due = self.next_ping_t.saturating_add(PING_PERIOD_S);
            self.next_ping_t = if t < due {
                due
            } else {
                t.saturating_add(PING_PERIOD_S)
            };
            Some(Command::Ping)
        } else {
            None
        }?;
        let mut out = [0u8; MAX_FRAME];
        let seq = self.view.next_tc_seq;
        let n = proto::encode_tc(seq, &cmd, &mut out);
        self.view.next_tc_seq = seq.wrapping_add(1);
        self.view.tc_sent += 1;
        Some((out, n))
    }

    /// Met une commande en file (ignorée si la file est pleine).
    pub fn queue(&mut self, c: Command) {
        if self.queue.len() < MAX_QUEUE {
            self.queue.push_back(c);
        }
    }

    pub fn view(&self) -> &GroundView {
        &self.view
    }

    /// Nombre de commandes en attente.
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }
}

fn push_bounded<T>(q: &mut VecDeque<T>, v: T, cap: usize) {
    if q.len() == cap {
        q.pop_front();
    }
    q.push_back(v);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beacon(last_tc_seq: u32) -> Beacon {
        Beacon {
            uptime_s: 100,
            boot_count: 1,
            vbatt_mv: 3900,
            last_tc_seq,
            ..Beacon::default()
        }
    }

    fn beacon_frame(seq: u32, b: &Beacon) -> Vec<u8> {
        let mut out = [0u8; MAX_FRAME];
        let n = proto::encode_beacon(seq, b, &mut out);
        out[..n].to_vec()
    }

    fn decode_tc(buf: &[u8]) -> (u32, Command) {
        let f = proto::decode_frame(buf).unwrap();
        assert_eq!(f.ftype, FrameType::Tc);
        (f.seq, Command::decode(f.payload).unwrap())
    }

    #[test]
    fn balise_valide_met_a_jour_la_vue() {
        let mut g = Ground::new(Policy::Auto);
        let b = beacon(0);
        g.on_frame(42, &beacon_frame(1, &b));
        assert_eq!(g.view().last_beacon, Some((42, b)));
        assert_eq!(g.view().beacons.len(), 1);
        assert_eq!(g.view().rejects.total(), 0);
    }

    #[test]
    fn trames_invalides_rejetees_et_comptees() {
        let mut g = Ground::new(Policy::Auto);
        let good = beacon_frame(1, &beacon(0));
        // 1 bit basculé dans la charge utile -> CRC.
        let mut flipped = good.clone();
        flipped[12] ^= 0x04;
        g.on_frame(1, &flipped);
        assert_eq!(g.view().rejects.bad_crc, 1);
        // Tronquée.
        g.on_frame(2, &good[..good.len() - 3]);
        assert_eq!(g.view().rejects.bad_len, 1);
        // Type inconnu (version 1, type 9) : recalcul du CRC non nécessaire, le type est testé avant.
        let mut unk = good.clone();
        unk[2] = 0x19;
        g.on_frame(3, &unk);
        assert_eq!(g.view().rejects.bad_type, 1);
        // SYNC faux.
        let mut sync = good.clone();
        sync[0] ^= 1;
        g.on_frame(4, &sync);
        assert_eq!(g.view().rejects.bad_sync, 1);
        // Vide.
        g.on_frame(5, &[]);
        assert_eq!(g.view().rejects.bad_sync, 2);
        assert!(g.view().last_beacon.is_none());
        assert!(g.view().beacons.is_empty());
        assert_eq!(g.view().frames_ok, 0);
    }

    #[test]
    fn hors_visibilite_rien_a_emettre() {
        let mut g = Ground::new(Policy::Auto);
        g.queue(Command::Reboot);
        for t in 0..200 {
            assert!(g.poll_tx(t, false).is_none());
        }
        assert_eq!(g.queue_len(), 1);
    }

    #[test]
    fn ping_au_debut_de_passe_puis_toutes_les_60s() {
        let mut g = Ground::new(Policy::Auto);
        let mut times = Vec::new();
        for t in 1000..1300 {
            if let Some((buf, n)) = g.poll_tx(t, true) {
                let (seq, cmd) = decode_tc(&buf[..n]);
                assert_eq!(cmd, Command::Ping);
                times.push((t, seq));
            }
        }
        let ts: Vec<u32> = times.iter().map(|x| x.0).collect();
        assert_eq!(ts, vec![1000, 1060, 1120, 1180, 1240]);
        let seqs: Vec<u32> = times.iter().map(|x| x.1).collect();
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
        // Nouvelle passe : Ping immédiat.
        assert!(g.poll_tx(2000, false).is_none());
        assert!(g.poll_tx(2100, true).is_some());
        assert!(g.poll_tx(2101, true).is_none());
    }

    #[test]
    fn manuel_n_emet_que_la_file() {
        let mut g = Ground::new(Policy::Manual);
        for t in 0..100 {
            assert!(g.poll_tx(t, true).is_none());
        }
        g.queue(Command::ClearFdir);
        let (buf, n) = g.poll_tx(100, true).unwrap();
        assert_eq!(decode_tc(&buf[..n]), (1, Command::ClearFdir));
        assert!(g.poll_tx(101, true).is_none());
    }

    #[test]
    fn auto_n_envoie_aucune_recuperation_spontanee() {
        let mut g = Ground::new(Policy::Auto);
        for t in 0..600 {
            if let Some((buf, n)) = g.poll_tx(t, true) {
                assert_eq!(decode_tc(&buf[..n]).1, Command::Ping);
            }
        }
    }

    #[test]
    fn resynchronisation_du_seq() {
        let mut g = Ground::new(Policy::Manual);
        g.queue(Command::Ping);
        let (buf, n) = g.poll_tx(0, true).unwrap();
        assert_eq!(decode_tc(&buf[..n]).0, 1);
        g.on_frame(1, &beacon_frame(1, &beacon(57)));
        g.queue(Command::Ping);
        let (buf, n) = g.poll_tx(2, true).unwrap();
        assert_eq!(decode_tc(&buf[..n]).0, 58);
        // Balise en retard (plus petite, dans la fenêtre) : pas de retour en arrière.
        g.on_frame(3, &beacon_frame(2, &beacon(10)));
        assert_eq!(g.view().next_tc_seq, 59);
        // NVM revenue à 0 mais 59 reste dans la fenêtre : pas de descente.
        g.on_frame(4, &beacon_frame(3, &beacon(0)));
        assert_eq!(g.view().next_tc_seq, 59);
    }

    #[test]
    fn resynchronisation_vers_le_bas_hors_fenetre() {
        let mut g = Ground::new(Policy::Manual);
        g.on_frame(0, &beacon_frame(1, &beacon(5000)));
        assert_eq!(g.view().next_tc_seq, 5001);
        g.on_frame(1, &beacon_frame(2, &beacon(0)));
        assert_eq!(g.view().next_tc_seq, 1);
    }

    #[test]
    fn historique_borne() {
        let mut g = Ground::new(Policy::Auto);
        for t in 0..(BEACON_HISTORY as u32 + 50) {
            g.on_frame(t, &beacon_frame(t, &beacon(0)));
        }
        assert_eq!(g.view().beacons.len(), BEACON_HISTORY);
        assert_eq!(g.view().beacons.front().unwrap().0, 50);
    }

    #[test]
    fn ack_et_events_enregistres() {
        let mut g = Ground::new(Policy::Auto);
        let ack = Ack {
            tc_seq: 7,
            opcode: 1,
            result: proto::AckResult::Ok,
        };
        let mut out = [0u8; MAX_FRAME];
        let n = proto::encode_ack(3, &ack, &mut out);
        g.on_frame(9, &out[..n]);
        assert_eq!(g.view().acks.back(), Some(&(9, ack)));
        let n = proto::encode_events(4, &[1, 2, 3], &mut out).unwrap();
        g.on_frame(10, &out[..n]);
        assert_eq!(g.view().events.back().unwrap().1, vec![1, 2, 3]);
    }

    #[test]
    fn aucun_panic_sur_tampons_aleatoires() {
        let mut g = Ground::new(Policy::Auto);
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15; // graine fixe
        let mut next = move || {
            s ^= s >> 12;
            s ^= s << 25;
            s ^= s >> 27;
            s.wrapping_mul(0x2545_F491_4F6C_DD1D)
        };
        for i in 0..10_000u32 {
            let len = (next() % 80) as usize;
            let mut buf: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            // Une fois sur deux, préfixe SYNC + type valide pour pousser plus loin le décodeur.
            if i % 2 == 0 && buf.len() >= 3 {
                buf[0] = 0xAE;
                buf[1] = 0x61;
                buf[2] = 0x10 | ((next() % 5) as u8);
            }
            g.on_frame(i, &buf);
            let _ = g.poll_tx(i, i % 3 != 0);
        }
    }
}
