use crate::core::{
    extension_payload_to_tab, prune_stale_tabs, ExtensionCommand, ExtensionTabPayload,
    ExtensionTabSnapshot, TabEntry,
};
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const BRIDGE_ADDR: &str = "127.0.0.1:39276";
const MAX_BODY_BYTES: usize = 1024 * 1024;
const TAB_MAX_AGE: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct ExtensionBridge {
    state: Arc<Mutex<BridgeState>>,
}

#[derive(Default)]
struct BridgeState {
    tabs: Vec<TabEntry>,
    commands: VecDeque<ExtensionCommand>,
}

#[derive(Serialize)]
struct CommandResponse {
    commands: Vec<ExtensionCommand>,
}

impl ExtensionBridge {
    pub fn start() -> Result<Self> {
        let state = Arc::new(Mutex::new(BridgeState::default()));
        let listener = TcpListener::bind(BRIDGE_ADDR)
            .with_context(|| format!("failed to bind extension bridge on {BRIDGE_ADDR}"))?;
        listener
            .set_nonblocking(false)
            .context("failed to configure extension bridge listener")?;

        let thread_state = Arc::clone(&state);
        thread::Builder::new()
            .name("mega-win-alt-tab-extension-bridge".to_string())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else {
                        continue;
                    };
                    let state = Arc::clone(&thread_state);
                    let _ = thread::Builder::new()
                        .name("mega-win-alt-tab-extension-client".to_string())
                        .spawn(move || {
                            let _ = handle_client(stream, state);
                        });
                }
            })
            .context("failed to start extension bridge thread")?;

        Ok(Self { state })
    }

    pub fn tabs(&self) -> Vec<TabEntry> {
        let now = Instant::now();
        let mut state = self.state.lock().expect("extension bridge mutex poisoned");
        prune_stale_tabs(&mut state.tabs, now, TAB_MAX_AGE);
        state.tabs.clone()
    }

    pub fn queue_activate_tab(&self, browser: &str, window_id: i64, tab_id: i64) {
        let mut state = self.state.lock().expect("extension bridge mutex poisoned");
        state.commands.push_back(ExtensionCommand::ActivateTab {
            browser: browser.to_string(),
            window_id,
            tab_id,
        });
    }
}

fn handle_client(mut stream: TcpStream, state: Arc<Mutex<BridgeState>>) -> Result<()> {
    let request = HttpRequest::read(&mut stream)?;
    let response = route_request(request, state);
    stream
        .write_all(response.as_bytes())
        .context("failed to write extension bridge response")?;
    Ok(())
}

fn route_request(request: HttpRequest, state: Arc<Mutex<BridgeState>>) -> String {
    if request.method == "OPTIONS" {
        return json_response(204, "");
    }

    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/health") => json_response(200, r#"{"ok":true}"#),
        ("GET", "/commands") => {
            let commands = {
                let mut state = state.lock().expect("extension bridge mutex poisoned");
                state.commands.drain(..).collect::<Vec<_>>()
            };
            let body = serde_json::to_string(&CommandResponse { commands })
                .unwrap_or_else(|_| r#"{"commands":[]}"#.to_string());
            json_response(200, &body)
        }
        ("POST", "/tabs") => match parse_tabs_payload(&request.body) {
            Ok(tabs) => {
                let now = Instant::now();
                let mut state = state.lock().expect("extension bridge mutex poisoned");
                state.tabs = tabs
                    .into_iter()
                    .map(|payload| extension_payload_to_tab(payload, now))
                    .collect();
                json_response(200, r#"{"ok":true}"#)
            }
            Err(err) => json_response(400, &format!(r#"{{"error":"{}"}}"#, escape_json(&err))),
        },
        _ => json_response(404, r#"{"error":"not found"}"#),
    }
}

fn parse_tabs_payload(body: &[u8]) -> Result<Vec<ExtensionTabPayload>> {
    if body.len() > MAX_BODY_BYTES {
        return Err(anyhow!("request body too large"));
    }

    let snapshot: ExtensionTabSnapshot =
        serde_json::from_slice(body).context("invalid tab snapshot json")?;

    let mut tabs = Vec::with_capacity(snapshot.tabs.len());
    for mut tab in snapshot.tabs {
        if tab.browser.trim().is_empty() {
            tab.browser = snapshot.browser.clone();
        }
        if tab.title.trim().is_empty() {
            continue;
        }
        tabs.push(tab);
    }

    Ok(tabs)
}

fn json_response(status: u16, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "OK",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json; charset=utf-8\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: content-type\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    )
}

fn escape_json(value: &anyhow::Error) -> String {
    value.to_string().replace('\\', "\\\\").replace('"', "\\\"")
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

impl HttpRequest {
    fn read(stream: &mut TcpStream) -> Result<Self> {
        let mut buffer = Vec::new();
        let mut scratch = [0u8; 4096];
        let mut header_end = None;

        while header_end.is_none() && buffer.len() <= MAX_BODY_BYTES {
            let read = stream
                .read(&mut scratch)
                .context("failed to read request")?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&scratch[..read]);
            header_end = find_header_end(&buffer);
        }

        let header_end = header_end.ok_or_else(|| anyhow!("malformed http request"))?;
        let headers = String::from_utf8_lossy(&buffer[..header_end]);
        let mut lines = headers.lines();
        let request_line = lines
            .next()
            .ok_or_else(|| anyhow!("missing http request line"))?;
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts
            .next()
            .ok_or_else(|| anyhow!("missing http method"))?
            .to_string();
        let target = request_parts
            .next()
            .ok_or_else(|| anyhow!("missing http path"))?;
        let path = target.split('?').next().unwrap_or(target).to_string();

        let content_length = lines
            .filter_map(|line| line.split_once(':'))
            .find_map(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);

        if content_length > MAX_BODY_BYTES {
            return Err(anyhow!("request body too large"));
        }

        let body_start = header_end + 4;
        while buffer.len().saturating_sub(body_start) < content_length {
            let read = stream.read(&mut scratch).context("failed to read body")?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&scratch[..read]);
        }

        let available = buffer.len().saturating_sub(body_start);
        let body_len = available.min(content_length);
        let body = buffer[body_start..body_start + body_len].to_vec();

        Ok(Self { method, path, body })
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_snapshot_and_fills_empty_browser() {
        let body = br#"{
            "browser": "chrome",
            "tabs": [
                { "browser": "", "windowId": 1, "tabId": 2, "title": "Inbox", "active": true },
                { "browser": "chrome", "windowId": 1, "tabId": 3, "title": "", "active": false }
            ]
        }"#;

        let tabs = parse_tabs_payload(body).unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].browser, "chrome");
        assert_eq!(tabs[0].title, "Inbox");
    }
}
