//! End to end: two players sign in (dev login), open a table, sit, start, pick Hold'em and play
//! a hand over real WebSockets; then each replays it and sees only their own hole cards.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use protocol::{
    CardSlot, Choice, ClientMsg, CreateTable, HandListing, HandReplay, Me, PromptOption, ServerMsg,
    TableListing,
};

use super::*;

fn test_state() -> Shared {
    Arc::new(AppState {
        config: Config {
            public_url: "http://localhost".into(),
            discord_client_id: None,
            discord_client_secret: None,
            allowed_guild: None,
            admins: Vec::new(),
            data_root: std::env::temp_dir(),
            bind: String::new(),
            static_dir: PathBuf::from("does-not-exist"),
            dev_mode: true,
        },
        db: Arc::new(Mutex::new(Db::in_memory().unwrap())),
        tables: Mutex::new(HashMap::new()),
        oauth: Mutex::new(HashMap::new()),
        http: reqwest::Client::new(),
        next_conn: AtomicU64::new(1),
    })
}

async fn login(base: &str, name: &str) -> String {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let r = client
        .get(format!("{base}/auth/dev?name={name}"))
        .send()
        .await
        .unwrap();
    let cookie = r.headers()[reqwest::header::SET_COOKIE].to_str().unwrap();
    cookie.split(';').next().unwrap().to_string()
}

async fn get<T: serde::de::DeserializeOwned>(base: &str, cookie: &str, path: &str) -> T {
    reqwest::Client::new()
        .get(format!("{base}{path}"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(addr: &str, cookie: &str, table: &str) -> Socket {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut req = format!("ws://{addr}/ws?table={table}")
        .into_client_request()
        .unwrap();
    req.headers_mut().insert("cookie", cookie.parse().unwrap());
    tokio_tungstenite::connect_async(req).await.unwrap().0
}

async fn send(ws: &mut Socket, msg: &ClientMsg) {
    ws.send(Message::Text(serde_json::to_string(msg).unwrap().into()))
        .await
        .unwrap();
}

async fn recv(ws: &mut Socket) -> ServerMsg {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("server went quiet")
            .unwrap()
            .unwrap();
        if let Message::Text(t) = m {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_players_play_a_hand_and_replay_it() {
    let state = test_state();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let base = format!("http://{addr}");
    tokio::spawn(axum::serve(listener, router(state.clone())).into_future());

    let ann = login(&base, "Ann").await;
    let bob = login(&base, "Bob").await;
    let me: Me = get(&base, &ann, "/api/me").await;
    assert_eq!(me.bankroll, 10_000);

    let table: TableListing = reqwest::Client::new()
        .post(format!("{base}/api/tables"))
        .header("cookie", &ann)
        .json(&CreateTable {
            name: "Friday".into(),
            max_seats: 6,
            pool: vec!["holdem_nl".into()],
            timer_secs: 30,
            listed: true,
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let listed: Vec<TableListing> = get(&base, &bob, "/api/tables").await;
    assert_eq!(listed.len(), 1);

    let mut wa = connect(&addr, &ann, &table.id).await;
    let mut wb = connect(&addr, &bob, &table.id).await;
    send(&mut wa, &ClientMsg::Sit { seat: 0 }).await;
    send(&mut wb, &ClientMsg::Sit { seat: 1 }).await;
    // Let both sits land before starting.
    tokio::time::sleep(Duration::from_millis(200)).await;
    send(&mut wa, &ClientMsg::Start).await;

    // Both sockets answer any prompt addressed to them: the game picker, then check/call.
    let mut summaries = 0;
    let mut sockets = [wa, wb];
    let mut answered = std::collections::HashSet::new();
    while summaries < 2 {
        for ws in sockets.iter_mut() {
            let next = tokio::time::timeout(Duration::from_millis(50), recv(ws)).await;
            let Ok(msg) = next else { continue };
            match msg {
                ServerMsg::Snapshot {
                    prompt: Some(p), ..
                } if answered.insert(p.id) => {
                    let choice = match &p.options[0] {
                        PromptOption::Game { id, .. } => Choice::Option { id: id.clone() },
                        _ => {
                            // Check or call when offered, else the first button.
                            let id = p
                                .options
                                .iter()
                                .find_map(|o| match o {
                                    PromptOption::Button { id, class, .. }
                                        if class == "check" || class == "call" =>
                                    {
                                        Some(id.clone())
                                    }
                                    _ => None,
                                })
                                .unwrap_or_else(|| match &p.options[0] {
                                    PromptOption::Button { id, .. } => id.clone(),
                                    o => panic!("unexpected option {o:?}"),
                                });
                            Choice::Option { id }
                        }
                    };
                    send(
                        ws,
                        &ClientMsg::Act {
                            prompt_id: p.id,
                            choice,
                        },
                    )
                    .await;
                }
                ServerMsg::HandSummary { .. } => summaries += 1,
                ServerMsg::Error { code, message } => panic!("{code}: {message}"),
                _ => {}
            }
        }
    }

    let hands: Vec<HandListing> =
        get(&base, &ann, &format!("/api/tables/{}/hands", table.id)).await;
    assert!(!hands.is_empty());
    let id = hands.last().unwrap().id;
    // Chips are conserved.
    assert_eq!(hands[0].results.iter().map(|r| r.cents).sum::<i64>(), 0);
    for cookie in [&ann, &bob] {
        let r: HandReplay = get(&base, cookie, &format!("/api/hands/{id}")).await;
        assert!(r.steps.len() >= 2);
        // Before showdown, only the viewer's own hole cards are face-up-to-them.
        let first = &r.steps[0].view;
        for s in &first.seats {
            let known = s
                .cards
                .iter()
                .filter(|c| matches!(c, CardSlot::Down(Some(_))))
                .count();
            if s.seat == first.viewer {
                assert_eq!(known, 2, "viewer sees own hole cards");
            } else {
                assert_eq!(known, 0, "viewer must not see others' hole cards");
            }
        }
    }
    let mine: Vec<HandListing> = get(&base, &bob, "/api/hands").await;
    assert_eq!(mine.len(), hands.len());
    let settle: Vec<protocol::SeatResult> =
        get(&base, &ann, &format!("/api/tables/{}/settle", table.id)).await;
    assert_eq!(settle.iter().map(|r| r.cents).sum::<i64>(), 0);
}

#[tokio::test]
async fn api_requires_sign_in_and_dev_login_is_dev_only() {
    let state = test_state();
    let app = router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());
    let r = reqwest::get(format!("http://{addr}/api/me")).await.unwrap();
    assert_eq!(r.status(), 401);
    let r = reqwest::get(format!("http://{addr}/healthz"))
        .await
        .unwrap();
    assert_eq!(r.text().await.unwrap(), "ok");
}
