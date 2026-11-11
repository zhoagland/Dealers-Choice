//! Sign-in (web.md §5, FR-WB-2): Discord OAuth2 (authorization code + PKCE + `state`), the
//! guild allowlist, sessions as a random 256-bit cookie stored hashed, a dev login for local
//! testing, and the Discord Activity token exchange (web.md §13).

use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use engine::Chips;
use protocol::ActivityToken;
use storage::Account;

use crate::AppState;

/// Sessions last 30 days, sliding (web.md §5).
pub const SESSION_TTL: i64 = 30 * 86_400;
const COOKIE: &str = "dc_session";
/// New accounts start with $100 (D-WEB-3).
pub const OPENING: Chips = Chips(10_000);

pub fn random_token(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).expect("OS random numbers");
    URL_SAFE_NO_PAD.encode(b)
}

fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// The signed-in account, from the session cookie or (Discord Activity) a bearer token.
pub fn account(
    state: &AppState,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Option<Account> {
    let from_cookie = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_string());
    let from_bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    let token = from_cookie
        .or(from_bearer)
        .or(query_token.map(str::to_string))?;
    let mut db = state.db.lock().unwrap();
    let id = db.web_session(&hash(&token), SESSION_TTL).ok()??;
    db.account(id).ok()?
}

fn new_session(state: &AppState, account: &Account) -> String {
    let token = random_token(32);
    let _ = state
        .db
        .lock()
        .unwrap()
        .create_web_session(&hash(&token), account.id, SESSION_TTL);
    token
}

fn session_cookie(state: &AppState, token: &str, max_age: i64) -> String {
    let secure = if state.config.public_url.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    format!("{COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}{secure}")
}

fn signed_in(state: &AppState, account: &Account) -> Response {
    let token = new_session(state, account);
    (
        [(
            header::SET_COOKIE,
            session_cookie(state, &token, SESSION_TTL),
        )],
        Redirect::to("/"),
    )
        .into_response()
}

fn redirect_uri(state: &AppState) -> String {
    format!(
        "{}/auth/discord/callback",
        state.config.public_url.trim_end_matches('/')
    )
}

/// `GET /auth/discord/login`
pub async fn discord_login(State(state): State<crate::Shared>) -> Response {
    let Some(client_id) = state.config.discord_client_id.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Discord sign-in isn't configured",
        )
            .into_response();
    };
    let st = random_token(16);
    let verifier = random_token(32);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    {
        let mut pending = state.oauth.lock().unwrap();
        pending.retain(|_, (_, t)| t.elapsed() < Duration::from_secs(600));
        pending.insert(st.clone(), (verifier, Instant::now()));
    }
    let scope = if state.config.allowed_guild.is_some() {
        "identify guilds"
    } else {
        "identify"
    };
    let url = format!(
        "https://discord.com/oauth2/authorize?response_type=code&client_id={}&scope={}&state={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&prompt=none",
        enc(&client_id),
        enc(scope),
        enc(&st),
        enc(&redirect_uri(&state)),
        enc(&challenge)
    );
    Redirect::to(&url).into_response()
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[derive(Deserialize)]
pub struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
}

#[derive(Deserialize)]
struct DiscordUser {
    id: String,
    username: String,
    global_name: Option<String>,
    avatar: Option<String>,
}

#[derive(Deserialize)]
struct Guild {
    id: String,
}

/// Exchange an authorization code for an access token.
async fn exchange(
    state: &AppState,
    code: &str,
    verifier: Option<&str>,
    redirect: bool,
) -> Result<String, String> {
    let (Some(id), Some(secret)) = (
        &state.config.discord_client_id,
        &state.config.discord_client_secret,
    ) else {
        return Err("Discord sign-in isn't configured".into());
    };
    let mut form = vec![
        ("client_id", id.clone()),
        ("client_secret", secret.clone()),
        ("grant_type", "authorization_code".into()),
        ("code", code.into()),
    ];
    if redirect {
        form.push(("redirect_uri", redirect_uri(state)));
    }
    if let Some(v) = verifier {
        form.push(("code_verifier", v.into()));
    }
    let reply = state
        .http
        .post("https://discord.com/api/oauth2/token")
        .form(&form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !reply.status().is_success() {
        return Err(format!("Discord refused the code ({})", reply.status()));
    }
    Ok(reply
        .json::<TokenReply>()
        .await
        .map_err(|e| e.to_string())?
        .access_token)
}

/// The Discord user behind `token`, checked against the guild allowlist; their account.
async fn account_for(state: &AppState, token: &str) -> Result<Account, (StatusCode, String)> {
    let get = |url: &'static str| state.http.get(url).bearer_auth(token).send();
    let user: DiscordUser = get("https://discord.com/api/users/@me")
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?
        .json()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    if let Some(guild) = &state.config.allowed_guild {
        let guilds: Vec<Guild> = get("https://discord.com/api/users/@me/guilds")
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?
            .json()
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
        if !guilds.iter().any(|g| &g.id == guild) {
            return Err((
                StatusCode::FORBIDDEN,
                "This table is for members of the host's Discord server.".into(),
            ));
        }
    }
    let name = user.global_name.unwrap_or(user.username);
    let admin = state.config.admins.contains(&user.id);
    state
        .db
        .lock()
        .unwrap()
        .upsert_account(&user.id, &name, user.avatar.as_deref(), admin, OPENING)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.0))
}

/// `GET /auth/discord/callback`
pub async fn discord_callback(
    State(state): State<crate::Shared>,
    Query(q): Query<Callback>,
) -> Response {
    if let Some(e) = q.error {
        return (
            StatusCode::UNAUTHORIZED,
            format!("Discord sign-in was cancelled: {e}"),
        )
            .into_response();
    }
    let (Some(code), Some(st)) = (q.code, q.state) else {
        return (StatusCode::BAD_REQUEST, "missing code").into_response();
    };
    let verifier = state.oauth.lock().unwrap().remove(&st);
    let Some((verifier, at)) = verifier else {
        return (StatusCode::BAD_REQUEST, "sign-in expired; try again").into_response();
    };
    if at.elapsed() > Duration::from_secs(600) {
        return (StatusCode::BAD_REQUEST, "sign-in expired; try again").into_response();
    }
    let token = match exchange(&state, &code, Some(&verifier), true).await {
        Ok(t) => t,
        Err(e) => return (StatusCode::UNAUTHORIZED, e).into_response(),
    };
    match account_for(&state, &token).await {
        Ok(acc) => signed_in(&state, &acc),
        Err((code, msg)) => (code, msg).into_response(),
    }
}

#[derive(Deserialize)]
pub struct DevLogin {
    name: String,
}

/// `GET /auth/dev?name=Ann` — local testing only (`DEV_MODE=1`).
pub async fn dev_login(State(state): State<crate::Shared>, Query(q): Query<DevLogin>) -> Response {
    if !state.config.dev_mode {
        return StatusCode::NOT_FOUND.into_response();
    }
    let name: String = q.name.trim().chars().take(32).collect();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "name required").into_response();
    }
    let acc =
        state
            .db
            .lock()
            .unwrap()
            .upsert_account(&format!("dev:{name}"), &name, None, true, OPENING);
    match acc {
        Ok(acc) => signed_in(&state, &acc),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.0).into_response(),
    }
}

/// `POST /auth/logout`
pub async fn logout(State(state): State<crate::Shared>, headers: HeaderMap) -> Response {
    if let Some(v) = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|kv| kv.trim().strip_prefix("dc_session="))
        })
    {
        let _ = state.db.lock().unwrap().delete_web_session(&hash(v));
    }
    (
        [(header::SET_COOKIE, session_cookie(&state, "", 0))],
        StatusCode::NO_CONTENT,
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct ActivityCode {
    code: String,
    /// The Activity instance (one table per instance, web.md §13).
    instance: String,
}

/// `POST /api/discord/token` — Discord Activity sign-in: the SDK's code becomes a Discord access
/// token (for the SDK's `authenticate`) and a session token for this app.
pub async fn activity_token(
    State(state): State<crate::Shared>,
    Json(body): Json<ActivityCode>,
) -> Result<Json<ActivityToken>, (StatusCode, String)> {
    let token = exchange(&state, &body.code, None, false)
        .await
        .map_err(|e| (StatusCode::UNAUTHORIZED, e))?;
    let acc = account_for(&state, &token).await?;
    let session = new_session(&state, &acc);
    let instance: String = body
        .instance
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    let table = crate::api::activity_table(&state, &instance, &acc)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(ActivityToken {
        access_token: token,
        session,
        table,
    }))
}
