//! Dashboard Aegis : serveur HTTP minimal (127.0.0.1 uniquement) + `World` en temps accéléré.
//! Routes : GET / | GET /api/state | GET /api/campaign | POST /api/fault | POST /api/cmd.

mod http;
mod json;

use firmware::proto::Command;
use http::{parse_request, ParseError, Request, Response};
use json::{array, get_num, get_str, parse_flat, Obj};
use sim::fault::{draw, Fault, FaultClass};
use sim::params::Params;
use sim::world::{Cpu, SimConfig, Snapshot, World};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static INDEX_HTML: &str = include_str!("../static/index.html");

/// Ordre transmis au fil de simulation.
enum Control {
    Cmd(Command),
    Faults { faults: Vec<Fault>, label: String },
}

struct Ctx {
    /// Dernier état sérialisé (produit par le fil de simulation).
    state: Mutex<String>,
    tx: Mutex<Sender<Control>>,
    results_dir: PathBuf,
    base_seed: u64,
    fault_counter: AtomicU64,
}

fn main() {
    let mut port: u16 = 8080;
    let mut seed: u64 = 1;
    let mut speed: u32 = 120; // ticks simulés par seconde réelle
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let v = args.next();
        let parsed = match (a.as_str(), v) {
            ("--port", Some(v)) => v.parse().map(|x| port = x).is_ok(),
            ("--seed", Some(v)) => v.parse().map(|x| seed = x).is_ok(),
            ("--speed", Some(v)) => v.parse().map(|x| speed = x).is_ok(),
            _ => false,
        };
        if !parsed {
            eprintln!("usage : dashboard [--port P] [--seed S] [--speed TICKS_PAR_S]");
            std::process::exit(2);
        }
    }
    let (tx, rx) = channel();
    let ctx = Arc::new(Ctx {
        state: Mutex::new("{}".to_string()),
        tx: Mutex::new(tx),
        results_dir: PathBuf::from("results"),
        base_seed: seed,
        fault_counter: AtomicU64::new(0),
    });
    eprintln!("dashboard : graine du monde = {seed}, vitesse = {speed} ticks/s");
    {
        let ctx = Arc::clone(&ctx);
        std::thread::spawn(move || sim_thread(&ctx, rx, seed, speed.max(1)));
    }
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bind 127.0.0.1:{port} impossible : {e}");
            std::process::exit(1);
        }
    };
    eprintln!("dashboard : http://127.0.0.1:{port}/");
    for conn in listener.incoming().flatten() {
        let ctx = Arc::clone(&ctx);
        std::thread::spawn(move || serve(conn, &ctx));
    }
}

// ---------------------------------------------------------------------------------------------
// Fil de simulation
// ---------------------------------------------------------------------------------------------

fn sim_thread(ctx: &Ctx, rx: Receiver<Control>, seed: u64, speed: u32) {
    let cfg = SimConfig {
        seed,
        duration_s: u32::MAX,
        ..SimConfig::default()
    };
    let mut world = World::new(cfg);
    let mut log: Vec<String> = Vec::new();
    let slice = Duration::from_millis(100);
    let per_slice = (speed / 10).max(1);
    loop {
        let t0 = Instant::now();
        while let Ok(c) = rx.try_recv() {
            match c {
                Control::Cmd(cmd) => {
                    log.push(format!("t={} TC {:?}", world.time(), cmd));
                    world.queue_cmd(cmd);
                }
                Control::Faults { faults, label } => {
                    for f in &faults {
                        log.push(format!(
                            "t={} panne {} {} {}",
                            world.time(),
                            label,
                            f.name(),
                            f.params_text()
                        ));
                        world.inject(f);
                    }
                }
            }
        }
        let mut snap = None;
        for _ in 0..per_slice {
            snap = Some(world.step());
        }
        if let Some(s) = snap {
            if log.len() > 30 {
                log.drain(..log.len() - 30);
            }
            let js = state_json(&s, &world, seed, speed, &log);
            if let Ok(mut g) = ctx.state.lock() {
                *g = js;
            }
        }
        if let Some(rest) = slice.checked_sub(t0.elapsed()) {
            std::thread::sleep(rest);
        }
    }
}

fn cpu_str(c: Cpu) -> String {
    match c {
        Cpu::Running => "Running".into(),
        Cpu::Booting(n) => format!("Booting({n})"),
        Cpu::Off => "Off".into(),
    }
}

fn state_json(s: &Snapshot, w: &World, seed: u64, speed: u32, log: &[String]) -> String {
    let g = w.ground();
    let beacons: Vec<String> = g
        .beacons
        .iter()
        .rev()
        .take(8)
        .map(|(t, b)| {
            Obj::new()
                .int("t", i64::from(*t))
                .int("mode", i64::from(b.mode))
                .int("boot_count", i64::from(b.boot_count))
                .int("reset_cause", i64::from(b.reset_cause))
                .int("vbatt_mv", i64::from(b.vbatt_mv))
                .int("temp_dc", i64::from(b.temp_batt_dc))
                .int("fdir_flags", i64::from(b.fdir_flags))
                .int("sensor_fault_mask", i64::from(b.sensor_fault_mask))
                .int("ecc_corr", i64::from(b.cnt_ecc_corr))
                .int("tmr_fix", i64::from(b.cnt_tmr_fix))
                .build()
        })
        .collect();
    let acks: Vec<String> = g
        .acks
        .iter()
        .rev()
        .take(8)
        .map(|(t, a)| {
            Obj::new()
                .int("t", i64::from(*t))
                .int("tc_seq", i64::from(a.tc_seq))
                .int("opcode", i64::from(a.opcode))
                .str("result", &format!("{:?}", a.result))
                .build()
        })
        .collect();
    let resets: Vec<String> = s.resets_by_cause.iter().map(u32::to_string).collect();
    let last_beacon = match s.last_beacon {
        Some((t, m, bc)) => Obj::new()
            .int("t", i64::from(t))
            .int("mode", i64::from(m))
            .int("boot_count", i64::from(bc))
            .build(),
        None => "null".into(),
    };
    let logs: Vec<String> = log
        .iter()
        .rev()
        .map(|l| format!("\"{}\"", json::escape(l)))
        .collect();
    Obj::new()
        .int("t", i64::from(s.t))
        .int("seed", seed as i64)
        .int("speed", i64::from(speed))
        .str(
            "mode",
            &s.mode.map_or("inconnu".to_string(), |m| format!("{m:?}")),
        )
        .bool("in_eclipse", s.in_eclipse)
        .bool("in_pass", s.in_pass)
        .int("phase_s", i64::from(s.phase_s))
        .num("soc_pct", s.soc_pct)
        .num("vbatt_mv", s.vbatt_mv)
        .num("temp_c", s.temp_c)
        .num("solar_w", s.solar_w)
        .num("load_w", s.load_w)
        .bool("obc_powered", s.obc_powered)
        .str("cpu", &cpu_str(s.cpu))
        .bool("hung", s.hung)
        .int("lines", i64::from(s.lines))
        .bool("kicked", s.kicked)
        .int("boot_count", i64::from(s.boot_count))
        .raw("resets_by_cause", &array(&resets))
        .str(
            "last_reset",
            &s.last_reset
                .map_or("aucun".to_string(), |r| format!("{r:?}")),
        )
        .int("tx_frames", i64::from(s.tx_frames))
        .int("rx_frames", i64::from(s.rx_frames))
        .int("tx_lost", i64::from(s.tx_lost))
        .int("beacons_rx", i64::from(s.beacons_rx))
        .int("acks_rx", i64::from(s.acks_rx))
        .raw("last_beacon", &last_beacon)
        .int("ground_rejects", i64::from(s.ground_rejects))
        .int("tc_sent", i64::from(s.tc_sent))
        .raw("beacons", &array(&beacons))
        .raw("acks", &array(&acks))
        .raw("log", &array(&logs))
        .build()
}

// ---------------------------------------------------------------------------------------------
// Serveur
// ---------------------------------------------------------------------------------------------

fn serve(mut conn: TcpStream, ctx: &Ctx) {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = conn.set_write_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    let resp = loop {
        match parse_request(&buf) {
            Ok(req) => break route(&req, ctx),
            Err(ParseError::Bad) => break Response::error(400, "requête mal formée"),
            Err(ParseError::TooLarge) => break Response::error(413, "corps trop grand"),
            Err(ParseError::Incomplete) => {}
        }
        if buf.len() > http::MAX_HEAD + http::MAX_BODY {
            break Response::error(413, "requête trop grande");
        }
        match conn.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let _ = conn.write_all(&resp.to_bytes());
}

fn route(req: &Request, ctx: &Ctx) -> Response {
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => Response {
            status: 200,
            content_type: "text/html",
            body: INDEX_HTML.to_string(),
        },
        ("GET", "/api/state") => {
            let s = ctx.state.lock().map(|g| g.clone()).unwrap_or_default();
            Response::json(200, s)
        }
        ("GET", "/api/campaign") => campaign(req, ctx),
        ("POST", "/api/fault") => fault(req, ctx),
        ("POST", "/api/cmd") => cmd(req, ctx),
        (_, "/" | "/api/state" | "/api/campaign" | "/api/fault" | "/api/cmd") => {
            Response::error(405, "méthode non permise")
        }
        _ => Response::error(404, "route inconnue"),
    }
}

fn send(ctx: &Ctx, c: Control) -> bool {
    ctx.tx.lock().map(|t| t.send(c).is_ok()).unwrap_or(false)
}

fn cmd(req: &Request, ctx: &Ctx) -> Response {
    let Ok(o) = parse_flat(&req.body) else {
        return Response::error(400, "JSON invalide");
    };
    let arg = get_num(&o, "arg").filter(|a| (0.0..=255.0).contains(a) && a.fract() == 0.0);
    let c = match (get_str(&o, "cmd"), arg) {
        (Some("Ping"), _) => Command::Ping,
        (Some("Reboot"), _) => Command::Reboot,
        (Some("ClearFdir"), _) => Command::ClearFdir,
        (Some("DumpEvents"), _) => Command::DumpEvents,
        (Some("SetMode"), Some(a)) => Command::SetMode(a as u8),
        (Some("PowerCycle"), Some(a)) => Command::PowerCycle(a as u8),
        (Some("SetMode" | "PowerCycle"), None) => {
            return Response::error(400, "argument 'arg' (0..255) requis")
        }
        _ => return Response::error(400, "commande inconnue"),
    };
    if send(ctx, Control::Cmd(c)) {
        Response::json(200, Obj::new().bool("queued", true).build())
    } else {
        Response::error(500, "fil de simulation arrêté")
    }
}

fn fault(req: &Request, ctx: &Ctx) -> Response {
    let Ok(o) = parse_flat(&req.body) else {
        return Response::error(400, "JSON invalide");
    };
    let class = match get_str(&o, "class").and_then(FaultClass::parse) {
        Some(c) if c != FaultClass::None => c,
        _ => return Response::error(400, "classe inconnue (F1..F13, D1)"),
    };
    let seed = match get_num(&o, "seed") {
        Some(s) if s >= 0.0 && s.fract() == 0.0 && s < 9.0e15 => s as u64,
        Some(_) => return Response::error(400, "graine invalide"),
        None => ctx
            .base_seed
            .wrapping_add(1 + ctx.fault_counter.fetch_add(1, Ordering::Relaxed)),
    };
    // Reproductibilité : mêmes (classe, graine) -> mêmes paramètres de panne (tirage de la campagne).
    // Les dates du tirage sont ignorées : la panne est injectée immédiatement.
    let faults: Vec<Fault> = draw(class, seed, &Params::default())
        .into_iter()
        .map(|e| e.fault)
        .collect();
    let desc: Vec<String> = faults
        .iter()
        .map(|f| {
            format!(
                "\"{}\"",
                json::escape(&format!("{} {}", f.name(), f.params_text()))
            )
        })
        .collect();
    eprintln!(
        "injection : classe={} graine={seed} -> {}",
        class.label(),
        desc.join(", ")
    );
    let label = format!("{}/graine={seed}", class.label());
    if !send(ctx, Control::Faults { faults, label }) {
        return Response::error(500, "fil de simulation arrêté");
    }
    Response::json(
        200,
        Obj::new()
            .str("class", class.label())
            .int("seed", seed as i64)
            .raw("faults", &array(&desc))
            .build(),
    )
}

/// Nom de dossier de résultats : caractères sûrs uniquement (pas de traversée de chemin).
fn safe_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn campaign(req: &Request, ctx: &Ctx) -> Response {
    let name = req
        .query
        .split('&')
        .find_map(|kv| kv.strip_prefix("name="))
        .map(str::to_string);
    let name = match name {
        Some(n) if safe_name(&n) => n,
        Some(_) => return Response::error(400, "nom invalide"),
        None => match latest_campaign(&ctx.results_dir) {
            Some(n) => n,
            None => return Response::error(404, "aucune campagne dans results/"),
        },
    };
    match std::fs::read_to_string(ctx.results_dir.join(&name).join("summary.json")) {
        Ok(s) => Response::json(200, s),
        Err(_) => Response::error(404, "summary.json introuvable"),
    }
}

fn latest_campaign(dir: &std::path::Path) -> Option<String> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let m = e.path().join("summary.json").metadata().ok()?;
            Some((m.modified().ok()?, e.file_name().into_string().ok()?))
        })
        .filter(|(_, n)| safe_name(n))
        .max()
        .map(|(_, n)| n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> (Ctx, Receiver<Control>) {
        let (tx, rx) = channel();
        let dir = std::env::temp_dir().join(format!("aegis-dash-test-{}", std::process::id()));
        (
            Ctx {
                state: Mutex::new("{\"mode\":\"Nominal\"}".into()),
                tx: Mutex::new(tx),
                results_dir: dir,
                base_seed: 7,
                fault_counter: AtomicU64::new(0),
            },
            rx,
        )
    }

    fn req(method: &str, path: &str, query: &str, body: &str) -> Request {
        Request {
            method: method.into(),
            path: path.into(),
            query: query.into(),
            body: body.into(),
        }
    }

    #[test]
    fn page_et_etat() {
        let (c, _rx) = ctx();
        let r = route(&req("GET", "/", "", ""), &c);
        assert_eq!(r.status, 200);
        assert!(r.body.contains("<html"));
        let r = route(&req("GET", "/api/state", "", ""), &c);
        assert_eq!(r.status, 200);
        assert_eq!(r.body.matches("\"mode\"").count(), 1);
    }

    #[test]
    fn routes_inconnues_et_methodes() {
        let (c, _rx) = ctx();
        assert_eq!(route(&req("GET", "/nope", "", ""), &c).status, 404);
        assert_eq!(route(&req("DELETE", "/api/state", "", ""), &c).status, 405);
        assert_eq!(route(&req("GET", "/api/cmd", "", ""), &c).status, 405);
    }

    #[test]
    fn commandes() {
        let (c, rx) = ctx();
        let ok = route(&req("POST", "/api/cmd", "", r#"{"cmd":"Ping"}"#), &c);
        assert_eq!(ok.status, 200);
        assert!(matches!(rx.try_recv(), Ok(Control::Cmd(Command::Ping))));
        let ok = route(
            &req("POST", "/api/cmd", "", r#"{"cmd":"SetMode","arg":1}"#),
            &c,
        );
        assert_eq!(ok.status, 200);
        assert!(matches!(
            rx.try_recv(),
            Ok(Control::Cmd(Command::SetMode(1)))
        ));
        for bad in [
            "pas json",
            r#"{"cmd":"SetMode"}"#,
            r#"{"cmd":"SetMode","arg":300}"#,
            r#"{"cmd":"Detruire"}"#,
            "{}",
        ] {
            assert_eq!(
                route(&req("POST", "/api/cmd", "", bad), &c).status,
                400,
                "{bad}"
            );
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn pannes_reproductibles() {
        let (c, rx) = ctx();
        let body = r#"{"class":"F1","seed":42}"#;
        let a = route(&req("POST", "/api/fault", "", body), &c);
        let b = route(&req("POST", "/api/fault", "", body), &c);
        assert_eq!(a.status, 200);
        assert_eq!(a.body, b.body);
        assert!(a.body.contains("RamSeu"));
        assert!(matches!(rx.try_recv(), Ok(Control::Faults { .. })));
        for bad in [
            r#"{"class":"F99"}"#,
            r#"{"class":"none"}"#,
            r#"{"seed":1}"#,
            r#"{"class":"F1","seed":-3}"#,
        ] {
            assert_eq!(
                route(&req("POST", "/api/fault", "", bad), &c).status,
                400,
                "{bad}"
            );
        }
    }

    #[test]
    fn graine_par_defaut_distincte_et_deterministe() {
        let (c, _rx) = ctx();
        let a = route(&req("POST", "/api/fault", "", r#"{"class":"F1"}"#), &c);
        let b = route(&req("POST", "/api/fault", "", r#"{"class":"F1"}"#), &c);
        assert!(a.body.contains("\"seed\":8"));
        assert!(b.body.contains("\"seed\":9"));
    }

    #[test]
    fn campagne_nom_invalide_ou_absente() {
        let (c, _rx) = ctx();
        assert_eq!(
            route(&req("GET", "/api/campaign", "name=../x", ""), &c).status,
            400
        );
        assert_eq!(
            route(&req("GET", "/api/campaign", "name=zzz", ""), &c).status,
            404
        );
    }

    #[test]
    fn etat_reel_du_monde() {
        let mut w = World::new(SimConfig::default());
        let s = w.step();
        let js = state_json(&s, &w, 1, 100, &["a\"b".to_string()]);
        assert_eq!(js.matches("\"mode\"").count(), 1);
        assert!(json::parse_flat(&js).is_err()); // non plat : tableaux, volontairement non lus
        assert!(js.starts_with('{') && js.ends_with('}'));
        assert!(js.contains("a\\\"b"));
    }
}
