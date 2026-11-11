//! `dc lab`: a local web page for configuring, starting and watching study campaigns
//! (`docs/tech/sim-program.md` §4.6).
//!
//! A tiny HTTP/1.1 server on the standard library only, bound to 127.0.0.1. It is a thin editor
//! over the same files `dc analyze` uses: saving writes `campaign.toml`, Start spawns a detached
//! `dc analyze run --name <n>` (so closing the page or the server never stops a run), Pause
//! writes the `PAUSE` file. Dashboards are served from `<data_root>/analysis/`.
//!
//! Safety: loopback only; requests whose `Host` isn't this server are refused (blocks DNS
//! rebinding), and writes must be `application/json` POSTs, which a page from another site can't
//! send without a CORS preflight this server never answers.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::Args;
use serde::Serialize;

use storage::DataRoot;

use crate::CliResult;
use crate::analyze::{
    Campaign, DEFAULT_BOT, Status, campaign_dir, default_games, game_info, is_running, list_status,
};

const PAGE: &str = include_str!("../assets/lab.html");
const MAX_BODY: usize = 1 << 20;

#[derive(Args)]
pub struct LabArgs {
    /// Port to listen on (default: any free port).
    #[arg(long, default_value_t = 0)]
    pub port: u16,
    /// Don't open the browser.
    #[arg(long)]
    pub no_open: bool,
}

pub fn run(args: LabArgs, root: &DataRoot) -> CliResult {
    let listener = TcpListener::bind(("127.0.0.1", args.port)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let url = format!("http://127.0.0.1:{port}/");
    println!("dc lab: {url}  (Ctrl+C stops the page; running campaigns keep going)");
    if !args.no_open {
        open_browser(&url);
    }
    let root = root.clone();
    for stream in listener.incoming().flatten() {
        let root = root.clone();
        std::thread::spawn(move || {
            if let Err(e) = handle(stream, &root, port) {
                eprintln!("lab: {e}");
            }
        });
    }
    Ok(())
}

fn open_browser(url: &str) {
    let r = if cfg!(windows) {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else {
        Command::new("xdg-open").arg(url).spawn()
    };
    if r.is_err() {
        println!("open {url} in your browser");
    }
}

// ---------------------------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------------------------

struct Request {
    method: String,
    path: String,
    query: String,
    host: String,
    content_type: String,
    body: Vec<u8>,
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn ok(content_type: &'static str, body: impl Into<Vec<u8>>) -> Response {
        Response {
            status: 200,
            content_type,
            body: body.into(),
        }
    }
    fn json<T: Serialize>(v: &T) -> Response {
        Response::ok(
            "application/json",
            serde_json::to_vec(v).unwrap_or_default(),
        )
    }
    fn err(status: u16, msg: impl Into<String>) -> Response {
        Response {
            status,
            content_type: "text/plain; charset=utf-8",
            body: msg.into().into_bytes(),
        }
    }
}

fn read_request(stream: &TcpStream) -> Result<Request, String> {
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| e.to_string())?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };
    let (mut host, mut content_type, mut len) = (String::new(), String::new(), 0usize);
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).map_err(|e| e.to_string())? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let v = v.trim();
            match k.trim().to_ascii_lowercase().as_str() {
                "host" => host = v.to_string(),
                "content-type" => content_type = v.to_ascii_lowercase(),
                "content-length" => len = v.parse().map_err(|_| "bad content-length")?,
                _ => {}
            }
        }
    }
    if len > MAX_BODY {
        return Err("request body too large".into());
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok(Request {
        method,
        path,
        query,
        host,
        content_type,
        body,
    })
}

fn handle(mut stream: TcpStream, root: &DataRoot, port: u16) -> Result<(), String> {
    let req = read_request(&stream)?;
    let resp = route(&req, root, port);
    let reason = match resp.status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        resp.status,
        resp.content_type,
        resp.body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(&resp.body))
        .map_err(|e| e.to_string())
}

fn route(req: &Request, root: &DataRoot, port: u16) -> Response {
    let allowed = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !allowed.contains(&req.host) {
        return Response::err(403, "dc lab only answers on its own loopback address");
    }
    let name = query_param(&req.query, "name");
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => Response::ok("text/html; charset=utf-8", PAGE),
        ("GET", "/api/state") => Response::json(&state(root)),
        ("GET", p) if p.starts_with("/analysis/") => serve_file(root, &p["/analysis/".len()..]),
        ("POST", p) if !req.content_type.starts_with("application/json") => {
            let _ = p;
            Response::err(403, "POST bodies must be application/json")
        }
        ("POST", "/api/campaign") => save_campaign(root, &req.body),
        ("POST", "/api/start") => with_name(name, |n| start(root, n)),
        ("POST", "/api/pause") => with_name(name, |n| pause(root, n)),
        _ => Response::err(404, "not found"),
    }
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.to_string())
}

/// Campaign names become folder names: keep them to a safe alphabet.
fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn with_name(name: Option<String>, f: impl FnOnce(&str) -> Response) -> Response {
    match name {
        Some(n) if valid_name(&n) => f(&n),
        _ => Response::err(400, "campaign names use letters, digits, - and _"),
    }
}

// ---------------------------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------------------------

#[derive(Serialize)]
struct LabState {
    games: Vec<GameInfo>,
    default_games: Vec<String>,
    default_bot: &'static str,
    bot_examples: Vec<&'static str>,
    campaigns: Vec<CampaignState>,
}

#[derive(Serialize)]
struct GameInfo {
    id: String,
    name: String,
    min_seats: u8,
    max_seats: u8,
}

#[derive(Serialize)]
struct CampaignState {
    name: String,
    running: bool,
    status: Status,
    campaign: Option<Campaign>,
    has_dashboards: bool,
}

fn state(root: &DataRoot) -> LabState {
    let games = games::registry::GAME_IDS
        .iter()
        .filter_map(|id| {
            let (name, range, _) = game_info(id).ok()?;
            Some(GameInfo {
                id: id.to_string(),
                name,
                min_seats: *range.start(),
                max_seats: *range.end(),
            })
        })
        .collect();
    let campaigns = list_status(root)
        .into_iter()
        .map(|mut status| {
            let dir = campaign_dir(root, &status.name);
            let running = is_running(&dir);
            if !running && status.state == "running" {
                status.state = "stopped (the run was interrupted; Start resumes it)".into();
            }
            CampaignState {
                name: status.name.clone(),
                running,
                campaign: Campaign::load(&dir).ok(),
                has_dashboards: dir.join("index.html").exists(),
                status,
            }
        })
        .collect();
    LabState {
        games,
        default_games: default_games(),
        default_bot: DEFAULT_BOT,
        bot_examples: vec![
            "random",
            "callstation",
            "maniac",
            "ismcts:1:100",
            "ismcts:3:300",
            "ismcts:5",
            "ismcts:7",
        ],
        campaigns,
    }
}

fn save_campaign(root: &DataRoot, body: &[u8]) -> Response {
    let c: Campaign = match serde_json::from_slice(body) {
        Ok(c) => c,
        Err(e) => return Response::err(400, format!("bad campaign: {e}")),
    };
    if !valid_name(&c.name) {
        return Response::err(400, "campaign names use letters, digits, - and _");
    }
    if let Err(e) = c.validate() {
        return Response::err(400, e);
    }
    if c.hands_per_game == 0 || c.chunk_deals == 0 {
        return Response::err(400, "hands and chunk size must be at least 1");
    }
    let dir = campaign_dir(root, &c.name);
    if is_running(&dir) {
        return Response::err(409, "pause the campaign before changing it");
    }
    match c.save(&dir) {
        Ok(()) => Response::json(&"saved"),
        Err(e) => Response::err(500, e),
    }
}

fn start(root: &DataRoot, name: &str) -> Response {
    let dir = campaign_dir(root, name);
    if Campaign::load(&dir).is_err() {
        return Response::err(404, "save the campaign first");
    }
    if is_running(&dir) {
        return Response::err(409, "already running");
    }
    match spawn_run(root, name, &dir) {
        Ok(()) => Response::json(&"started"),
        Err(e) => Response::err(500, e),
    }
}

/// Start `dc analyze run --name <name>` as its own process, detached from this one, logging to
/// `<campaign>/run.log`.
fn spawn_run(root: &DataRoot, name: &str, dir: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("run.log"))
        .map_err(|e| e.to_string())?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(exe);
    cmd.arg("--data-root")
        .arg(&root.0)
        .args(["analyze", "run", "--name", name])
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log2);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

fn pause(root: &DataRoot, name: &str) -> Response {
    let dir = campaign_dir(root, name);
    if !is_running(&dir) {
        return Response::err(409, "not running");
    }
    match std::fs::write(dir.join("PAUSE"), b"") {
        Ok(()) => Response::json(&"pausing after the current chunk"),
        Err(e) => Response::err(500, e.to_string()),
    }
}

/// Files under `<data_root>/analysis/` (dashboards). Only plain path segments are allowed.
fn serve_file(root: &DataRoot, rel: &str) -> Response {
    let segs: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
    let safe = |s: &&str| {
        *s != ".." && *s != "." && !s.contains('\\') && !s.contains(':') && !s.contains('%')
    };
    if segs.is_empty() || !segs.iter().all(safe) {
        return Response::err(400, "bad path");
    }
    let mut p: PathBuf = root.0.join("analysis");
    for s in &segs {
        p.push(s);
    }
    let ct = match p.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("json") => "application/json",
        Some("toml" | "log") => "text/plain; charset=utf-8",
        _ => return Response::err(404, "not found"),
    };
    match std::fs::read(&p) {
        Ok(b) => Response::ok(ct, b),
        Err(_) => Response::err(404, "not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, path: &str, host: &str, ct: &str) -> Request {
        Request {
            method: method.into(),
            path: path.into(),
            query: String::new(),
            host: host.into(),
            content_type: ct.into(),
            body: Vec::new(),
        }
    }

    #[test]
    fn foreign_hosts_and_form_posts_are_refused() {
        let root = DataRoot(std::env::temp_dir().join("dc-lab-test-none"));
        assert_eq!(
            route(&req("GET", "/", "evil.com", ""), &root, 5000).status,
            403
        );
        assert_eq!(
            route(&req("GET", "/", "127.0.0.1:5000", ""), &root, 5000).status,
            200
        );
        let form = req("POST", "/api/campaign", "127.0.0.1:5000", "text/plain");
        assert_eq!(route(&form, &root, 5000).status, 403);
    }

    #[test]
    fn paths_outside_analysis_are_refused() {
        let root = DataRoot(std::env::temp_dir().join("dc-lab-test-none"));
        for p in [
            "/analysis/../x.html",
            "/analysis/a\\..\\b.html",
            "/analysis/C:/x.html",
        ] {
            let r = route(&req("GET", p, "localhost:1", ""), &root, 1);
            assert_eq!(r.status, 400, "{p}");
        }
        assert!(valid_name("study-1_a") && !valid_name("../x") && !valid_name(""));
    }
}
