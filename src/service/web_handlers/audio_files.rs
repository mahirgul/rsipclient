//! Web Dashboard API handlers for managing WAV audio files.

use super::super::web_server::{verify_token, AppState};
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use std::fs;
use std::path::PathBuf;

#[derive(serde::Serialize)]
pub struct WavFile {
    pub name: String,
    pub size: u64,
    pub duration_secs: f64,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Build a `Content-Disposition` value that is safe to put in a header.
///
/// Filenames may legitimately contain non-ASCII characters (e.g. `kayıt.wav`),
/// but `HeaderValue` only accepts visible ASCII — formatting the raw name into
/// the header used to panic. Emit an ASCII-sanitized `filename=` plus an
/// RFC 5987 `filename*=UTF-8''…` parameter carrying the real name.
fn content_disposition(name: &str) -> String {
    let ascii_fallback: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();

    let mut encoded = String::with_capacity(name.len());
    for &b in name.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'~') {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{:02X}", b));
        }
    }

    format!(
        "inline; filename=\"{}\"; filename*=UTF-8''{}",
        ascii_fallback, encoded
    )
}

fn validate_filename(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
        && !name.chars().any(|c| c.is_control())
        && (name.to_lowercase().ends_with(".wav") || name.to_lowercase().ends_with(".mp3"))
}

/// List all WAV files in the current working directory
pub async fn get_audio_files(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let mut files = vec![];
    let paths = fs::read_dir(".").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    for entry in paths.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                if filename.to_lowercase().ends_with(".wav") {
                    if let Ok(metadata) = entry.metadata() {
                        let size = metadata.len();

                        // Try to parse WAV header to get duration & sample rate
                        let mut duration_secs = 0.0;
                        let mut sample_rate = 8000;
                        let mut channels = 1;

                        if let Ok(data) = fs::read(&path) {
                            if let Ok((info, _)) = crate::rtp::wav::parse_wav(&data) {
                                sample_rate = info.sample_rate;
                                channels = info.channels;
                                let bytes_per_sample = info.bits_per_sample as usize / 8;
                                if sample_rate > 0 && channels > 0 && bytes_per_sample > 0 {
                                    duration_secs = (info.data_len as f64
                                        / (channels as f64 * bytes_per_sample as f64))
                                        / sample_rate as f64;
                                }
                            }
                        }

                        files.push(WavFile {
                            name: filename.to_string(),
                            size,
                            duration_secs,
                            sample_rate,
                            channels,
                        });
                    }
                }
            }
        }
    }

    Ok(Json(files))
}

/// Upload a WAV file (binary POST body)
pub async fn upload_audio_file(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    if !validate_filename(&name) {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Write file to current directory
    fs::write(&name, &body).map_err(|e| {
        log::error!("Failed to write audio file '{}': {}", name, e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    log::info!("Uploaded audio file: {} ({} bytes)", name, body.len());

    Ok(Json(serde_json::json!({
        "success": true,
        "name": name,
        "size": body.len()
    })))
}

/// Download/stream a WAV file
pub async fn download_audio_file(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    if !validate_filename(&name) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let path = PathBuf::from(&name);
    if !path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let data = fs::read(path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let content_type = if name.to_lowercase().ends_with(".mp3") {
        "audio/mpeg"
    } else {
        "audio/wav"
    };

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(content_type),
    );
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&content_disposition(&name))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );

    Ok((response_headers, data))
}

/// Delete a WAV file
pub async fn delete_audio_file(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    if !validate_filename(&name) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let path = PathBuf::from(&name);
    if !path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    fs::remove_file(path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    log::info!("Deleted audio file: {}", name);

    Ok(Json(serde_json::json!({ "success": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn validate_filename_rejects_traversal_and_control_chars() {
        assert!(validate_filename("greeting.wav"));
        assert!(validate_filename("kayıt.wav"));
        assert!(!validate_filename("../etc/passwd.wav"));
        assert!(!validate_filename("dir/greeting.wav"));
        assert!(!validate_filename("dir\\greeting.wav"));
        assert!(!validate_filename("bad\r\nname.wav"));
        assert!(!validate_filename("notaudio.txt"));
        assert!(!validate_filename(""));
    }

    /// Non-ASCII filenames are valid on disk but are not valid header bytes;
    /// building the header from the raw name used to panic on unwrap.
    #[test]
    fn content_disposition_is_a_valid_header_value_for_non_ascii_names() {
        for name in ["kayıt.wav", "günaydın şirket.wav", "quote\".wav", "ok.wav"] {
            let value = content_disposition(name);
            assert!(
                HeaderValue::from_str(&value).is_ok(),
                "not a valid header value for {}: {}",
                name,
                value
            );
        }
    }

    #[test]
    fn content_disposition_keeps_the_real_name_in_filename_star() {
        let value = content_disposition("kayıt.wav");
        assert!(value.contains("filename=\"kay_t.wav\""), "{}", value);
        assert!(
            value.contains("filename*=UTF-8''kay%C4%B1t.wav"),
            "{}",
            value
        );
    }

    #[test]
    fn content_disposition_passes_ascii_names_through() {
        assert_eq!(
            content_disposition("greeting.wav"),
            "inline; filename=\"greeting.wav\"; filename*=UTF-8''greeting.wav"
        );
    }
}
