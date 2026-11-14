//! REST endpoints (web.md §5–7): who am I, the game catalog, tables (lobby), hand history with
//! per-viewer replay, settle-up, and the admin bankroll tools.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use serde::Deserialize;

use engine::{ActionClass, Chips, Game, Seat, ToAct};
use games::registry::{GameVisitor, with_game};
use protocol::{
    AdminAccount, BankrollChange, CreateTable, GameInfo, HandListing, HandReplay, Me, ReplayStep,
    SeatResult, TableListing,
};
use sim::encoding::{HandRecord, decode_all, decode_carry};
use storage::{Account, LedgerKind, WebHandRow};

use crate::table::{Cmd, Settings};
use crate::{AppState, Shared, auth, catalog, table};

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

fn err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (code, msg.into())
}

fn db_err(e: storage::DbError) -> (StatusCode, String) {
    err(StatusCode::INTERNAL_SERVER_ERROR, e.0)
}

/// The signed-in account; 401 otherwise.
pub struct Authed(pub Account);

impl FromRequestParts<Shared> for Authed {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Shared,
    ) -> Result<Self, Self::Rejection> {
        auth::account(state, &parts.headers, None)
            .map(Authed)
            .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "sign in first"))
    }
}

pub async fn me(State(state): State<Shared>, Authed(acc): Authed) -> ApiResult<Me> {
    let bankroll = state
        .db
        .lock()
        .unwrap()
        .bankroll(acc.player_id)
        .map_err(db_err)?;
    Ok(Json(Me {
        id: acc.id,
        name: acc.name,
        avatar: acc.avatar,
        is_admin: acc.is_admin,
        bankroll: bankroll.0,
    }))
}

/// `GET /api/avatar/{account}`: the account's Discord profile picture, fetched through this
/// server so it loads inside Discord's Activity frame (which blocks other hosts). No sign-in:
/// `<img>` requests carry no bearer token, and Discord avatars are public anyway.
pub async fn avatar(
    State(state): State<Shared>,
    Path(id): Path<i64>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    use axum::response::IntoResponse;
    let acc = state.db.lock().unwrap().account(id).map_err(db_err)?;
    let Some((discord_id, hash)) = acc.and_then(|a| Some((a.discord_id, a.avatar?))) else {
        return Err(err(StatusCode::NOT_FOUND, "no avatar"));
    };
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !ok(&discord_id) || !ok(&hash) {
        return Err(err(StatusCode::NOT_FOUND, "no avatar"));
    }
    let url = format!("https://cdn.discordapp.com/avatars/{discord_id}/{hash}.png?size=128");
    let reply = state
        .http
        .get(url)
        .send()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, e.to_string()))?;
    if !reply.status().is_success() {
        return Err(err(StatusCode::NOT_FOUND, "no avatar"));
    }
    let bytes = reply
        .bytes()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "image/png"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        bytes,
    )
        .into_response())
}

pub async fn config(State(state): State<Shared>) -> Json<protocol::ServerConfig> {
    Json(protocol::ServerConfig {
        discord_client_id: state.config.discord_client_id.clone(),
        dev_mode: state.config.dev_mode,
    })
}

pub async fn games(Authed(_): Authed) -> Json<Vec<GameInfo>> {
    Json(catalog::playable(&[]))
}

/// Listed tables, plus the ones you host (everything for admins).
pub async fn tables(State(state): State<Shared>, Authed(acc): Authed) -> Json<Vec<TableListing>> {
    let tables = state.tables.lock().unwrap();
    let mut out: Vec<TableListing> = tables
        .values()
        .filter_map(|t| {
            let l = t.listing.lock().unwrap().clone();
            (l.listed || t.host == acc.id || acc.is_admin).then_some(l)
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Json(out)
}

pub async fn table(
    State(state): State<Shared>,
    Authed(_): Authed,
    Path(id): Path<String>,
) -> ApiResult<TableListing> {
    let tables = state.tables.lock().unwrap();
    let t = tables
        .get(&id)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such table (it may have closed)"))?;
    Ok(Json(t.listing.lock().unwrap().clone()))
}

/// Open a table and start its thread.
pub fn open_table(
    state: &AppState,
    id: &str,
    name: &str,
    host: i64,
    settings: Settings,
) -> Result<TableListing, String> {
    let session = {
        let mut db = state.db.lock().unwrap();
        let session = db.start_session(0).map_err(|e| e.0)?;
        let json = serde_json::to_string(&settings).unwrap();
        db.create_web_table(id, name, host, &json, session)
            .map_err(|e| e.0)?;
        session
    };
    let handle = table::spawn(
        id.into(),
        name.into(),
        host,
        settings,
        state.db.clone(),
        session,
    );
    let listing = handle.listing.lock().unwrap().clone();
    state.tables.lock().unwrap().insert(id.into(), handle);
    Ok(listing)
}

pub async fn create_table(
    State(state): State<Shared>,
    Authed(acc): Authed,
    Json(body): Json<CreateTable>,
) -> ApiResult<TableListing> {
    let name: String = body.name.trim().chars().take(40).collect();
    if name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "give the table a name"));
    }
    if !(2..=9).contains(&body.max_seats) {
        return Err(err(StatusCode::BAD_REQUEST, "tables seat 2 to 9 players"));
    }
    let known: Vec<String> = catalog::playable(&[]).into_iter().map(|g| g.id).collect();
    if let Some(bad) = body.pool.iter().find(|g| !known.contains(g)) {
        return Err(err(StatusCode::BAD_REQUEST, format!("unknown game {bad}")));
    }
    let settings = Settings {
        max_seats: body.max_seats,
        pool: body.pool,
        timer_secs: body.timer_secs.clamp(10, 120),
        bank_secs: 60,
        listed: body.listed,
    };
    let id = auth::random_token(6);
    open_table(&state, &id, &name, acc.id, settings)
        .map(Json)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub async fn close_table(
    State(state): State<Shared>,
    Authed(acc): Authed,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut tables = state.tables.lock().unwrap();
    let t = tables
        .get(&id)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such table"))?;
    if t.host != acc.id && !acc.is_admin {
        return Err(err(
            StatusCode::FORBIDDEN,
            "only the host can close the table",
        ));
    }
    let _ = t.tx.send(Cmd::Close);
    tables.remove(&id);
    state
        .db
        .lock()
        .unwrap()
        .close_web_table(&id)
        .map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// The Discord Activity's table: one per Activity instance (web.md §13), opened on first use.
pub fn activity_table(state: &AppState, instance: &str, acc: &Account) -> Result<String, String> {
    if instance.is_empty() {
        return Err("missing Activity instance".into());
    }
    let id = format!("act-{instance}");
    if state.tables.lock().unwrap().contains_key(&id) {
        return Ok(id);
    }
    let settings = Settings {
        max_seats: 9,
        pool: Vec::new(),
        timer_secs: 30,
        bank_secs: 60,
        listed: false,
    };
    open_table(state, &id, "Voice channel table", acc.id, settings)?;
    Ok(id)
}

// ----- hand history ---------------------------------------------------------------------------

fn seat_ids(row: &WebHandRow) -> Vec<Option<i64>> {
    serde_json::from_str(&row.seats).unwrap_or_default()
}

fn names(state: &AppState, ids: &[Option<i64>]) -> Vec<String> {
    let db = state.db.lock().unwrap();
    ids.iter()
        .map(|id| {
            id.and_then(|id| db.account(id).ok().flatten())
                .map_or_else(|| "?".to_string(), |a| a.name)
        })
        .collect()
}

fn listing(state: &AppState, row: &WebHandRow) -> HandListing {
    let ids = seat_ids(row);
    let names = names(state, &ids);
    let cents: Vec<i64> = serde_json::from_str(&row.results).unwrap_or_default();
    HandListing {
        id: row.id,
        game: catalog::playable(std::slice::from_ref(&row.game_id))
            .first()
            .map_or(row.game_id.clone(), |g| g.name.clone()),
        at: row.at,
        results: names
            .into_iter()
            .zip(cents)
            .map(|(name, cents)| SeatResult { name, cents })
            .collect(),
    }
}

pub async fn table_hands(
    State(state): State<Shared>,
    Authed(_): Authed,
    Path(id): Path<String>,
) -> ApiResult<Vec<HandListing>> {
    let rows = state
        .db
        .lock()
        .unwrap()
        .web_hands(&id, 200)
        .map_err(db_err)?;
    Ok(Json(rows.iter().map(|r| listing(&state, r)).collect()))
}

pub async fn my_hands(
    State(state): State<Shared>,
    Authed(acc): Authed,
) -> ApiResult<Vec<HandListing>> {
    let rows = state
        .db
        .lock()
        .unwrap()
        .web_hands_for(acc.id, 200)
        .map_err(db_err)?;
    Ok(Json(rows.iter().map(|r| listing(&state, r)).collect()))
}

/// Replays a stored hand and produces the views of one seat only (FR-WB-8).
struct Replay {
    record: Vec<u8>,
    seat: u8,
    names: Vec<String>,
}

impl GameVisitor<Result<(String, Vec<ReplayStep>, String), String>> for Replay {
    fn visit<G: Game>(self, game: G) -> Result<(String, Vec<ReplayStep>, String), String> {
        let recs: Vec<HandRecord<G::Action>> = decode_all(&self.record)?;
        let rec = recs.into_iter().next().ok_or("empty hand record")?;
        if rec.game_version != game.meta().version {
            return Err("this hand was played under older rules and can't be replayed".into());
        }
        let carry: G::Carry = decode_carry(&rec.carry_in)?;
        let me = Seat(self.seat);
        let mut s = game.new_hand_with(&rec.setup, &carry);
        let mut steps = Vec::new();
        let mut dealt = false;
        let view = |s: &G::State| game.view(&game.observe(s, me));
        for &a in &rec.actions {
            match game.to_act(&s) {
                ToAct::Terminal => break,
                ToAct::Chance => {
                    game.apply(&mut s, a);
                    dealt = true;
                }
                ToAct::Seat(p) => {
                    if dealt {
                        steps.push(ReplayStep {
                            label: "Deal".into(),
                            view: view(&s),
                        });
                        dealt = false;
                    }
                    let what = if p == me {
                        game.describe_action(a)
                    } else {
                        match game.classify(&s, a) {
                            ActionClass::CardChoice => "chooses cards".into(),
                            ActionClass::Declare => "declares".into(),
                            _ => game.describe_action(a),
                        }
                    };
                    let who = self.names.get(p.index()).cloned().unwrap_or_default();
                    game.apply(&mut s, a);
                    steps.push(ReplayStep {
                        label: format!("{who}: {what}"),
                        view: view(&s),
                    });
                }
            }
        }
        if dealt || steps.is_empty() {
            steps.push(ReplayStep {
                label: "End".into(),
                view: view(&s),
            });
        }
        let text = game.describe_observation(&game.observe(&s, me));
        Ok((game.meta().display_name.to_string(), steps, text))
    }
}

pub async fn hand(
    State(state): State<Shared>,
    Authed(acc): Authed,
    Path(id): Path<i64>,
) -> ApiResult<HandReplay> {
    let row = state
        .db
        .lock()
        .unwrap()
        .web_hand(id)
        .map_err(db_err)?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such hand"))?;
    let ids = seat_ids(&row);
    let seat = ids.iter().position(|&a| a == Some(acc.id)).ok_or_else(|| {
        err(
            StatusCode::FORBIDDEN,
            "you can replay only hands you played",
        )
    })?;
    let names = names(&state, &ids);
    let replay = Replay {
        record: row.record.clone(),
        seat: seat as u8,
        names: names.clone(),
    };
    let (game, steps, text) = with_game(&row.game_id, None, replay)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(HandReplay {
        id: row.id,
        game,
        steps,
        text,
        names,
    }))
}

#[derive(Deserialize)]
pub struct Since {
    /// Unix seconds; default: the last 24 hours.
    since: Option<i64>,
}

/// Settle-up (FR-WB-5): each player's net result at a table since `since`.
pub async fn settle(
    State(state): State<Shared>,
    Authed(_): Authed,
    Path(id): Path<String>,
    Query(q): Query<Since>,
) -> ApiResult<Vec<SeatResult>> {
    let since = q.since.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64)
            - 86_400
    });
    let rows = state
        .db
        .lock()
        .unwrap()
        .web_hands(&id, 100_000)
        .map_err(db_err)?;
    let mut net: BTreeMap<String, i64> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.at >= since) {
        for s in listing(&state, r).results {
            *net.entry(s.name).or_default() += s.cents;
        }
    }
    let mut out: Vec<SeatResult> = net
        .into_iter()
        .map(|(name, cents)| SeatResult { name, cents })
        .collect();
    out.sort_by_key(|s| -s.cents);
    Ok(Json(out))
}

// ----- admin ----------------------------------------------------------------------------------

fn admin(acc: &Account) -> Result<(), (StatusCode, String)> {
    if acc.is_admin {
        Ok(())
    } else {
        Err(err(StatusCode::FORBIDDEN, "admins only"))
    }
}

pub async fn admin_accounts(
    State(state): State<Shared>,
    Authed(acc): Authed,
) -> ApiResult<Vec<AdminAccount>> {
    admin(&acc)?;
    let rows = state.db.lock().unwrap().accounts().map_err(db_err)?;
    Ok(Json(
        rows.into_iter()
            .map(|(a, b)| AdminAccount {
                id: a.id,
                name: a.name,
                is_admin: a.is_admin,
                bankroll: b.0,
            })
            .collect(),
    ))
}

pub async fn admin_bankroll(
    State(state): State<Shared>,
    Authed(acc): Authed,
    Json(body): Json<BankrollChange>,
) -> ApiResult<AdminAccount> {
    admin(&acc)?;
    let mut db = state.db.lock().unwrap();
    let target = db
        .account(body.account)
        .map_err(db_err)?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such account"))?;
    match body.op.as_str() {
        "deposit" if body.cents > 0 => db
            .ledger(
                target.player_id,
                None,
                &[(LedgerKind::Deposit, Chips(body.cents))],
            )
            .map_err(db_err)?,
        "reset" if body.cents >= 0 => db
            .reset_bankroll(target.player_id, Chips(body.cents))
            .map_err(db_err)?,
        _ => {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "op is deposit (> 0) or reset (>= 0)",
            ));
        }
    }
    let bankroll = db.bankroll(target.player_id).map_err(db_err)?;
    Ok(Json(AdminAccount {
        id: target.id,
        name: target.name,
        is_admin: target.is_admin,
        bankroll: bankroll.0,
    }))
}
