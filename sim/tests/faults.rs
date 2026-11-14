//! T-014 : effet PHYSIQUE de chaque classe de pannes (F1-F13, D1), indépendamment de la réaction du
//! firmware (banc sans firmware quand c'est possible), reproductibilité et absence de HARNESS.

use firmware::{Hal, Line, Reg, ResetCause, SensorId, NVM_SIZE, RAM_SIZE};
use sim::fault::{self, Fault, FaultClass, StuckKind};
use sim::hw::SensorFault;
use sim::oracle::Category;
use sim::world::{run, Cpu, SimConfig, World};

fn world(phase: u32, soc: f64) -> World {
    let mut cfg = SimConfig::default();
    cfg.params.phase_init_s = Some(phase);
    cfg.params.soc_init_pct = Some(soc);
    World::new(cfg)
}

/// Monde sans firmware : le test pilote le matériel, la mesure ne dépend pas du firmware.
fn bench(phase: u32, soc: f64) -> World {
    let mut w = world(phase, soc);
    w.set_firmware_enabled(false);
    w
}

fn bits_differing(a: &[u8], b: &[u8]) -> Vec<usize> {
    let mut v = Vec::new();
    for i in 0..a.len() {
        for k in 0..8 {
            if (a[i] ^ b[i]) & (1 << k) != 0 {
                v.push(i * 8 + k);
            }
        }
    }
    v
}

#[test]
fn f1_one_ram_bit_flipped() {
    let mut w = bench(0, 80.0);
    let before = *w.firmware_mut().ram_mut();
    w.inject(&Fault::RamSeu { byte: 300, bit: 5 });
    let after = *w.firmware_mut().ram_mut();
    assert_eq!(bits_differing(&before, &after), vec![300 * 8 + 5]);
    let r = &w.injections()[0];
    assert_eq!(r.applied_t, Some(0));
    assert!(r.touched.contains("byte=300") && r.touched.contains("region="));
}

#[test]
fn f2_adjacent_bits_flipped() {
    for n in 2..=4u8 {
        let mut w = bench(0, 80.0);
        let before = *w.firmware_mut().ram_mut();
        w.inject(&Fault::RamMbu {
            byte: 100,
            bit: 6,
            n,
        });
        let after = *w.firmware_mut().ram_mut();
        let d = bits_differing(&before, &after);
        let expect: Vec<usize> = (0..n as usize).map(|k| 100 * 8 + 6 + k).collect();
        assert_eq!(d, expect);
    }
}

#[test]
fn f3_register_bit_flipped() {
    let mut w = bench(0, 80.0);
    let b = w.hw().reg(Reg::LoadSwitches);
    w.inject(&Fault::RegSeu {
        reg: Reg::LoadSwitches,
        bit: 2,
    });
    assert_eq!(w.hw().reg(Reg::LoadSwitches), b ^ 0b100);
}

#[test]
fn f4_latchup_raises_current_and_unmitigated_line_is_destroyed_at_30s() {
    let mut w = bench(0, 80.0);
    w.hw_mut()
        .set_reg(Reg::LoadSwitches, Line::Radio.mask() | Line::Payload.mask());
    let base = w.step();
    let i0 = w.hw_mut().sensor(SensorId::ILinePayload).unwrap();
    w.inject(&Fault::LatchUp {
        line: Line::Payload,
        extra_ma: 600,
    });
    let hot = w.step();
    let i1 = w.hw_mut().sensor(SensorId::ILinePayload).unwrap();
    assert!(i1 - i0 > 550, "courant {i0} -> {i1}");
    assert!(hot.load_w > base.load_w + 3.0, "charge réellement accrue");
    // 29 ticks de plus (30 au total avec le premier) : pas encore détruite à 28, détruite à 30.
    let mut destroyed_after = None;
    for k in 2..=40u32 {
        w.step();
        if w.truth().is_destroyed(Line::Payload) && destroyed_after.is_none() {
            destroyed_after = Some(k);
        }
    }
    assert_eq!(destroyed_after, Some(30), "destruction à T_SEL = 30 s");
    assert!(w.hw().line_dead(Line::Payload));
    // ligne détruite : plus de courant, plus de charge
    let s = w.step();
    assert!(w.hw_mut().sensor(SensorId::ILinePayload).unwrap().abs() < 30);
    assert!(s.load_w < base.load_w);
}

#[test]
fn f4_cut_for_two_seconds_clears_but_one_second_does_not() {
    let both = Line::Radio.mask() | Line::Payload.mask();
    let mut w = bench(0, 80.0);
    w.hw_mut().set_reg(Reg::LoadSwitches, both);
    w.inject(&Fault::LatchUp {
        line: Line::Payload,
        extra_ma: 500,
    });
    for _ in 0..10 {
        w.step();
    }
    w.hw_mut().set_reg(Reg::LoadSwitches, Line::Radio.mask());
    w.step();
    w.step(); // 2 s coupée
    assert_eq!(w.sel_extra_ma(Line::Payload), None, "effacé après 2 s");
    w.hw_mut().set_reg(Reg::LoadSwitches, both);
    for _ in 0..60 {
        w.step();
    }
    assert!(!w.truth().is_destroyed(Line::Payload));

    let mut w = bench(0, 80.0);
    w.hw_mut().set_reg(Reg::LoadSwitches, both);
    w.inject(&Fault::LatchUp {
        line: Line::Payload,
        extra_ma: 500,
    });
    for _ in 0..10 {
        w.step();
    }
    w.hw_mut().set_reg(Reg::LoadSwitches, Line::Radio.mask());
    w.step(); // 1 s seulement
    w.hw_mut().set_reg(Reg::LoadSwitches, both);
    assert_eq!(w.sel_extra_ma(Line::Payload), Some(500));
    for _ in 0..30 {
        w.step();
    }
    assert!(
        w.truth().is_destroyed(Line::Payload),
        "coupure < 2 s : persiste"
    );
}

#[test]
fn f4_latent_until_the_line_is_powered() {
    let mut w = bench(0, 80.0);
    w.inject(&Fault::LatchUp {
        line: Line::Heater,
        extra_ma: 400,
    });
    for _ in 0..5 {
        w.step();
    }
    assert_eq!(w.injections()[0].applied_t, None);
    w.hw_mut()
        .set_reg(Reg::LoadSwitches, Line::Radio.mask() | Line::Heater.mask());
    w.step();
    assert_eq!(w.sel_extra_ma(Line::Heater), Some(400));
}

#[test]
fn f5_sensor_faults_change_what_the_hardware_returns() {
    let mut w = bench(0, 80.0);
    w.step();
    let v = w.hw().sensor_raw(SensorId::VBattA);
    w.inject(&Fault::SensorStuck {
        sensor: SensorId::VBattA,
        kind: StuckKind::Frozen,
    });
    assert_eq!(
        w.hw().sensor_fault(SensorId::VBattA),
        Some(SensorFault::Frozen(v))
    );
    for _ in 0..20 {
        w.step();
    }
    assert_eq!(w.hw_mut().sensor(SensorId::VBattA), Ok(v));
    assert_ne!(w.hw().sensor_raw(SensorId::VBattA), 0);
    w.inject(&Fault::SensorStuck {
        sensor: SensorId::TempObc,
        kind: StuckKind::Err,
    });
    assert!(w.hw_mut().sensor(SensorId::TempObc).is_err());
    w.inject(&Fault::SensorStuck {
        sensor: SensorId::ISolar,
        kind: StuckKind::OutOfRange(30_000),
    });
    assert_eq!(w.hw_mut().sensor(SensorId::ISolar), Ok(30_000));
}

#[test]
fn f6_uplink_frame_really_altered() {
    let frame_len = |w: &mut World, f: Fault| {
        // phase 1500 : début de la passe, le sol émet ; firmware coupé : la trame reste en file.
        w.inject(&f);
        for _ in 0..100 {
            w.step();
            if w.injections()[0].applied_t.is_some() {
                break;
            }
        }
        w.injections()[0].clone()
    };
    let mut w = bench(1500, 80.0);
    let r = frame_len(
        &mut w,
        Fault::TcCorrupt {
            bits: vec![1, 40, 41, 99],
        },
    );
    let (b, a) = (r.frame_before.unwrap(), r.frame_after.unwrap());
    assert_eq!(b.len(), a.len());
    assert_eq!(bits_differing(&b, &a).len(), 4);
    assert_eq!(
        w.hw().uplink_peek(),
        Some(&a[..]),
        "la trame altérée est celle livrée"
    );

    let mut w = bench(1500, 80.0);
    let r = frame_len(&mut w, Fault::TcTruncate { frac: 3 });
    let (b, a) = (r.frame_before.unwrap(), r.frame_after.unwrap());
    assert!(a.len() < b.len() && !a.is_empty());
    assert_eq!(&b[..a.len()], &a[..]);
    assert_eq!(w.hw().uplink_peek(), Some(&a[..]));

    // rejeu : deux trames en file au lieu d'une
    let mut w = bench(1500, 80.0);
    let r = frame_len(&mut w, Fault::TcReplay { pick: 0 });
    assert!(r.applied_t.is_some());
    assert!(w.hw().uplink_len() >= 2, "trame + rejeu");
}

#[test]
fn f7_spurious_reset_and_config_hit() {
    let mut w = world(0, 80.0);
    for _ in 0..3 {
        w.step();
    }
    w.inject(&Fault::WdgSpurious);
    let s = w.step();
    assert_eq!(s.resets_by_cause[ResetCause::Watchdog as usize], 1);
    assert!(matches!(w.cpu(), Cpu::Booting(_)));

    let mut w = bench(0, 80.0);
    w.inject(&Fault::WdgRegHit {
        reg: Reg::WdgCtrl,
        bit: 0,
    });
    assert_eq!(w.hw().reg(Reg::WdgCtrl), 0, "watchdog désactivé");
    for _ in 0..100 {
        w.step();
    }
    w.inject(&Fault::WdgRegHit {
        reg: Reg::WdgTimeout,
        bit: 15,
    });
    assert_eq!(w.hw().reg(Reg::WdgTimeout), 10 | 0x8000);
}

#[test]
fn f8_hits_fdir_and_sched_regions() {
    use firmware::{region_of, RegionKind, RAM_MAP};
    for (kind, mk) in [
        (
            RegionKind::Fdir,
            (|byte, bit| Fault::FdirHit { byte, bit }) as fn(u16, u8) -> Fault,
        ),
        (RegionKind::Sched, |byte, bit| Fault::SchedHit { byte, bit }),
    ] {
        let r = RAM_MAP.iter().find(|r| r.kind == kind).unwrap();
        let byte = (r.start + r.len / 2) as u16;
        let mut w = bench(0, 80.0);
        let before = *w.firmware_mut().ram_mut();
        w.inject(&mk(byte, 3));
        let after = *w.firmware_mut().ram_mut();
        assert_eq!(bits_differing(&before, &after), vec![byte as usize * 8 + 3]);
        assert_eq!(region_of(byte as usize).unwrap().kind, kind);
    }
    // les tirages de F8 restent dans leur région
    let p = sim::params::Params::default();
    for seed in 0..200 {
        let ev = fault::draw(FaultClass::F8, seed, &p);
        let (byte, kind) = match ev[0].fault {
            Fault::FdirHit { byte, .. } => (byte, RegionKind::Fdir),
            Fault::SchedHit { byte, .. } => (byte, RegionKind::Sched),
            _ => panic!(),
        };
        assert_eq!(region_of(byte as usize).unwrap().kind, kind);
    }
}

#[test]
fn f9_hang_stops_the_firmware_until_reset() {
    let mut w = world(0, 80.0);
    for _ in 0..5 {
        w.step();
    }
    w.inject(&Fault::CpuHang);
    let ram = *w.firmware_mut().ram_mut();
    let s = w.step();
    assert!(s.hung);
    assert_eq!(
        *w.firmware_mut().ram_mut(),
        ram,
        "aucun cycle firmware exécuté"
    );
    let mut reset = false;
    for _ in 0..20 {
        let s = w.step();
        reset |= s.resets_by_cause[ResetCause::Watchdog as usize] == 1;
    }
    assert!(reset && !w.step().hung, "le watchdog libère le blocage");
}

#[test]
fn f10_forces_soc_and_cuts_obc() {
    let mut w = bench(4000, 80.0); // éclipse
    w.inject(&Fault::BattDeplete { soc_x10: 30 });
    let s = w.step();
    assert!(s.soc_pct < 3.1 && s.soc_pct > 2.5, "soc {}", s.soc_pct);
    assert!(!s.obc_powered, "coupure matérielle sous SOC_OFF");
    assert_eq!(s.cpu, Cpu::Off);
}

#[test]
fn f11_nvm_bits_flipped() {
    let mut w = bench(0, 80.0);
    let before = *w.hw().nvm();
    w.inject(&Fault::NvmCorrupt {
        bits: vec![0, 9, 2047],
    });
    let after = *w.hw().nvm();
    assert_eq!(bits_differing(&before, &after), vec![0, 9, 2047]);
    assert_eq!(NVM_SIZE, 256);
}

#[test]
fn f12_downlink_frame_really_altered() {
    let mut w = world(1500, 80.0);
    w.inject(&Fault::TmCorrupt {
        bits: vec![5, 70, 71],
    });
    let mut got = None;
    for _ in 0..60 {
        w.step();
        if w.injections()[0].applied_t.is_some() {
            got = Some(w.injections()[0].clone());
            break;
        }
    }
    let r = got.expect("aucune trame descendante en passe");
    let (b, a) = (r.frame_before.unwrap(), r.frame_after.unwrap());
    assert_eq!(bits_differing(&b, &a).len(), 3);
    let dl = w.drain_downlink();
    assert!(
        dl.iter().any(|f| f.bytes == a),
        "le sol a reçu la trame altérée"
    );
}

#[test]
fn f13_obc_cut_for_10s_then_cold_restart() {
    let mut w = world(0, 80.0);
    for _ in 0..10 {
        w.step();
    }
    w.inject(&Fault::ObcLatchUp);
    let mut off = 0;
    let mut browned = false;
    for _ in 0..40 {
        let s = w.step();
        if !s.obc_powered {
            off += 1;
        }
        browned |= s.resets_by_cause[ResetCause::Brownout as usize] == 1;
    }
    assert_eq!(off, 10, "coupé exactement 10 s");
    assert!(browned, "redémarrage à froid");
    assert_eq!(w.cpu(), Cpu::Running);
}

#[test]
fn d1_dead_watchdog_plus_hang_is_never_released() {
    let mut w = world(0, 80.0);
    for _ in 0..5 {
        w.step();
    }
    w.inject(&Fault::WdgDead);
    w.inject(&Fault::CpuHang);
    for _ in 0..500 {
        w.step();
    }
    let s = w.step();
    assert!(s.hung);
    assert_eq!(s.resets_by_cause[ResetCause::Watchdog as usize], 0);
    assert!(w.hw().wdg_dead());
}

#[test]
fn same_seed_same_fault_and_same_run() {
    for c in [
        FaultClass::F1,
        FaultClass::F4,
        FaultClass::F6,
        FaultClass::D1,
    ] {
        let a = run(fault::sim_config(c, 7));
        let b = run(fault::sim_config(c, 7));
        assert_eq!(a, b, "{c:?}");
        assert!(!a.injections.is_empty() || c == FaultClass::None);
    }
}

#[test]
fn every_class_twenty_seeds_no_harness() {
    let classes: Vec<FaultClass> = FaultClass::ALL.to_vec();
    std::thread::scope(|sc| {
        let hs: Vec<_> = classes
            .into_iter()
            .map(|c| {
                sc.spawn(move || {
                    let mut line = format!("{:>4}:", c.label());
                    for seed in 1..=20u64 {
                        let r = run(fault::sim_config(c, seed));
                        assert!(
                            !r.categories().contains(&Category::Harness),
                            "{c:?} graine {seed}"
                        );
                        line += &format!(" {:?}", r.verdict);
                    }
                    println!("{line}");
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
    });
    let _ = RAM_SIZE;
}
