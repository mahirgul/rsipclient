use super::super::web_server::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::IntoResponse,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use std::collections::HashMap;

pub async fn audio_ws_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ws: WebSocketUpgrade,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, StatusCode> {
    let token = params.get("token").cloned();
    if token.as_deref() != Some(&state.session_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    {
        let cls = state.clients.lock().await;
        if !cls.contains_key(&name) {
            return Err(StatusCode::NOT_FOUND);
        }
    }

    Ok(ws.on_upgrade(move |socket| handle_audio_ws(socket, state, name)))
}

pub async fn handle_audio_ws(socket: WebSocket, state: AppState, account_name: String) {
    let clients = state.clients.lock().await;
    let mc = match clients.get(&account_name) {
        Some(mc) => mc,
        None => return,
    };

    let mut audio_rx = mc.audio_tx.subscribe();
    let client = mc.client.clone();
    let codec = mc.codec;
    drop(clients);

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Loop 1: Send incoming caller audio to browser (binary frames of Vec<i16>)
    let mut send_task = tokio::spawn(async move {
        while let Ok(samples) = audio_rx.recv().await {
            let mut bytes = Vec::with_capacity(samples.len() * 2);
            for sample in samples {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            if ws_sender.send(Message::Binary(bytes)).await.is_err() {
                break;
            }
        }
    });

    // Loop 2: Receive browser microphone audio (binary frames of i16) and send it as RTP to caller
    let client_clone = client.clone();
    let mut recv_task = tokio::spawn(async move {
        let mut seq = 0;
        let mut timestamp = 0;

        while let Some(Ok(msg)) = ws_receiver.next().await {
            if let Message::Binary(bytes) = msg {
                let mut samples = Vec::with_capacity(bytes.len() / 2);
                for chunk in bytes.chunks_exact(2) {
                    let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                    samples.push(sample);
                }

                let cg = client_clone.lock().await;
                if cg.in_call {
                    if let (Some(ref rtp_rec), Some(target)) =
                        (&cg.rtp_receiver, cg.remote_rtp_addr)
                    {
                        let _ = rtp_rec
                            .send_audio_samples(&samples, target, codec, &mut seq, &mut timestamp)
                            .await;
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => { recv_task.abort(); }
        _ = &mut recv_task => { send_task.abort(); }
    }
}
