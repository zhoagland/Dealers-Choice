//! dc-server: Dealer's Choice over the internet (docs/tech/web.md). Serves the React app, the REST
//! API, Discord sign-in and one WebSocket per tab; each table runs in its own thread.
//!
//! Configuration (environment, see `deploy/.env.example`):
//! `PUBLIC_URL`, `DISCORD_CLIENT_ID`, `DISCORD_CLIENT_SECRET`, `ALLOWED_GUILD_ID`,
//! `ADMIN_DISCORD_IDS` (comma-separated), `DC_DATA_ROOT`, `BIND_ADDR`, `STATIC_DIR`, `DEV_MODE`.

mod api;
mod auth;
mod catalog;
mod prompt;
mod table;
mod ws;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::routing::{get, post};
use tower_http::services::{ServeDir, ServeFile};

use storage::Db;

pub struct Config {
    pub public_url: String,
    pub discord_client_id: Option<String>,
    pub discord_client_secret: Option<String>,
    pub allowed_guild: Option<String>,
    pub admins: Vec<String>,
    pub data_root: PathBuf,
    pub bind: String,
    pub static_dir: PathBuf,
    pub dev_mode: bool,
}

impl Config {
    fn from_env() -> Result<Config, String> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let dev_mode = var("DEV_MODE").is_some_and(|v| v == "1" || v == "true");
        let c = Config {
            public_url: var("PUBLIC_URL").unwrap_or_else(|| "http://localhost:8080".into()),
            discord_client_id: var("DISCORD_CLIENT_ID"),
            discord_client_secret: var("DISCORD_CLIENT_SECRET"),
            allowed_guild: var("ALLOWED_GUILD_ID"),
            admins: var("ADMIN_DISCORD_IDS")
                .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default(),
            data_root: var("DC_DATA_ROOT").map_or_else(|| PathBuf::from("data"), PathBuf::from),
            bind: var("BIND_ADDR").unwrap_or_else(|| "127.0.0.1:8080".into()),
            static_dir: var("STATIC_DIR").map_or_else(|| PathBuf::from("web/dist"), PathBuf::from),
            dev_mode,
        };
        if !dev_mode {
            // D-WEB-2: production requires Discord sign-in and the guild allowlist.
            if c.discord_client_id.is_none() || c.discord_client_secret.is_none() {
                return Err(
                    "set DISCORD_CLIENT_ID and DISCORD_CLIENT_SECRET (or DEV_MODE=1)".into(),
                );
            }
            if c.allowed_guild.is_none() {
                return Err("set ALLOWED_GUILD_ID (or DEV_MODE=1)".into());
            }
        }
        Ok(c)
    }
}

pub struct AppState {
    pub config: Config,
    pub db: Arc<Mutex<Db>>,
    pub tables: Mutex<HashMap<String, table::TableHandle>>,
    /// OAuth `state` → (PKCE verifier, when).
    pub oauth: Mutex<HashMap<String, (String, Instant)>>,
    pub http: reqwest::Client,
    pub next_conn: AtomicU64,
}

pub type Shared = Arc<AppState>;

pub fn router(state: Shared) -> Router {
    let index = state.config.static_dir.join("index.html");
    let spa = ServeDir::new(&state.config.static_dir).fallback(ServeFile::new(index));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/auth/discord/login", get(auth::discord_login))
        .route("/auth/discord/callback", get(auth::discord_callback))
        .route("/auth/dev", get(auth::dev_login))
        .route("/auth/logout", post(auth::logout))
        .route("/api/discord/token", post(auth::activity_token))
        .route("/api/config", get(api::config))
        .route("/api/me", get(api::me))
        .route("/api/games", get(api::games))
        .route("/api/tables", get(api::tables).post(api::create_table))
        .route("/api/tables/{id}", get(api::table))
        .route("/api/tables/{id}/close", post(api::close_table))
        .route("/api/tables/{id}/hands", get(api::table_hands))
        .route("/api/tables/{id}/settle", get(api::settle))
        .route("/api/hands", get(api::my_hands))
        .route("/api/hands/{id}", get(api::hand))
        .route("/api/admin/accounts", get(api::admin_accounts))
        .route("/api/admin/bankroll", post(api::admin_bankroll))
        .route("/ws", get(ws::ws))
        .fallback_service(spa)
        .with_state(state)
}

/// Reopen the tables that were open when the server stopped, paying back the interrupted game
/// first (D-WEB-6).
fn restore_tables(state: &AppState) {
    let rows = state
        .db
        .lock()
        .unwrap()
        .open_web_tables()
        .unwrap_or_default();
    for row in rows {
        let Ok(settings) = serde_json::from_str::<table::Settings>(&row.settings) else {
            let _ = state.db.lock().unwrap().close_web_table(&row.id);
            continue;
        };
        table::apply_refund(
            &mut state.db.lock().unwrap(),
            &row.id,
            &row.refund,
            row.session_id,
        );
        let handle = table::spawn(
            row.id.clone(),
            row.name,
            row.host,
            settings,
            state.db.clone(),
            row.session_id,
        );
        state.tables.lock().unwrap().insert(row.id, handle);
    }
}

/// Nightly `VACUUM INTO` snapshots in `<data>/backups`, newest 7 kept (FR-WB-10).
fn backup(state: &AppState) {
    let dir = state.config.data_root.join("backups");
    let day = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400);
    let path = dir.join(format!("dc-day{day:06}.sqlite"));
    if let Err(e) = state.db.lock().unwrap().backup_to(&path) {
        eprintln!("backup failed: {}", e.0);
        return;
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|it| {
            it.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("dc-day"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    while files.len() > 7 {
        let _ = std::fs::remove_file(files.remove(0));
    }
}

/// `dc-server --healthcheck`: exit 0 if this container's server answers `/healthz` (for
/// Docker's HEALTHCHECK; the image has no curl).
fn healthcheck() -> i32 {
    use std::io::{Read, Write};
    let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let addr = bind.replace("0.0.0.0", "127.0.0.1");
    let Ok(mut s) = std::net::TcpStream::connect(&addr) else {
        return 1;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    if write!(
        s,
        "GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .is_err()
    {
        return 1;
    }
    let mut reply = String::new();
    let _ = s.read_to_string(&mut reply);
    if reply.starts_with("HTTP/1.1 200") {
        0
    } else {
        1
    }
}

/// `dc-server --backup <file>`: a consistent snapshot of the database (safe while the server
/// runs), e.g. before a deploy.
fn backup_once(path: Option<&String>) -> i32 {
    let Some(path) = path else {
        eprintln!("dc-server: --backup needs a file path");
        return 2;
    };
    let root = std::env::var("DC_DATA_ROOT").unwrap_or_else(|_| "data".into());
    let db = match Db::open(&PathBuf::from(root).join("dc.sqlite")) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("dc-server: database: {}", e.0);
            return 1;
        }
    };
    match db.backup_to(std::path::Path::new(path)) {
        Ok(()) => {
            println!("backed up to {path}");
            0
        }
        Err(e) => {
            eprintln!("dc-server: backup: {}", e.0);
            1
        }
    }
}

#[tokio::main]
async fn main() {
    // Maintenance commands for containers and deploy scripts (no web server started).
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--healthcheck") => std::process::exit(healthcheck()),
        Some("--backup") => std::process::exit(backup_once(args.get(1))),
        Some(other) => {
            eprintln!(
                "dc-server: unknown argument {other:?} (use --healthcheck or --backup <file>)"
            );
            std::process::exit(2);
        }
        None => {}
    }
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("dc-server: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = std::fs::create_dir_all(&config.data_root) {
        eprintln!(
            "dc-server: can't create {}: {e}",
            config.data_root.display()
        );
        std::process::exit(2);
    }
    let db = match Db::open(&config.data_root.join("dc.sqlite")) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("dc-server: database: {}", e.0);
            std::process::exit(2);
        }
    };
    let bind = config.bind.clone();
    let state: Shared = Arc::new(AppState {
        config,
        db: Arc::new(Mutex::new(db)),
        tables: Mutex::new(HashMap::new()),
        oauth: Mutex::new(HashMap::new()),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("http client"),
        next_conn: AtomicU64::new(1),
    });
    restore_tables(&state);
    {
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                let s = state.clone();
                let _ = tokio::task::spawn_blocking(move || backup(&s)).await;
                tokio::time::sleep(Duration::from_secs(86_400)).await;
            }
        });
    }
    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("dc-server: can't listen on {bind}: {e}");
            std::process::exit(2);
        }
    };
    println!(
        "dc-server listening on {bind} ({}){}",
        state.config.public_url,
        if state.config.dev_mode {
            " — DEV_MODE: /auth/dev?name=… signs in without Discord"
        } else {
            ""
        }
    );
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("server");
}

#[cfg(test)]
mod tests;
