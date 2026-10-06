//! Web Server module - provides the REST API and serves the embedded Dashboard UI

use super::web_handlers::*;
use super::ManagedClient;
use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{delete, get, post, put},
    Router,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub clients: Arc<Mutex<HashMap<String, ManagedClient>>>,
    pub global_shutdown: Arc<Mutex<bool>>,
    pub config_path: String,
    pub web_username: String,
    pub web_password: String,
    pub session_token: String,
    pub plugin_manager: crate::plugins::PluginManager,
    pub login_limiter: LoginLimiter,
}

/// Failed logins allowed from one address before it is locked out.
const LOGIN_MAX_FAILURES: u32 = 5;
/// How long an address stays locked out after too many failures.
const LOGIN_LOCKOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// Bound on tracked addresses, so a spray of sources cannot grow the map forever.
const LOGIN_MAX_TRACKED: usize = 10_000;

#[derive(Default)]
struct LoginAttempts {
    failures: u32,
    locked_until: Option<std::time::Instant>,
}

/// Throttles password guessing on `/api/login`, per client IP.
///
/// The dashboard ships with default credentials and is often exposed on a
/// LAN; without a limit a script can try passwords as fast as the server
/// answers. Tracking per address keeps one attacker from locking out everyone.
#[derive(Clone, Default)]
pub struct LoginLimiter {
    attempts: Arc<std::sync::Mutex<HashMap<std::net::IpAddr, LoginAttempts>>>,
}

impl LoginLimiter {
    /// Whether `ip` may attempt a login now.
    pub fn allowed(&self, ip: std::net::IpAddr) -> bool {
        let mut map = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        match map.get_mut(&ip).and_then(|a| a.locked_until) {
            Some(until) if std::time::Instant::now() < until => false,
            Some(_) => {
                // Lockout served; start counting afresh.
                map.remove(&ip);
                true
            }
            None => true,
        }
    }

    pub fn record_failure(&self, ip: std::net::IpAddr) {
        let mut map = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() >= LOGIN_MAX_TRACKED && !map.contains_key(&ip) {
            let now = std::time::Instant::now();
            map.retain(|_, a| a.locked_until.is_some_and(|t| t > now));
        }
        let entry = map.entry(ip).or_default();
        entry.failures += 1;
        if entry.failures >= LOGIN_MAX_FAILURES {
            entry.locked_until = Some(std::time::Instant::now() + LOGIN_LOCKOUT);
            log::warn!(
                "Too many failed dashboard logins from {}; locked for {:?}",
                ip,
                LOGIN_LOCKOUT
            );
        }
    }

    pub fn record_success(&self, ip: std::net::IpAddr) {
        let mut map = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(&ip);
    }
}

#[derive(serde::Serialize)]
pub struct StatusResponse {
    pub os_name: String,
    pub total_accounts: usize,
    pub registered_accounts: usize,
    pub active_calls: usize,
    pub accounts: Vec<AccountStatus>,
    pub app_version: String,
    pub config_path: String,
}

#[derive(serde::Serialize)]
pub struct AccountStatus {
    pub name: String,
    pub username: String,
    pub domain: String,
    pub server: String,
    pub sip_port: u16,
    pub registered: bool,
    pub in_call: bool,
    pub held: bool,
    pub call_id: Option<String>,
    pub remote_uri: Option<String>,
    pub direction: Option<String>,
    pub ringing: bool,
    pub ringing_from: Option<String>,
    pub call_duration_secs: u64,
    pub codec: String,
    pub codec_rate: u32,
    pub audio_input_device: Option<String>,
    pub audio_output_device: Option<String>,
}

/// Compare a secret without an early exit on the first differing byte.
///
/// `==` on tokens and passwords returns as soon as two bytes differ, which
/// leaks how much of a guess was correct. The length is still observable, but
/// the contents are not.
pub fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Helper to verify Authorization header token
pub fn verify_token(headers: &HeaderMap, state: &AppState) -> Result<(), StatusCode> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));

    if let Some(tok) = token {
        if secret_eq(tok, &state.session_token) {
            return Ok(());
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

/// Serve the single-page HTML dashboard
async fn index() -> impl IntoResponse {
    Html(include_str!("web/index.html"))
}

async fn style_css() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css")],
        include_str!("web/style.css"),
    )
}

async fn auth_js() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        include_str!("web/auth.js"),
    )
}

async fn audio_js() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        include_str!("web/audio.js"),
    )
}

async fn config_js() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        include_str!("web/config.js"),
    )
}

async fn app_js() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        include_str!("web/app.js"),
    )
}

async fn plugins_js() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        include_str!("web/plugins.js"),
    )
}

async fn favicon() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "image/x-icon")],
        include_bytes!("web/favicon.ico").as_slice(),
    )
}

/// Launch the Axum HTTP server
pub async fn start_web_server(state: AppState, port: u16) {
    let app = build_router(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    log::info!("Starting dashboard web server on http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            log::error!("Failed to bind web server to port {}: {}", port, e);
            return;
        }
    };

    // Connect info gives the login handler the client address to throttle on.
    let service = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
    if let Err(e) = axum::serve(listener, service).await {
        log::error!("Axum web server error: {}", e);
    }
}

/// Reject unauthenticated `/api` requests before any handler runs.
///
/// Handlers also check the token, but their body extractors run first: an
/// anonymous request with a bad body got a 422 describing the expected
/// fields instead of a 401. Login is the one public endpoint, and the audio
/// WebSocket authenticates with a query token because browsers cannot set
/// headers on WebSocket upgrades.
async fn require_api_token(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, StatusCode> {
    let path = request.uri().path();
    if !path.starts_with("/api/") || path == "/api/login" {
        return Ok(next.run(request).await);
    }
    if path.ends_with("/audio-ws") {
        let query_token = request
            .uri()
            .query()
            .unwrap_or("")
            .split('&')
            .find_map(|pair| pair.strip_prefix("token="));
        if !query_token.is_some_and(|t| secret_eq(t, &state.session_token)) {
            return Err(StatusCode::UNAUTHORIZED);
        }
    } else {
        verify_token(request.headers(), &state)?;
    }
    Ok(next.run(request).await)
}

/// All dashboard routes, bound to `state`.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/style.css", get(style_css))
        .route("/auth.js", get(auth_js))
        .route("/audio.js", get(audio_js))
        .route("/config.js", get(config_js))
        .route("/app.js", get(app_js))
        .route("/plugins.js", get(plugins_js))
        .route("/favicon.ico", get(favicon))
        .route("/api/login", post(login))
        .route("/api/status", get(get_status))
        .route("/api/accounts", get(get_accounts))
        .route("/api/accounts", post(add_account))
        .route("/api/accounts/:name", put(edit_account))
        .route("/api/accounts/:name", delete(delete_account))
        .route("/api/accounts/:name/register", post(register_account))
        .route("/api/accounts/:name/unregister", post(unregister_account))
        .route("/api/accounts/:name/call", post(call_account))
        .route("/api/accounts/:name/answer", post(answer_account))
        .route("/api/accounts/:name/reject", post(reject_account))
        .route("/api/accounts/:name/hangup", post(hangup_account))
        .route("/api/accounts/:name/hold", post(hold_account))
        .route("/api/accounts/:name/resume", post(resume_account))
        .route("/api/accounts/:name/transfer", post(transfer_account))
        .route("/api/accounts/:name/dtmf", post(dtmf_account))
        .route("/api/accounts/:name/message", post(send_message_account))
        .route(
            "/api/accounts/:name/info-dtmf",
            post(send_info_dtmf_account),
        )
        .route("/api/accounts/:name/play", post(play_account))
        .route("/api/config", get(get_config))
        .route("/api/config", put(put_config))
        .route("/api/logs", get(get_logs))
        .route("/api/sip/traces", get(get_sip_traces))
        .route("/api/calls/history", get(get_call_history))
        .route("/api/audio", get(audio_files::get_audio_files))
        .route("/api/audio/:name", post(audio_files::upload_audio_file))
        .route("/api/audio/:name", get(audio_files::download_audio_file))
        .route("/api/audio/:name", delete(audio_files::delete_audio_file))
        .route("/api/accounts/:name/audio-ws", get(audio_ws_handler))
        .route("/api/plugins", get(plugins::get_plugins_status))
        .route("/api/plugins", put(plugins::update_plugins_config))
        .route(
            "/api/plugins/scripts/:filename",
            get(plugins::get_script_content),
        )
        .route("/api/plugins/scripts", post(plugins::save_script_file))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_api_token,
        ))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_eq_matches_string_equality() {
        assert!(secret_eq("s3cret", "s3cret"));
        assert!(secret_eq("", ""));
        assert!(!secret_eq("s3cret", "s3crey"));
        assert!(!secret_eq("s3cret", "s3cre"));
        assert!(!secret_eq("", "x"));
        assert!(!secret_eq("şifre", "sifre"));
        assert!(secret_eq("şifre", "şifre"));
    }

    const TOKEN: &str = "test-session-token";

    /// Start the dashboard on an ephemeral port; returns its base URL.
    async fn spawn_server() -> String {
        let script_dir = std::env::temp_dir().join(format!("rsip-web-{}", uuid::Uuid::new_v4()));
        let plugins = crate::plugins::PluginSystemConfig {
            script_dir: script_dir.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let state = AppState {
            clients: Arc::new(Mutex::new(HashMap::new())),
            global_shutdown: Arc::new(Mutex::new(false)),
            config_path: script_dir
                .join("config.toml")
                .to_string_lossy()
                .into_owned(),
            web_username: "admin".into(),
            web_password: "correct horse".into(),
            session_token: TOKEN.into(),
            plugin_manager: crate::plugins::PluginManager::new(plugins),
            login_limiter: LoginLimiter::default(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let service =
            build_router(state).into_make_service_with_connect_info::<std::net::SocketAddr>();
        tokio::spawn(async move { axum::serve(listener, service).await });
        format!("http://{addr}")
    }

    async fn login(http: &reqwest::Client, base: &str, password: &str) -> reqwest::StatusCode {
        http.post(format!("{base}/api/login"))
            .json(&serde_json::json!({"username": "admin", "password": password}))
            .send()
            .await
            .unwrap()
            .status()
    }

    /// Every API endpoint except login must refuse a request without the
    /// session token, whatever its body.
    #[tokio::test]
    async fn api_routes_require_the_session_token() {
        let base = spawn_server().await;
        let http = reqwest::Client::new();
        let routes = [
            ("GET", "/api/status"),
            ("GET", "/api/accounts"),
            ("POST", "/api/accounts"),
            ("PUT", "/api/accounts/main"),
            ("DELETE", "/api/accounts/main"),
            ("POST", "/api/accounts/main/register"),
            ("POST", "/api/accounts/main/unregister"),
            ("POST", "/api/accounts/main/call"),
            ("POST", "/api/accounts/main/answer"),
            ("POST", "/api/accounts/main/reject"),
            ("POST", "/api/accounts/main/hangup"),
            ("POST", "/api/accounts/main/hold"),
            ("POST", "/api/accounts/main/resume"),
            ("POST", "/api/accounts/main/transfer"),
            ("POST", "/api/accounts/main/dtmf"),
            ("POST", "/api/accounts/main/message"),
            ("POST", "/api/accounts/main/play"),
            ("GET", "/api/accounts/main/audio-ws"),
            ("GET", "/api/config"),
            ("PUT", "/api/config"),
            ("GET", "/api/logs"),
            ("GET", "/api/sip/traces"),
            ("GET", "/api/calls/history"),
            ("GET", "/api/audio"),
            ("POST", "/api/audio/x.wav"),
            ("GET", "/api/audio/x.wav"),
            ("DELETE", "/api/audio/x.wav"),
            ("GET", "/api/plugins"),
            ("PUT", "/api/plugins"),
            ("GET", "/api/plugins/scripts/a.rhai"),
            ("POST", "/api/plugins/scripts"),
        ];
        for (method, path) in routes {
            for auth in [None, Some("Bearer wrong-token"), Some(TOKEN)] {
                // The bare token without the Bearer scheme is not accepted either.
                let mut req = http
                    .request(method.parse().unwrap(), format!("{base}{path}"))
                    .header("Content-Type", "application/json")
                    .body("{}");
                if let Some(value) = auth {
                    req = req.header("Authorization", value);
                }
                let status = req.send().await.unwrap().status();
                assert_eq!(
                    status, 401,
                    "{method} {path} with {auth:?} returned {status}"
                );
            }
        }
    }

    #[tokio::test]
    async fn static_assets_are_public() {
        let base = spawn_server().await;
        for path in ["/", "/style.css", "/app.js", "/auth.js", "/favicon.ico"] {
            let res = reqwest::get(format!("{base}{path}")).await.unwrap();
            assert_eq!(res.status(), 200, "{path}");
        }
    }

    #[tokio::test]
    async fn login_returns_a_token_that_unlocks_the_api() {
        let base = spawn_server().await;
        let http = reqwest::Client::new();
        let res = http
            .post(format!("{base}/api/login"))
            .json(&serde_json::json!({"username": "admin", "password": "correct horse"}))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["token"], TOKEN);

        let res = http
            .get(format!("{base}/api/status"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        let status: serde_json::Value = res.json().await.unwrap();
        assert_eq!(status["total_accounts"], 0);

        assert_eq!(login(&http, &base, "wrong").await, 401);
    }

    #[tokio::test]
    async fn repeated_failed_logins_are_throttled() {
        let base = spawn_server().await;
        let http = reqwest::Client::new();
        for _ in 0..LOGIN_MAX_FAILURES {
            assert_eq!(login(&http, &base, "guess").await, 401);
        }
        // Locked: even the right password is refused until the lockout ends.
        assert_eq!(login(&http, &base, "guess").await, 429);
        assert_eq!(login(&http, &base, "correct horse").await, 429);
    }

    #[test]
    fn limiter_resets_on_success_and_tracks_addresses_separately() {
        let limiter = LoginLimiter::default();
        let a: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        let b: std::net::IpAddr = "10.0.0.2".parse().unwrap();
        for _ in 0..LOGIN_MAX_FAILURES - 1 {
            limiter.record_failure(a);
        }
        assert!(limiter.allowed(a));
        limiter.record_success(a);
        for _ in 0..LOGIN_MAX_FAILURES - 1 {
            limiter.record_failure(a);
        }
        assert!(limiter.allowed(a), "success must reset the count");
        limiter.record_failure(a);
        assert!(!limiter.allowed(a));
        assert!(limiter.allowed(b));
    }
}
