//! S-19 語音節點登錄：選節點、健康探測、連線重試、管理 API、節點密鑰與內部端點隔離。
//! 全部使用合成資料與假 AI，不連真的 cascade。

mod common;

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use common::Script::*;
use common::*;
use futures_util::StreamExt;
use serde_json::{Value, json};
use socrates_chat_backend::{
    AppState, app,
    cascade::{OpenError, voice_session_prompt},
    config::Config,
    internal_app,
    voice_nodes::{FAILURES_TO_DEGRADE, Node, NodeId, NodeStatus, health_url},
};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::{
    Message, client::IntoClientRequest, http::header as ws_header,
};
use uuid::Uuid;

const ENV_SECRET: &str = "synthetic-env-secret";
const MODELS: &str = "/internal/cascade/v1/models";
const CHAT: &str = "/internal/cascade/v1/chat/completions";
const NODES: &str = "/api/admin/voice-nodes";

fn env_config() -> Config {
    Config {
        cascade_realtime_url: Some("ws://127.0.0.1:1/v1/realtime".into()),
        cascade_llm_token: Some(ENV_SECRET.into()),
        ..test_config()
    }
}

struct Rig {
    app: Router,
    state: AppState,
}

fn rig(pool: PgPool, config: Config, scripts: Vec<Script>) -> Rig {
    register_fixture_pool(&pool);
    let state = AppState::new(pool, config, Arc::new(FakeIdentity), FakeAi::with(scripts));
    Rig {
        app: app(state.clone()),
        state,
    }
}

fn node(name: &str, id: NodeId, max: Option<usize>) -> Node {
    Node {
        id,
        name: name.into(),
        realtime_url: format!("ws://{name}.invalid/v1/realtime"),
        max_sessions: max,
        status: NodeStatus::Active,
    }
}

fn db(name: &str, max: Option<usize>) -> Node {
    node(name, NodeId::Db(Uuid::new_v4()), max)
}

// ---------- 選節點 ----------

#[test]
fn the_least_loaded_node_wins_and_ties_go_to_env_then_name() {
    let sessions = Arc::new(socrates_chat_backend::cascade::VoiceSessions::default());
    let nodes = vec![db("b", None), db("a", None), node("env", NodeId::Env, None)];
    // 全部 0 場：環境節點優先
    let first = sessions
        .open_balanced(Uuid::new_v4(), Uuid::new_v4(), &nodes)
        .unwrap();
    assert_eq!(first.node(), NodeId::Env);
    // env 有 1 場：其餘 0 場的依名稱，a 先
    let second = sessions
        .open_balanced(Uuid::new_v4(), Uuid::new_v4(), &nodes)
        .unwrap();
    assert_eq!(second.node(), nodes[1].id);
    let third = sessions
        .open_balanced(Uuid::new_v4(), Uuid::new_v4(), &nodes)
        .unwrap();
    assert_eq!(third.node(), nodes[0].id);
    // 三個都 1 場，又回到環境節點
    let fourth = sessions
        .open_balanced(Uuid::new_v4(), Uuid::new_v4(), &nodes)
        .unwrap();
    assert_eq!(fourth.node(), NodeId::Env);
}

#[test]
fn a_full_node_is_skipped_and_all_full_means_no_node() {
    let sessions = Arc::new(socrates_chat_backend::cascade::VoiceSessions::default());
    let small = db("small", Some(1));
    let big = db("big", Some(3));
    let nodes = vec![small.clone(), big.clone()];
    let held: Vec<_> = (0..4)
        .map(|_| {
            sessions
                .open_balanced(Uuid::new_v4(), Uuid::new_v4(), &nodes)
                .unwrap()
        })
        .collect();
    assert_eq!(held.iter().filter(|s| s.node() == small.id).count(), 1);
    assert_eq!(held.iter().filter(|s| s.node() == big.id).count(), 3);
    assert_eq!(
        sessions
            .open_balanced(Uuid::new_v4(), Uuid::new_v4(), &nodes)
            .err(),
        Some(OpenError::NoNode)
    );
    assert_eq!(
        sessions
            .open_balanced(Uuid::new_v4(), Uuid::new_v4(), &[])
            .err(),
        Some(OpenError::NoNode)
    );
}

#[test]
fn one_student_cannot_hold_two_sessions() {
    let sessions = Arc::new(socrates_chat_backend::cascade::VoiceSessions::default());
    let nodes = vec![db("a", None)];
    let user = Uuid::new_v4();
    let _first = sessions
        .open_balanced(user, Uuid::new_v4(), &nodes)
        .unwrap();
    assert_eq!(
        sessions.open_balanced(user, Uuid::new_v4(), &nodes).err(),
        Some(OpenError::UserBusy)
    );
}

#[test]
fn concurrent_opens_never_exceed_max_sessions() {
    let sessions = Arc::new(socrates_chat_backend::cascade::VoiceSessions::default());
    let nodes = Arc::new(vec![db("a", Some(5)), db("b", Some(3))]);
    let threads: Vec<_> = (0..40)
        .map(|_| {
            let (sessions, nodes) = (sessions.clone(), nodes.clone());
            std::thread::spawn(move || {
                sessions
                    .open_balanced(Uuid::new_v4(), Uuid::new_v4(), &nodes)
                    .ok()
            })
        })
        .collect();
    let held: Vec<_> = threads
        .into_iter()
        .filter_map(|t| t.join().unwrap())
        .collect();
    assert_eq!(held.len(), 8);
    let counts = sessions.counts();
    assert_eq!(counts[&nodes[0].id], 5);
    assert_eq!(counts[&nodes[1].id], 3);
}

#[test]
fn releasing_a_session_frees_its_slot() {
    let sessions = Arc::new(socrates_chat_backend::cascade::VoiceSessions::default());
    let nodes = vec![db("a", Some(1))];
    let first = sessions
        .open_balanced(Uuid::new_v4(), Uuid::new_v4(), &nodes)
        .unwrap();
    drop(first);
    assert!(sessions.counts().is_empty());
    assert!(
        sessions
            .open_balanced(Uuid::new_v4(), Uuid::new_v4(), &nodes)
            .is_ok()
    );
}

#[test]
fn rebinding_moves_the_count_and_failing_to_rebind_leaks_nothing() {
    let sessions = Arc::new(socrates_chat_backend::cascade::VoiceSessions::default());
    let (a, b) = (db("a", None), db("b", None));
    let mut session = sessions
        .open_balanced(Uuid::new_v4(), Uuid::new_v4(), std::slice::from_ref(&a))
        .unwrap();
    assert!(sessions.rebind(&mut session, std::slice::from_ref(&b)));
    assert_eq!(session.node(), b.id);
    assert_eq!(sessions.node_of(session.token()), Some(b.id));
    assert_eq!(sessions.counts().get(&a.id), None);
    assert_eq!(sessions.counts()[&b.id], 1);
    // 沒有候選：維持原綁定，釋放時才扣回
    assert!(!sessions.rebind(&mut session, &[]));
    assert_eq!(session.node(), b.id);
    drop(session);
    assert!(sessions.counts().is_empty());
}

#[test]
fn the_health_url_is_derived_from_the_realtime_url() {
    let url = |raw| health_url(raw).map(|u| u.to_string());
    assert_eq!(
        url("ws://10.0.0.5:8110/v1/realtime").as_deref(),
        Some("http://10.0.0.5:8110/health")
    );
    assert_eq!(
        url("wss://node.example/v1/realtime?x=1").as_deref(),
        Some("https://node.example/health")
    );
    assert_eq!(url("http://node.example/v1/realtime"), None);
}

// ---------- 管理 API ----------

async fn add_node(r: &Rig, admin: &str, name: &str, url: &str, max: i64) -> Value {
    let res = send(
        &r.app,
        json_req(
            "POST",
            NODES,
            Some(admin),
            Some(json!({"name": name, "realtime_url": url, "max_sessions": max})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await
}

async fn patch(r: &Rig, admin: &str, id: &str, body: Value) -> StatusCode {
    send(
        &r.app,
        json_req("PATCH", &format!("{NODES}/{id}"), Some(admin), Some(body)),
    )
    .await
    .status()
}

#[sqlx::test]
async fn only_admins_can_use_the_node_api(pool: PgPool) {
    let r = rig(pool, test_config(), vec![]);
    let a = seed_actors(&r.app).await;
    let fake = Uuid::new_v4();
    let body = json!({"name": "n", "realtime_url": "ws://x/v1/realtime", "max_sessions": 1});
    let calls = [
        ("GET", NODES.to_string(), None),
        ("POST", NODES.to_string(), Some(body.clone())),
        ("PATCH", format!("{NODES}/{fake}"), Some(json!({}))),
        ("POST", format!("{NODES}/{fake}/rotate-token"), None),
        ("DELETE", format!("{NODES}/{fake}"), None),
    ];
    for (method, path, body) in calls {
        let status =
            |cookie: Option<&str>| send(&r.app, json_req(method, &path, cookie, body.clone()));
        assert_eq!(
            status(None).await.status(),
            StatusCode::UNAUTHORIZED,
            "{path}"
        );
        for cookie in [&a.student, &a.teacher] {
            assert_eq!(
                status(Some(cookie)).await.status(),
                StatusCode::FORBIDDEN,
                "{method} {path}"
            );
        }
    }
    let ok = send(&r.app, get(NODES, Some(&a.admin))).await;
    assert_eq!(ok.status(), StatusCode::OK);
}

#[sqlx::test]
async fn the_token_appears_once_and_never_again(pool: PgPool) {
    let r = rig(pool.clone(), env_config(), vec![]);
    let a = seed_actors(&r.app).await;
    let created = add_node(&r, &a.admin, "alpha", "ws://10.0.0.5:8110/v1/realtime", 2).await;
    let token = created["token"].as_str().unwrap();
    assert_eq!(token.len(), 64);
    assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
    let id = created["node"]["id"].as_str().unwrap();

    // 資料庫只存雜湊
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT token_hash FROM voice_nodes WHERE id = $1::uuid")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored.len(), 32);
    assert_ne!(stored, token.as_bytes());

    let list = json_body(send(&r.app, get(NODES, Some(&a.admin))).await).await;
    assert!(!list.to_string().contains(token));
    let items = list.as_array().unwrap();
    assert_eq!(items[0]["id"], "env");
    assert_eq!(items[0]["read_only"], true);
    assert_eq!(items[0]["max_sessions"], Value::Null);
    assert_eq!(items[1]["name"], "alpha");
    assert_eq!(items[1]["max_sessions"], 2);
    assert_eq!(items[1]["status"], "active");
    assert_eq!(items[1]["healthy"], true);
    assert_eq!(items[1]["sessions"], 0);

    let patched = send(
        &r.app,
        json_req(
            "PATCH",
            &format!("{NODES}/{id}"),
            Some(&a.admin),
            Some(json!({"max_sessions": 3})),
        ),
    )
    .await;
    assert!(!json_body(patched).await.to_string().contains(token));
}

#[sqlx::test]
async fn bad_node_input_is_rejected(pool: PgPool) {
    let r = rig(pool, test_config(), vec![]);
    let a = seed_actors(&r.app).await;
    add_node(&r, &a.admin, "alpha", "ws://10.0.0.5/v1/realtime", 1).await;
    let create = |body: Value| send(&r.app, json_req("POST", NODES, Some(&a.admin), Some(body)));
    let good = json!({"name": "beta", "realtime_url": "ws://h/v1/realtime", "max_sessions": 1});
    let with = |key: &str, value: Value| {
        let mut body = good.clone();
        body[key] = value;
        body
    };
    for (body, code) in [
        (
            with("realtime_url", json!("http://h/v1/realtime")),
            "invalid_realtime_url",
        ),
        (
            with("realtime_url", json!("not a url")),
            "invalid_realtime_url",
        ),
        (with("max_sessions", json!(0)), "invalid_max_sessions"),
        (with("name", json!("  ")), "invalid_name"),
        (with("name", json!("env")), "invalid_name"),
        (
            json!({"name": "beta", "realtime_url": "ws://h/v1/realtime"}),
            "invalid_request",
        ),
    ] {
        assert_json_error(create(body).await, StatusCode::BAD_REQUEST, code).await;
    }
    assert_json_error(
        create(with("name", json!("alpha"))).await,
        StatusCode::CONFLICT,
        "voice_node_name_taken",
    )
    .await;
    let missing = Uuid::new_v4();
    assert_eq!(
        patch(
            &r,
            &a.admin,
            &missing.to_string(),
            json!({"status": "draining"})
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        patch(
            &r,
            &a.admin,
            &missing.to_string(),
            json!({"status": "paused"})
        )
        .await,
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test]
async fn every_node_change_is_audited_without_url_or_token(pool: PgPool) {
    let r = rig(pool.clone(), test_config(), vec![]);
    let a = seed_actors(&r.app).await;
    let created = add_node(
        &r,
        &a.admin,
        "alpha",
        "ws://secret-host.example/v1/realtime",
        1,
    )
    .await;
    let id = created["node"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        patch(&r, &a.admin, &id, json!({"status": "draining"})).await,
        StatusCode::OK
    );
    let rotated = send(
        &r.app,
        json_req(
            "POST",
            &format!("{NODES}/{id}/rotate-token"),
            Some(&a.admin),
            None,
        ),
    )
    .await;
    assert_eq!(rotated.status(), StatusCode::OK);
    let rotated = json_body(rotated).await;
    let deleted = send(
        &r.app,
        json_req("DELETE", &format!("{NODES}/{id}"), Some(&a.admin), None),
    )
    .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT action, target_email FROM audit_logs WHERE action LIKE 'voice_node_%' ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let actions: Vec<_> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        actions,
        [
            "voice_node_create",
            "voice_node_update",
            "voice_node_rotate_token",
            "voice_node_delete"
        ]
    );
    for (_, target) in &rows {
        assert!(target.contains(&id) && target.contains("alpha"));
        assert!(!target.contains("secret-host"));
        assert!(!target.contains(created["token"].as_str().unwrap()));
        assert!(!target.contains(rotated["token"].as_str().unwrap()));
    }
}

// ---------- 候選節點與環境節點 ----------

#[sqlx::test]
async fn only_the_env_vars_behave_as_before(pool: PgPool) {
    let r = rig(pool.clone(), env_config(), vec![]);
    let candidates = r.state.voice_nodes.candidates(&r.state, &[]).await;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, NodeId::Env);
    assert_eq!(candidates[0].max_sessions, None);
    assert!(r.state.voice_nodes.any_available(&r.state).await);

    let capped = rig(
        pool,
        Config {
            cascade_max_sessions: Some(2),
            ..env_config()
        },
        vec![],
    );
    let candidates = capped
        .state
        .voice_nodes
        .candidates(&capped.state, &[])
        .await;
    assert_eq!(candidates[0].max_sessions, Some(2));
}

#[sqlx::test]
async fn nothing_configured_means_off(pool: PgPool) {
    let r = rig(pool, test_config(), vec![]);
    assert!(!r.state.voice_nodes.any_available(&r.state).await);
    assert_eq!(
        send(&r.app, llm_req(Some("anything"), "{}")).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn registered_nodes_join_the_pool_without_dropping_env(pool: PgPool) {
    let r = rig(pool, env_config(), vec![]);
    let a = seed_actors(&r.app).await;
    add_node(&r, &a.admin, "alpha", "ws://10.0.0.5/v1/realtime", 1).await;
    let ids: Vec<_> = r
        .state
        .voice_nodes
        .candidates(&r.state, &[])
        .await
        .into_iter()
        .map(|n| n.id)
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&NodeId::Env));
}

#[sqlx::test]
async fn draining_disabled_and_unhealthy_nodes_take_no_new_sessions(pool: PgPool) {
    let r = rig(pool, test_config(), vec![]);
    let a = seed_actors(&r.app).await;
    let mut ids = Vec::new();
    for name in ["a", "b", "c", "d"] {
        let created = add_node(&r, &a.admin, name, "ws://10.0.0.5/v1/realtime", 1).await;
        ids.push(created["node"]["id"].as_str().unwrap().to_string());
    }
    assert_eq!(
        patch(&r, &a.admin, &ids[0], json!({"status": "draining"})).await,
        StatusCode::OK
    );
    assert_eq!(
        patch(&r, &a.admin, &ids[1], json!({"status": "disabled"})).await,
        StatusCode::OK
    );
    r.state
        .voice_nodes
        .mark_unhealthy(NodeId::Db(ids[2].parse().unwrap()));
    let left = r.state.voice_nodes.candidates(&r.state, &[]).await;
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].name, "d");

    // 全不可用：沒有候選，voice_realtime 為假
    assert_eq!(
        patch(&r, &a.admin, &ids[3], json!({"status": "draining"})).await,
        StatusCode::OK
    );
    assert!(!r.state.voice_nodes.any_available(&r.state).await);
    let conv = start_conversation(&r.app, &a).await;
    let res = send(
        &r.app,
        get(
            &format!("/api/conversations/{conv}/voice"),
            Some(&a.student),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    let detail = json_body(
        send(
            &r.app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student)),
        )
        .await,
    )
    .await;
    assert_eq!(detail["voice_realtime"], false);
}

#[sqlx::test]
async fn voice_realtime_ignores_whether_the_node_is_full(pool: PgPool) {
    let r = rig(pool, test_config(), vec![]);
    let a = seed_actors(&r.app).await;
    add_node(&r, &a.admin, "a", "ws://10.0.0.5/v1/realtime", 1).await;
    let nodes = r.state.voice_nodes.candidates(&r.state, &[]).await;
    let _held = r
        .state
        .voice_sessions
        .open_balanced(Uuid::new_v4(), Uuid::new_v4(), &nodes)
        .unwrap();
    let conv = start_conversation(&r.app, &a).await;
    let detail = json_body(
        send(
            &r.app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student)),
        )
        .await,
    )
    .await;
    assert_eq!(detail["voice_realtime"], true);
}

// ---------- 健康探測 ----------

/// 假的健康端點：依 `status` 回應；`None` 表示收下連線卻不回應。
async fn health_server(status: Arc<Mutex<Option<u16>>>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut tcp, _)) = listener.accept().await else {
                return;
            };
            let current = *status.lock().unwrap();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 1024];
                let _ = tcp.read(&mut buf).await;
                match current {
                    Some(code) => {
                        let reply = format!(
                            "HTTP/1.1 {code} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        );
                        let _ = tcp.write_all(reply.as_bytes()).await;
                    }
                    None => tokio::time::sleep(Duration::from_secs(30)).await,
                }
            });
        }
    });
    format!("ws://{addr}/v1/realtime")
}

#[sqlx::test]
async fn three_failures_degrade_a_node_and_one_success_restores_it(pool: PgPool) {
    let r = rig(pool, test_config(), vec![]);
    let a = seed_actors(&r.app).await;
    let status = Arc::new(Mutex::new(Some(200)));
    let url = health_server(status.clone()).await;
    let created = add_node(&r, &a.admin, "a", &url, 1).await;
    let id = NodeId::Db(created["node"]["id"].as_str().unwrap().parse().unwrap());

    r.state.voice_nodes.probe_all(&r.state).await;
    assert!(r.state.voice_nodes.is_healthy(id));

    *status.lock().unwrap() = Some(500);
    for _ in 0..FAILURES_TO_DEGRADE - 1 {
        r.state.voice_nodes.probe_all(&r.state).await;
        assert!(r.state.voice_nodes.is_healthy(id), "not yet degraded");
    }
    r.state.voice_nodes.probe_all(&r.state).await;
    assert!(!r.state.voice_nodes.is_healthy(id));
    assert!(
        r.state
            .voice_nodes
            .candidates(&r.state, &[])
            .await
            .is_empty()
    );

    *status.lock().unwrap() = Some(204);
    r.state.voice_nodes.probe_all(&r.state).await;
    assert!(r.state.voice_nodes.is_healthy(id));
}

#[sqlx::test]
async fn a_health_probe_that_hangs_counts_as_a_failure(pool: PgPool) {
    let r = rig(pool, test_config(), vec![]);
    let a = seed_actors(&r.app).await;
    let url = health_server(Arc::new(Mutex::new(None))).await;
    let created = add_node(&r, &a.admin, "a", &url, 1).await;
    let id = NodeId::Db(created["node"]["id"].as_str().unwrap().parse().unwrap());
    let started = Instant::now();
    r.state.voice_nodes.probe_all(&r.state).await;
    assert!(started.elapsed() < Duration::from_secs(5));
    // 一次逾時只算一次失敗
    assert!(r.state.voice_nodes.is_healthy(id));
    r.state.voice_nodes.mark_unhealthy(id);
    assert!(!r.state.voice_nodes.is_healthy(id));
}

// ---------- 內部端點：節點密鑰 ----------

fn internal(path: &str, token: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut b = Request::builder()
        .method(if body.is_some() { "POST" } else { "GET" })
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(body.map_or_else(Body::empty, |v| Body::from(v.to_string())))
        .unwrap()
}

fn llm_req(token: Option<&str>, raw: &str) -> Request<Body> {
    let body = serde_json::from_str(raw).unwrap_or(json!({}));
    internal(CHAT, token, Some(body))
}

fn turn(system: &str) -> Value {
    json!({"model": "rust", "messages": [
        {"role": "system", "content": system},
        {"role": "user", "content": "合成的第一句"}
    ]})
}

#[sqlx::test]
async fn a_node_token_works_until_it_is_rotated_disabled_or_deleted(pool: PgPool) {
    let r = rig(pool, env_config(), vec![]);
    let a = seed_actors(&r.app).await;
    let created = add_node(&r, &a.admin, "a", "ws://10.0.0.5/v1/realtime", 1).await;
    let id = created["node"]["id"].as_str().unwrap().to_string();
    let token = created["token"].as_str().unwrap().to_string();
    let status = |t: String| {
        let app = r.app.clone();
        async move { send(&app, internal(MODELS, Some(&t), None)).await.status() }
    };

    assert_eq!(status(token.clone()).await, StatusCode::OK);
    assert_eq!(status("wrong".into()).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        send(&r.app, internal(MODELS, None, None)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(status(ENV_SECRET.into()).await, StatusCode::OK);

    // draining：密鑰仍有效
    patch(&r, &a.admin, &id, json!({"status": "draining"})).await;
    assert_eq!(status(token.clone()).await, StatusCode::OK);

    // 輪換：舊的立刻失效，新的可用，不等快取
    let rotated = json_body(
        send(
            &r.app,
            json_req(
                "POST",
                &format!("{NODES}/{id}/rotate-token"),
                Some(&a.admin),
                None,
            ),
        )
        .await,
    )
    .await;
    let fresh = rotated["token"].as_str().unwrap().to_string();
    assert_eq!(status(token).await, StatusCode::UNAUTHORIZED);
    assert_eq!(status(fresh.clone()).await, StatusCode::OK);

    // disabled：立刻失效；改回 active 即恢復
    patch(&r, &a.admin, &id, json!({"status": "disabled"})).await;
    assert_eq!(status(fresh.clone()).await, StatusCode::UNAUTHORIZED);
    patch(&r, &a.admin, &id, json!({"status": "active"})).await;
    assert_eq!(status(fresh.clone()).await, StatusCode::OK);

    // 刪除
    send(
        &r.app,
        json_req("DELETE", &format!("{NODES}/{id}"), Some(&a.admin), None),
    )
    .await;
    assert_eq!(status(fresh).await, StatusCode::UNAUTHORIZED);
    assert_eq!(status(ENV_SECRET.into()).await, StatusCode::OK);
}

#[sqlx::test]
async fn a_token_only_drives_sessions_bound_to_its_own_node(pool: PgPool) {
    let r = rig(
        pool.clone(),
        env_config(),
        vec![Reply(vec!["你為什麼", "這樣認為？"])],
    );
    let a = seed_actors(&r.app).await;
    let alpha = add_node(&r, &a.admin, "alpha", "ws://10.0.0.5/v1/realtime", 1).await;
    let beta = add_node(&r, &a.admin, "beta", "ws://10.0.0.6/v1/realtime", 1).await;
    let conv = start_conversation(&r.app, &a).await;
    let user: Uuid = sqlx::query_scalar("SELECT user_id FROM conversations WHERE id = $1::uuid")
        .bind(&conv)
        .fetch_one(&pool)
        .await
        .unwrap();
    let alpha_id = NodeId::Db(alpha["node"]["id"].as_str().unwrap().parse().unwrap());
    let candidates: Vec<Node> = r
        .state
        .voice_nodes
        .candidates(&r.state, &[])
        .await
        .into_iter()
        .filter(|n| n.id == alpha_id)
        .collect();
    let session = r
        .state
        .voice_sessions
        .open_balanced(user, conv.parse().unwrap(), &candidates)
        .unwrap();
    let system = voice_session_prompt(session.token());

    for wrong in [beta["token"].as_str().unwrap(), ENV_SECRET] {
        let res = send(&r.app, internal(CHAT, Some(wrong), Some(turn(&system)))).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
    let own = alpha["token"].as_str().unwrap();
    let res = send(&r.app, internal(CHAT, Some(own), Some(turn(&system)))).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[sqlx::test]
async fn the_internal_port_can_be_split_from_the_public_one(pool: PgPool) {
    let config = Config {
        internal_listen_addr: Some("127.0.0.1:0".parse().unwrap()),
        ..env_config()
    };
    let r = rig(pool, config, vec![]);
    // 公開 router 沒有內部端點
    let public = send(&r.app, internal(MODELS, Some(ENV_SECRET), None)).await;
    assert_eq!(public.status(), StatusCode::NOT_FOUND);
    // 獨立的 router 有
    let private = internal_app(r.state.clone());
    let ok = send(&private, internal(MODELS, Some(ENV_SECRET), None)).await;
    assert_eq!(ok.status(), StatusCode::OK);
    let denied = send(&private, internal(MODELS, Some("wrong"), None)).await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    // 獨立的 router 不提供公開 API
    let api = send(&private, get("/api/openapi.json", None)).await;
    assert_eq!(api.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn without_the_split_the_internal_routes_stay_on_the_public_router(pool: PgPool) {
    let r = rig(pool, env_config(), vec![]);
    let res = send(&r.app, internal(MODELS, Some(ENV_SECRET), None)).await;
    assert_eq!(res.status(), StatusCode::OK);
}

// ---------- 連線：重試、逾時、撤銷 ----------

/// 假的 cascade：記下收到的文字訊息。
async fn fake_cascade() -> (String, Arc<Mutex<Vec<String>>>) {
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = received.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let seen = seen.clone();
            tokio::spawn(async move {
                let Ok(mut socket) = tokio_tungstenite::accept_async(tcp).await else {
                    return;
                };
                while let Some(Ok(frame)) = socket.next().await {
                    if let Message::Text(text) = frame {
                        seen.lock().unwrap().push(text.as_str().to_string());
                    }
                }
            });
        }
    });
    (format!("ws://{addr}/v1/realtime"), received)
}

/// 收下 TCP 連線卻不做 WebSocket 握手：連線嘗試只會等到逾時。
async fn silent_node() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((tcp, _)) = listener.accept().await {
            held.push(tcp);
        }
    });
    format!("ws://{addr}/v1/realtime")
}

/// 一個關閉的埠：連線立刻被拒絕。
async fn dead_node() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("ws://{addr}/v1/realtime")
}

struct Live {
    r: Rig,
    a: Actors,
    conv: String,
    addr: SocketAddr,
}

async fn live(pool: PgPool, config: Config) -> Live {
    let r = rig(pool, config, vec![]);
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = r.app.clone();
    tokio::spawn(async move {
        axum::serve(
            listener,
            served.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    Live { r, a, conv, addr }
}

type Client =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

impl Live {
    async fn connect(&self) -> Client {
        let mut request = format!("ws://{}/api/conversations/{}/voice", self.addr, self.conv)
            .into_client_request()
            .unwrap();
        let headers = request.headers_mut();
        headers.insert(ws_header::COOKIE, self.a.student.parse().unwrap());
        headers.insert(
            ws_header::ORIGIN,
            self.r.state.config.app_base_url.parse().unwrap(),
        );
        tokio_tungstenite::connect_async(request).await.unwrap().0
    }

    async fn node(&self, name: &str, url: &str) -> NodeId {
        let created = add_node(&self.r, &self.a.admin, name, url, 1).await;
        NodeId::Db(created["node"]["id"].as_str().unwrap().parse().unwrap())
    }
}

/// 等條件成立，最多 `secs` 秒。
async fn eventually(secs: u64, mut ok: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    ok()
}

async fn closed(client: &mut Client, secs: u64) -> bool {
    tokio::time::timeout(Duration::from_secs(secs), async {
        loop {
            match client.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await
    .is_ok()
}

#[sqlx::test]
async fn a_dead_first_node_is_marked_unhealthy_and_the_second_is_used(pool: PgPool) {
    let l = live(pool, test_config()).await;
    let (good_url, received) = fake_cascade().await;
    let first = l.node("a-dead", &dead_node().await).await;
    let second = l.node("b-good", &good_url).await;

    let mut client = l.connect().await;
    assert!(eventually(5, || !received.lock().unwrap().is_empty()).await);
    assert!(received.lock().unwrap()[0].contains("session.update"));
    assert!(!l.r.state.voice_nodes.is_healthy(first));
    assert!(l.r.state.voice_nodes.is_healthy(second));
    let counts = l.r.state.voice_sessions.counts();
    assert_eq!(counts.get(&first), None, "the failed node keeps no session");
    assert_eq!(counts.get(&second), Some(&1));

    client.close(None).await.unwrap();
    assert!(eventually(5, || l.r.state.voice_sessions.counts().is_empty()).await);
}

#[sqlx::test]
async fn when_both_nodes_fail_the_session_ends_and_nothing_leaks(pool: PgPool) {
    let l = live(pool, test_config()).await;
    let first = l.node("a", &dead_node().await).await;
    let second = l.node("b", &dead_node().await).await;
    let mut client = l.connect().await;
    assert!(closed(&mut client, 8).await);
    assert!(!l.r.state.voice_nodes.is_healthy(first));
    assert!(!l.r.state.voice_nodes.is_healthy(second));
    assert!(eventually(3, || l.r.state.voice_sessions.counts().is_empty()).await);
}

#[sqlx::test]
async fn unreachable_nodes_fail_within_the_browsers_ten_second_window(pool: PgPool) {
    let l = live(pool, test_config()).await;
    l.node("a", &silent_node().await).await;
    l.node("b", &silent_node().await).await;
    let started = Instant::now();
    let mut client = l.connect().await;
    assert!(closed(&mut client, 9).await);
    // 兩次各 3 秒
    let took = started.elapsed();
    assert!(took >= Duration::from_secs(5), "{took:?}");
    assert!(took < Duration::from_secs(8), "{took:?}");
}

#[sqlx::test]
async fn disabling_a_node_ends_its_running_sessions(pool: PgPool) {
    let l = live(pool, test_config()).await;
    let (url, received) = fake_cascade().await;
    let id = l.node("a", &url).await;
    let NodeId::Db(uuid) = id else { unreachable!() };
    let mut client = l.connect().await;
    assert!(eventually(5, || !received.lock().unwrap().is_empty()).await);

    assert_eq!(
        patch(
            &l.r,
            &l.a.admin,
            &uuid.to_string(),
            json!({"status": "disabled"})
        )
        .await,
        StatusCode::OK
    );
    assert!(closed(&mut client, 4).await);
    assert!(eventually(3, || l.r.state.voice_sessions.counts().is_empty()).await);
}

#[sqlx::test]
async fn draining_lets_a_running_session_finish(pool: PgPool) {
    let l = live(pool, test_config()).await;
    let (url, received) = fake_cascade().await;
    let id = l.node("a", &url).await;
    let NodeId::Db(uuid) = id else { unreachable!() };
    let mut client = l.connect().await;
    assert!(eventually(5, || !received.lock().unwrap().is_empty()).await);
    patch(
        &l.r,
        &l.a.admin,
        &uuid.to_string(),
        json!({"status": "draining"}),
    )
    .await;
    assert!(!closed(&mut client, 3).await, "still running");
    assert_eq!(l.r.state.voice_sessions.counts().get(&id), Some(&1));
}

#[sqlx::test]
async fn deleting_a_node_ends_its_running_sessions(pool: PgPool) {
    let l = live(pool, test_config()).await;
    let (url, received) = fake_cascade().await;
    let id = l.node("a", &url).await;
    let NodeId::Db(uuid) = id else { unreachable!() };
    let mut client = l.connect().await;
    assert!(eventually(5, || !received.lock().unwrap().is_empty()).await);
    let res = send(
        &l.r.app,
        json_req("DELETE", &format!("{NODES}/{uuid}"), Some(&l.a.admin), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(closed(&mut client, 4).await);
}
