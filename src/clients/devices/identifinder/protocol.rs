//! IdentiFINDER R400 HTTP/SignalR protocol DTOs and stream line parser.
//!
//! Ported from `luracs/clients/identifinder_client/identifinder.py`.

use serde::Deserialize;

pub const DEFAULT_CHANNELS: usize = 1024;

pub mod endpoints {
    pub const API: &str = "/virtualPha/api/";
    pub const SIGNALR: &str = "/virtualPha/signalR";
}

#[derive(Debug, Deserialize)]
pub struct SignalRMessage {
    #[serde(rename = "M")]
    pub messages: Option<Vec<SignalRPayload>>,
}

#[derive(Debug, Deserialize)]
pub struct SignalRPayload {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub running: bool,
    #[serde(rename = "Misc")]
    pub misc: Option<MiscDto>,
    #[serde(rename = "Spectrum")]
    pub spectrum: Option<Vec<f64>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct MiscDto {
    #[serde(rename = "AcquisitionMode")]
    pub acquisition_mode: Option<String>,
    #[serde(rename = "Location")]
    pub location: Option<String>,
    #[serde(rename = "LiveTime")]
    pub live_time: Option<serde_json::Value>,
    #[serde(rename = "RealTime")]
    pub real_time: Option<serde_json::Value>,
    #[serde(rename = "DoseRate")]
    pub dose_rate: Option<serde_json::Value>,
    #[serde(rename = "GammaRate")]
    pub gamma_rate: Option<serde_json::Value>,
}

/// Parse a single SSE (`data: {...}`) or raw SignalR JSON line into a successful payload.
#[must_use]
pub fn parse_stream_line(line: &str) -> Option<SignalRPayload> {
    let text = line.trim();
    let json_str = if let Some(rest) = text.strip_prefix("data:") {
        rest.trim()
    } else if text.starts_with('{') {
        text
    } else {
        return None;
    };

    if json_str.is_empty() {
        return None;
    }

    let msg: SignalRMessage = serde_json::from_str(json_str).ok()?;
    let payload = msg.messages?.into_iter().next()?;
    if !payload.success {
        return None;
    }

    Some(payload)
}

/// Parse seconds from `"54.0 s"` or a JSON number.
#[must_use]
pub fn parse_seconds(val: Option<&serde_json::Value>) -> f64 {
    match val {
        None | Some(serde_json::Value::Null) => 0.0,
        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(serde_json::Value::String(s)) => s
            .trim()
            .replace('s', "")
            .trim()
            .parse::<f64>()
            .unwrap_or(0.0),
        Some(_) => 0.0,
    }
}

/// Extract the leading floating-point number from strings like `"0.12 µSv/h"` or a JSON number.
#[must_use]
pub fn parse_leading_number(val: Option<&serde_json::Value>) -> f64 {
    match val {
        None | Some(serde_json::Value::Null) => 0.0,
        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(serde_json::Value::String(s)) => {
            let mut num = String::new();
            for ch in s.trim().chars() {
                if ch.is_ascii_digit() || ch == '.' || ch == '-' {
                    num.push(ch);
                } else if !num.is_empty() {
                    break;
                }
            }
            num.parse::<f64>().unwrap_or(0.0)
        }
        Some(_) => 0.0,
    }
}
