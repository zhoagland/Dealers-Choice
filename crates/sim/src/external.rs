//! External policies (PRD FR-BT-8, `docs/tech/bots.md` §6): a bot whose decisions come from a
//! program outside this process — a local program speaking JSON lines on stdin/stdout (a bot in
//! any language), or a model behind an HTTP endpoint (first target: Jev).
//!
//! - **Request:** the seat's `Observation` and the legal actions, as JSON ([`DecisionRequest`]).
//!   **Response:** one probability per legal action (`{"probs": [..]}` or a bare array).
//! - **Determinism (NFR-1):** we sample the response with the seat's own `DetRng`, and cache
//!   responses by `(model, SHA-256 of the request)`. A re-run with a warm cache makes no calls and
//!   gives identical results. The cache can be saved to a JSON-lines file and reused across runs.
//! - **Failure:** a timeout, error, or malformed answer uses the fallback bot for that decision
//!   and is counted, so results are never silently contaminated.
//! - **Metrics:** calls, cache hits, failures and latency per model ([`metrics`]).
//!
//! A spec file (`ext:<path>`), e.g. `config/external/example.toml`:
//!
//! ```toml
//! model = "my-bot-v1"            # cache key and report label
//! transport = "stdio"            # or "http"
//! command = ["python", "tools/external_bot_example.py"]
//! # url = "https://api.example.com/v1/decide"   # transport = "http"
//! # api_key_env = "JEV_API_KEY"                 # sent as a bearer token
//! timeout_ms = 5000
//! concurrency = 2                # parallel processes / in-flight requests
//! fallback = "ismcts:3"
//! cache = "data/external/my-bot-v1.jsonl"       # optional, persisted responses
//! ```

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use bots::Policy;
use engine::{DetRng, Game};

use crate::spec::BotSpec;

/// What an external policy is asked.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionRequest {
    pub model: String,
    pub game_id: String,
    pub game_version: u32,
    /// The seat's observation (everything it knows), in the game's serde form.
    pub observation: serde_json::Value,
    /// The legal actions, in the game's serde form, plus a readable label for each.
    pub legal: Vec<serde_json::Value>,
    pub legal_labels: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Reply {
    Probs { probs: Vec<f64> },
    Bare(Vec<f64>),
}

/// Something that answers decision requests.
pub trait Transport: Send {
    fn decide(&mut self, req: &DecisionRequest) -> Result<Vec<f64>, String>;

    /// Tokens billed since the last call (input, output), for cost reporting. Default none.
    fn take_usage(&mut self) -> (u64, u64) {
        (0, 0)
    }
}

/// The spec file.
#[derive(Clone, Debug, Deserialize)]
pub struct ExternalSpec {
    pub model: String,
    pub transport: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "one")]
    pub concurrency: usize,
    #[serde(default = "default_fallback")]
    pub fallback: String,
    #[serde(default)]
    pub cache: Option<String>,
    /// `jev` transport: the model name sent to TypeSafe (default `jev-latest`).
    #[serde(default)]
    pub jev_model: Option<String>,
    /// `jev` transport: the instruction for the choice question.
    #[serde(default)]
    pub instructions: Option<String>,
}

fn default_timeout() -> u64 {
    5_000
}
fn one() -> usize {
    1
}
fn default_fallback() -> String {
    "ismcts:3".into()
}

impl ExternalSpec {
    pub fn load(path: &str) -> Result<ExternalSpec, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let spec: ExternalSpec = toml::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
        match spec.transport.as_str() {
            "stdio" if spec.command.is_empty() => Err(format!("{path}: stdio needs `command`")),
            "http" if spec.url.is_none() => Err(format!("{path}: http needs `url`")),
            "stdio" | "http" | "jev" | "mock" => Ok(spec),
            t => Err(format!(
                "{path}: unknown transport {t:?} (stdio, http or jev)"
            )),
        }
    }

    fn transport(&self) -> Result<Box<dyn Transport>, String> {
        let timeout = Duration::from_millis(self.timeout_ms);
        match self.transport.as_str() {
            "stdio" => Ok(Box::new(StdioJsonLines::spawn(&self.command, timeout)?)),
            "http" => {
                let key = match &self.api_key_env {
                    Some(var) => Some(
                        std::env::var(var)
                            .map_err(|_| format!("set the {var} environment variable"))?,
                    ),
                    None => None,
                };
                Ok(Box::new(HttpJson::new(
                    self.url.clone().unwrap(),
                    key,
                    timeout,
                )))
            }
            "jev" => {
                let var = self.api_key_env.as_deref().unwrap_or("TYPESAFE_API_KEY");
                let key = std::env::var(var)
                    .map_err(|_| format!("set the {var} environment variable"))?;
                Ok(Box::new(Jev::new(
                    self.url.clone().unwrap_or_else(|| JEV_URL.into()),
                    key,
                    self.jev_model
                        .clone()
                        .unwrap_or_else(|| "jev-latest".into()),
                    self.instructions.clone(),
                    timeout,
                )))
            }
            _ => Ok(Box::new(Mock)),
        }
    }
}

/// TypeSafe's System One endpoint (docs.typesafe.ai, quick start).
pub const JEV_URL: &str = "https://api.typesafe.ai/v1/systemone";

/// Jev (TypeSafe System One): the decision becomes one `choice` question whose options are the
/// legal actions (`a0`, `a1`, … described by their labels); the state is the request as JSON.
/// The answer's `probabilities` map gives the distribution. Usage tokens are counted for cost.
pub struct Jev {
    url: String,
    key: String,
    model: String,
    instructions: String,
    agent: ureq::Agent,
    usage: (u64, u64),
}

impl Jev {
    pub fn new(
        url: String,
        key: String,
        model: String,
        instructions: Option<String>,
        timeout: Duration,
    ) -> Jev {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .into();
        Jev {
            url,
            key,
            model,
            instructions: instructions.unwrap_or_else(|| {
                "You are a strong poker player. The state is your seat's view of the hand \
                 (`observation`) and the legal actions. Choose the action that maximises your \
                 expected winnings."
                    .into()
            }),
            agent,
            usage: (0, 0),
        }
    }

    /// The System One request body for `req`.
    pub fn body(&self, req: &DecisionRequest) -> serde_json::Value {
        let criteria: serde_json::Map<String, serde_json::Value> = req
            .legal_labels
            .iter()
            .enumerate()
            .map(|(i, l)| (format!("a{i}"), serde_json::Value::String(l.clone())))
            .collect();
        serde_json::json!({
            "model": self.model,
            "state": serde_json::to_string(req).expect("requests serialize"),
            "questions": {
                "action": {
                    "type": "choice",
                    "instructions": self.instructions,
                    "criteria": criteria,
                }
            }
        })
    }
}

/// The distribution in a System One reply, in legal-action order.
pub fn jev_probs(reply: &serde_json::Value, n: usize) -> Result<Vec<f64>, String> {
    let probs = reply
        .pointer("/answers/action/probabilities")
        .and_then(|p| p.as_object())
        .ok_or_else(|| format!("no answers.action.probabilities in {reply}"))?;
    (0..n)
        .map(|i| {
            probs
                .get(&format!("a{i}"))
                .and_then(|v| v.as_f64())
                .ok_or_else(|| format!("no probability for a{i}"))
        })
        .collect()
}

impl Transport for Jev {
    fn decide(&mut self, req: &DecisionRequest) -> Result<Vec<f64>, String> {
        let mut reply = self
            .agent
            .post(&self.url)
            .header("Authorization", &format!("Bearer {}", self.key))
            .send_json(self.body(req))
            .map_err(|e| e.to_string())?;
        let status = reply.status();
        let text = reply
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!(
                "HTTP {status}: {}",
                text.chars().take(200).collect::<String>()
            ));
        }
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("bad reply: {e}"))?;
        let tokens = |k: &str| {
            json.pointer(&format!("/usage/{k}"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        };
        self.usage.0 += tokens("input_tokens");
        self.usage.1 += tokens("output_tokens");
        jev_probs(&json, req.legal.len())
    }

    fn take_usage(&mut self) -> (u64, u64) {
        std::mem::take(&mut self.usage)
    }
}

/// A local program: one JSON request per line on its stdin, one reply per line on its stdout.
pub struct StdioJsonLines {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<std::io::Result<String>>,
    timeout: Duration,
}

impl StdioJsonLines {
    pub fn spawn(command: &[String], timeout: Duration) -> Result<StdioJsonLines, String> {
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("can't start {:?}: {e}", command[0]))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        // A reader thread, so a hung program can time out instead of blocking the sim.
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(StdioJsonLines {
            child,
            stdin,
            lines,
            timeout,
        })
    }
}

impl Drop for StdioJsonLines {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

impl Transport for StdioJsonLines {
    fn decide(&mut self, req: &DecisionRequest) -> Result<Vec<f64>, String> {
        let line = serde_json::to_string(req).map_err(|e| e.to_string())?;
        writeln!(self.stdin, "{line}").map_err(|e| format!("write: {e}"))?;
        self.stdin.flush().map_err(|e| format!("write: {e}"))?;
        match self.lines.recv_timeout(self.timeout) {
            Ok(Ok(reply)) => parse_reply(&reply),
            Ok(Err(e)) => Err(format!("read: {e}")),
            Err(RecvTimeoutError::Timeout) => Err("timed out".into()),
            Err(RecvTimeoutError::Disconnected) => Err("the program exited".into()),
        }
    }
}

/// A model behind an HTTP endpoint: POST the request as JSON, read the reply as JSON.
pub struct HttpJson {
    url: String,
    key: Option<String>,
    agent: ureq::Agent,
}

impl HttpJson {
    pub fn new(url: String, key: Option<String>, timeout: Duration) -> HttpJson {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .into();
        HttpJson { url, key, agent }
    }
}

impl Transport for HttpJson {
    fn decide(&mut self, req: &DecisionRequest) -> Result<Vec<f64>, String> {
        let mut call = self.agent.post(&self.url);
        if let Some(k) = &self.key {
            call = call.header("Authorization", &format!("Bearer {k}"));
        }
        let mut reply = call.send_json(req).map_err(|e| e.to_string())?;
        let status = reply.status();
        let body = reply
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!(
                "HTTP {status}: {}",
                body.chars().take(200).collect::<String>()
            ));
        }
        parse_reply(&body)
    }
}

/// Uniform over the legal actions (tests and dry runs).
pub struct Mock;

impl Transport for Mock {
    fn decide(&mut self, req: &DecisionRequest) -> Result<Vec<f64>, String> {
        Ok(vec![1.0; req.legal.len()])
    }
}

fn parse_reply(text: &str) -> Result<Vec<f64>, String> {
    match serde_json::from_str::<Reply>(text.trim()) {
        Ok(Reply::Probs { probs }) | Ok(Reply::Bare(probs)) => Ok(probs),
        Err(e) => Err(format!(
            "bad reply {:?}: {e}",
            text.chars().take(120).collect::<String>()
        )),
    }
}

/// Per-model counters (FR-BT-8 d).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Metrics {
    pub decisions: u64,
    pub cache_hits: u64,
    pub calls: u64,
    pub failures: u64,
    pub total_latency_ms: f64,
    pub max_latency_ms: f64,
    /// Billed tokens (Jev), for cost.
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Metrics {
    pub fn mean_latency_ms(&self) -> f64 {
        if self.calls == 0 {
            0.0
        } else {
            self.total_latency_ms / self.calls as f64
        }
    }
}

/// One loaded spec: its transports (a pool of `concurrency`), cache and counters, shared by
/// every bot built from it in this process.
struct Shared {
    spec: ExternalSpec,
    pool: Vec<Mutex<Option<Box<dyn Transport>>>>,
    cache: Mutex<HashMap<String, Vec<f64>>>,
    cache_file: Option<Mutex<std::fs::File>>,
    metrics: Mutex<Metrics>,
}

fn registry() -> &'static Mutex<HashMap<String, Arc<Shared>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<Shared>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

fn shared(path: &str) -> Arc<Shared> {
    let mut reg = registry().lock().unwrap();
    if let Some(s) = reg.get(path) {
        return s.clone();
    }
    let spec = ExternalSpec::load(path).unwrap_or_else(|e| panic!("external bot {e}"));
    let mut cache = HashMap::new();
    let mut cache_file = None;
    if let Some(p) = &spec.cache {
        if let Ok(text) = std::fs::read_to_string(p) {
            for line in text.lines() {
                if let Ok((k, v)) = serde_json::from_str::<(String, Vec<f64>)>(line) {
                    cache.insert(k, v);
                }
            }
        }
        if let Some(dir) = std::path::Path::new(p).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        cache_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .ok()
            .map(Mutex::new);
    }
    let pool = (0..spec.concurrency.max(1))
        .map(|_| Mutex::new(None))
        .collect();
    let s = Arc::new(Shared {
        spec,
        pool,
        cache: Mutex::new(cache),
        cache_file,
        metrics: Mutex::new(Metrics::default()),
    });
    reg.insert(path.to_string(), s.clone());
    s
}

/// Counters of every external model used in this process, by model name.
pub fn metrics() -> Vec<(String, Metrics)> {
    let reg = registry().lock().unwrap();
    let mut out: Vec<(String, Metrics)> = reg
        .values()
        .map(|s| (s.spec.model.clone(), s.metrics.lock().unwrap().clone()))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn key(req: &DecisionRequest) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(req).expect("requests serialize");
    let mut h = Sha256::new();
    h.update(&bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

impl Shared {
    /// Probabilities for `req` (cached), or why not.
    fn probs(&self, req: &DecisionRequest) -> Result<Vec<f64>, String> {
        let k = key(req);
        self.metrics.lock().unwrap().decisions += 1;
        if let Some(p) = self.cache.lock().unwrap().get(&k).cloned() {
            self.metrics.lock().unwrap().cache_hits += 1;
            return Ok(p);
        }
        // Take a free transport from the pool (start it on first use); wait on the first if all busy.
        let slot = self
            .pool
            .iter()
            .find_map(|m| m.try_lock().ok())
            .unwrap_or_else(|| self.pool[0].lock().unwrap());
        let mut slot = slot;
        if slot.is_none() {
            *slot = Some(self.spec.transport()?);
        }
        let start = Instant::now();
        let result = slot.as_mut().unwrap().decide(req);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let (tin, tout) = slot.as_mut().unwrap().take_usage();
        if result.is_err() && self.spec.transport == "stdio" {
            *slot = None; // restart a broken program next time
        }
        drop(slot);
        {
            let mut m = self.metrics.lock().unwrap();
            m.calls += 1;
            m.total_latency_ms += ms;
            m.max_latency_ms = m.max_latency_ms.max(ms);
            m.input_tokens += tin;
            m.output_tokens += tout;
        }
        let probs = result?;
        if probs.len() != req.legal.len()
            || probs.iter().any(|p| !p.is_finite() || *p < 0.0)
            || probs.iter().sum::<f64>() <= 0.0
        {
            return Err(format!(
                "expected {} non-negative probabilities, got {probs:?}",
                req.legal.len()
            ));
        }
        self.cache.lock().unwrap().insert(k.clone(), probs.clone());
        if let Some(f) = &self.cache_file
            && let Ok(line) = serde_json::to_string(&(&k, &probs))
        {
            let _ = writeln!(f.lock().unwrap(), "{line}");
        }
        Ok(probs)
    }
}

/// The bot: asks the external model, samples its answer with the seat's RNG, and falls back on
/// failure.
pub struct ExternalPolicy<G: Game> {
    shared: Arc<Shared>,
    fallback: Box<dyn Policy<G>>,
}

impl<G: Game> ExternalPolicy<G> {
    pub fn new(game: &G, path: &str) -> ExternalPolicy<G> {
        let shared = shared(path);
        let fallback: BotSpec = shared
            .spec
            .fallback
            .parse()
            .unwrap_or_else(|e| panic!("external bot {path}: fallback: {e}"));
        if matches!(fallback, BotSpec::External { .. }) {
            panic!("external bot {path}: the fallback can't be another external bot");
        }
        ExternalPolicy {
            fallback: fallback.build(game),
            shared,
        }
    }
}

impl<G: Game> Policy<G> for ExternalPolicy<G> {
    fn act(
        &mut self,
        game: &G,
        obs: &G::Observation,
        legal: &[G::Action],
        rng: &mut DetRng,
    ) -> G::Action {
        let m = game.meta();
        let req = DecisionRequest {
            model: self.shared.spec.model.clone(),
            game_id: m.id.into(),
            game_version: m.version,
            observation: serde_json::to_value(obs).expect("observations serialize"),
            legal: legal
                .iter()
                .map(|a| serde_json::to_value(a).expect("actions serialize"))
                .collect(),
            legal_labels: legal.iter().map(|&a| game.describe_action(a)).collect(),
        };
        match self.shared.probs(&req) {
            Ok(p) => {
                let weighted: Vec<(G::Action, f64)> = legal.iter().copied().zip(p).collect();
                engine::game::sample_weighted(&weighted, rng)
            }
            Err(e) => {
                self.shared.metrics.lock().unwrap().failures += 1;
                if std::env::var_os("DC_EXTERNAL_QUIET").is_none() {
                    eprintln!(
                        "external bot {}: {e}; using {}",
                        self.shared.spec.model, self.shared.spec.fallback
                    );
                }
                self.fallback.act(game, obs, legal, rng)
            }
        }
    }

    fn name(&self) -> String {
        format!("ext:{}", self.shared.spec.model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use games::kuhn::Kuhn;

    fn play(game: &Kuhn, bots: Vec<BotSpec>, deals: u64, seed: u64) -> Vec<f64> {
        let spec = crate::RunSpec {
            game: "kuhn".into(),
            bots,
            deals,
            seed,
            duplicate: true,
            history_sample: 0.0,
            stack: None,
            dealer_ante: false,
            study: false,
            balance: false,
            config: None,
        };
        crate::run(game, &spec, &mut crate::NullSink, |_, _| {})
            .bots
            .iter()
            .map(|b| b.per_hand)
            .collect()
    }

    fn spec_file(name: &str, body: &str) -> String {
        let dir = std::env::temp_dir().join(format!("dc-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn mock_external_bot_plays_and_caches() {
        let cache = spec_file("cache.jsonl", "");
        let path = spec_file(
            "mock.toml",
            &format!(
                "model = \"mock\"\ntransport = \"mock\"\nfallback = \"random\"\ncache = \"{cache}\"\n"
            ),
        );
        let spec: BotSpec = format!("ext:{path}").parse().unwrap();
        assert_eq!(spec.to_string(), format!("ext:{path}"));
        let game = Kuhn::new(Default::default());
        let run = || play(&game, vec![spec.clone(), BotSpec::Random], 200, 11);
        let first = run();
        let m1 = metrics().into_iter().find(|(k, _)| k == "mock").unwrap().1;
        assert!(
            m1.calls > 0 && m1.calls < m1.decisions,
            "Kuhn has few info sets: cache hits"
        );
        // Same seed, warm cache: identical results, no new calls.
        let second = run();
        assert_eq!(first, second);
        let m2 = metrics().into_iter().find(|(k, _)| k == "mock").unwrap().1;
        assert_eq!(m2.calls, m1.calls);
        assert!(std::fs::read_to_string(&cache).unwrap().lines().count() as u64 == m1.calls);
    }

    /// A local stand-in for TypeSafe's endpoint: checks the request shape and answers in the
    /// documented reply format, so the whole `jev` path runs without network or key.
    #[test]
    fn jev_transport_speaks_system_one() {
        use std::io::{Read, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut s = stream.unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                // Read headers + body (Content-Length).
                loop {
                    let n = s.read(&mut chunk).unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(i) = text.find("\r\n\r\n") {
                        let len: usize = text
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse().unwrap())
                            })
                            .unwrap_or(0);
                        if buf.len() >= i + 4 + len {
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&buf).to_string();
                assert!(
                    text.contains("authorization: Bearer test-key")
                        || text.contains("Authorization: Bearer test-key")
                );
                let body: serde_json::Value =
                    serde_json::from_str(&text[text.find("\r\n\r\n").unwrap() + 4..]).unwrap();
                assert_eq!(body["model"], "jev-latest");
                assert_eq!(body["questions"]["action"]["type"], "choice");
                let n = body["questions"]["action"]["criteria"]
                    .as_object()
                    .unwrap()
                    .len();
                // Prefer the last option.
                let probs: serde_json::Map<String, serde_json::Value> = (0..n)
                    .map(|i| {
                        (
                            format!("a{i}"),
                            serde_json::json!(if i == n - 1 { 1.0 } else { 0.0 }),
                        )
                    })
                    .collect();
                let reply = serde_json::json!({
                    "model": "jev-1.13.0",
                    "answers": {"action": {"type": "choice", "choice": format!("a{}", n - 1), "confidence": 1.0, "probabilities": probs}},
                    "usage": {"input_tokens": 400, "output_tokens": 20}
                })
                .to_string();
                write!(
                    s,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                )
                .unwrap();
            }
        });
        // SAFETY: only this test reads this variable.
        unsafe { std::env::set_var("DC_TEST_JEV_KEY", "test-key") };
        let path = spec_file(
            "jev.toml",
            &format!(
                "model = \"jev-test\"\ntransport = \"jev\"\nurl = \"{url}\"\napi_key_env = \"DC_TEST_JEV_KEY\"\nfallback = \"random\"\n"
            ),
        );
        let game = Kuhn::new(Default::default());
        play(
            &game,
            vec![format!("ext:{path}").parse().unwrap(), BotSpec::Random],
            30,
            5,
        );
        let m = metrics()
            .into_iter()
            .find(|(k, _)| k == "jev-test")
            .unwrap()
            .1;
        assert_eq!(m.failures, 0, "{m:?}");
        assert!(m.calls > 0);
        assert_eq!(m.input_tokens, 400 * m.calls);
        assert_eq!(m.output_tokens, 20 * m.calls);
    }

    #[test]
    fn bad_replies_use_the_fallback() {
        assert!(parse_reply("{\"probs\": [0.5, 0.5]}").is_ok());
        assert!(parse_reply("[1, 0]").is_ok());
        assert!(parse_reply("nope").is_err());
        let path = spec_file(
            "broken.toml",
            "model = \"broken\"\ntransport = \"stdio\"\ncommand = [\"this-program-does-not-exist-dc\"]\nfallback = \"random\"\ntimeout_ms = 200\n",
        );
        let game = Kuhn::new(Default::default());
        let bots: Vec<BotSpec> = vec![format!("ext:{path}").parse().unwrap(), BotSpec::Random];
        // SAFETY: tests in this module don't read the variable concurrently with writes elsewhere.
        unsafe { std::env::set_var("DC_EXTERNAL_QUIET", "1") };
        play(&game, bots, 20, 3);
        let m = metrics()
            .into_iter()
            .find(|(k, _)| k == "broken")
            .unwrap()
            .1;
        assert_eq!(m.failures, m.decisions);
    }
}
