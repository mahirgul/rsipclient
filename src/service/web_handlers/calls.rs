use super::super::web_server::{verify_token, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};

#[derive(serde::Deserialize)]
pub struct CallRequest {
    pub target: String,
}

#[derive(serde::Deserialize)]
pub struct DtmfRequest {
    pub digits: String,
}

pub async fn call_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<CallRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    // Clone everything we need, then drop HashMap lock before I/O
    let (client_arc, codec, audio_tx) = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        (mc.client.clone(), mc.codec, mc.audio_tx.clone())
    };
    let mut client = client_arc.lock().await;

    match client.invite(&payload.target).await {
        Ok(true) => {
            if let Some(ref rx) = client.rtp_receiver {
                rx.start(codec, Some(audio_tx));
            }
            Ok(Json(
                serde_json::json!({ "success": true, "msg": "Call established" }),
            ))
        }
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "Call failed" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

pub async fn hangup_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        mc.client.clone()
    };
    let mut client = client_arc.lock().await;
    match client.bye().await {
        Ok(true) => Ok(Json(
            serde_json::json!({ "success": true, "msg": "Call ended" }),
        )),
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "No active call" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

pub async fn hold_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        mc.client.clone()
    };
    let mut client = client_arc.lock().await;
    match client.hold().await {
        Ok(true) => Ok(Json(
            serde_json::json!({ "success": true, "msg": "Call put on hold" }),
        )),
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "Hold failed or no active call" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

pub async fn resume_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        mc.client.clone()
    };
    let mut client = client_arc.lock().await;
    match client.resume().await {
        Ok(true) => Ok(Json(
            serde_json::json!({ "success": true, "msg": "Call resumed" }),
        )),
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "Resume failed or no active call" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

pub async fn transfer_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<CallRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        mc.client.clone()
    };
    let mut client = client_arc.lock().await;
    match client.transfer(&payload.target).await {
        Ok(true) => Ok(Json(
            serde_json::json!({ "success": true, "msg": "Transfer initiated" }),
        )),
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "Transfer failed or no active call" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

pub async fn dtmf_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<DtmfRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        mc.client.clone()
    };
    let mut client = client_arc.lock().await;
    match client.send_dtmf(&payload.digits).await {
        Ok(true) => Ok(Json(
            serde_json::json!({ "success": true, "msg": format!("Sent DTMF: {}", payload.digits) }),
        )),
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "DTMF failed or no active call" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

#[derive(serde::Deserialize)]
pub struct PlayRequest {
    pub file: String,
}

pub async fn play_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<PlayRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let (client_arc, codec, rtp_port) = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        (mc.client.clone(), mc.codec, mc.account.rtp_port_start)
    };

    let client = client_arc.lock().await;
    if !client.in_call {
        return Err(StatusCode::BAD_REQUEST);
    }

    let target = match client.remote_rtp_addr {
        Some(addr) => addr,
        None => return Err(StatusCode::BAD_REQUEST),
    };

    let socket_opt = client.rtp_receiver.as_ref().map(|r| r.socket());
    drop(client);

    match tokio::fs::read(&payload.file).await {
        Ok(data) => match crate::rtp::wav::parse_wav(&data) {
            Ok((info, samples)) => {
                let sample_rate = info.sample_rate;
                tokio::spawn(async move {
                    let res = if let Some(socket) = socket_opt {
                        crate::rtp::send_wav_rtp_on_socket(
                            &socket,
                            &samples,
                            sample_rate,
                            target,
                            codec,
                        )
                        .await
                    } else {
                        crate::rtp::send_wav_rtp(&samples, sample_rate, target, 0, rtp_port, codec)
                            .await
                    };
                    match res {
                        Ok(n) => log::info!("Sent {} RTP packets (codec={:?})", n, codec),
                        Err(e) => log::error!("RTP send error: {}", e),
                    }
                });
                Ok(Json(serde_json::json!({
                    "success": true,
                    "msg": format!("Started playing '{}'", payload.file)
                })))
            }
            Err(e) => Ok(Json(serde_json::json!({
                "success": false,
                "msg": format!("WAV parse error: {}", e)
            }))),
        },
        Err(e) => Ok(Json(serde_json::json!({
            "success": false,
            "msg": format!("Cannot read file '{}': {}", payload.file, e)
        }))),
    }
}
