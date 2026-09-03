//! Web Dashboard API handlers for SIP calls.
//!
//! Provides handlers to trigger calls, transfer, hold, resume, play audio, send DTMF, or hang up calls.

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
            if client.settings.session_timers {
                crate::service::managed_client::spawn_session_refresher(client_arc.clone(), 1800);
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

    match crate::rtp::play_wav_file(&payload.file, socket_opt, target, codec, rtp_port).await {
        Ok(_) => Ok(Json(serde_json::json!({
            "success": true,
            "msg": format!("Started playing '{}'", payload.file)
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "success": false,
            "msg": format!("Play WAV error: {}", e)
        }))),
    }
}

#[derive(serde::Deserialize)]
pub struct MessageRequest {
    pub target: String,
    pub body: String,
}

/// Send SIP MESSAGE (RFC 3428) text message via REST API
pub async fn send_message_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<MessageRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        mc.client.clone()
    };
    let client = client_arc.lock().await;
    match client.send_message(&payload.target, &payload.body).await {
        Ok(resp) => Ok(Json(serde_json::json!({
            "success": true,
            "msg": format!("SIP MESSAGE sent to {}: {}", payload.target, resp)
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "success": false,
            "msg": format!("SIP MESSAGE error: {}", e)
        }))),
    }
}

#[derive(serde::Deserialize)]
pub struct InfoDtmfRequest {
    pub digit: char,
    pub duration_ms: Option<u32>,
}

/// Send out-of-band SIP INFO DTMF (RFC 6086) via REST API
pub async fn send_info_dtmf_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<InfoDtmfRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        mc.client.clone()
    };
    let client = client_arc.lock().await;
    let dur = payload.duration_ms.unwrap_or(250);
    match client.send_info_dtmf(payload.digit, dur).await {
        Ok(resp) => Ok(Json(serde_json::json!({
            "success": true,
            "msg": format!("SIP INFO DTMF '{}' sent: {}", payload.digit, resp)
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "success": false,
            "msg": format!("SIP INFO DTMF error: {}", e)
        }))),
    }
}
