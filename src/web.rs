use axum::{Router, routing::get, response::{Html, IntoResponse}, Json, extract::State, serve, extract::ws::{WebSocketUpgrade, Message}};
use serde::{Serialize, Deserialize};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use crate::hub::Hub;
use serde_json;
use crate::uplink::UplinkStatus;
use serde_json::json;
use std::time::Duration;

#[derive(Serialize, Deserialize)]
pub struct Status {
    pub server_name: String,
    pub uptime: u64,
    pub clients: usize,
}

#[derive(Serialize, Deserialize)]
pub struct ClientInfo {
    pub id: usize,
    pub callsign: Option<String>,
    pub filter: Option<Vec<crate::filter::ClientFilter>>,
}

#[derive(Clone)]
pub struct AppState {
    pub hub: Arc<Mutex<Hub>>,
    pub uplink_status: Arc<Mutex<UplinkStatus>>,
}

fn filter_summary(filters: &Option<Vec<crate::filter::ClientFilter>>) -> String {
    match filters {
        Some(fs) => fs.iter().map(|f| format!("{:?}", f)).collect::<Vec<_>>().join(", "),
        None => String::new(),
    }
}

async fn root(State(state): State<AppState>) -> impl IntoResponse {
    let mut hub_guard = state.hub.lock().unwrap();
    hub_guard.update_totals();
    let started = hub_guard.start_time;
    let _uptime = hub_guard.uptime();
    let _server_id = "aprsserver-rust";
    let _admin = "admin@example.com";
    let _email = "admin@example.com";
    let _software = "aprsserver-rust";
    let _version = "0.1.0";
    let _os = std::env::consts::OS;
    let _started_str = format!("{:?}", started);
    let uplink = state.uplink_status.lock().unwrap();
    let uplink_table = format!(r#"
    <table class="min-w-full bg-white rounded shadow overflow-hidden mb-4">
      <thead><tr><th class="bg-purple-100 px-4 py-2 text-left" colspan="2">Uplink</th></tr></thead>
      <tbody>
        <tr><td class="px-4 py-2 font-semibold">Host</td><td class="px-4 py-2" id="uplink-host">{}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Port</td><td class="px-4 py-2" id="uplink-port">{}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Connected</td><td class="px-4 py-2" id="uplink-connected">{}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Last Connect</td><td class="px-4 py-2" id="uplink-last-connect">{:?}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Packets RX</td><td class="px-4 py-2" id="uplink-packets-rx">{}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Packets TX</td><td class="px-4 py-2" id="uplink-packets-tx">{}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Bytes RX</td><td class="px-4 py-2" id="uplink-bytes-rx">{}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Bytes TX</td><td class="px-4 py-2" id="uplink-bytes-tx">{}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Connect Errors</td><td class="px-4 py-2" id="uplink-connect-errors">{}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Read Errors</td><td class="px-4 py-2" id="uplink-read-errors">{}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Write Errors</td><td class="px-4 py-2" id="uplink-write-errors">{}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Last Error</td><td class="px-4 py-2" id="uplink-last-error">{}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Last RX Time</td><td class="px-4 py-2" id="uplink-last-rx-time">{:?}</td></tr>
        <tr><td class="px-4 py-2 font-semibold">Last TX Time</td><td class="px-4 py-2" id="uplink-last-tx-time">{:?}</td></tr>
      </tbody>
    </table>
    "#,
    uplink.host,
    uplink.port,
    uplink.connected,
    uplink.last_connect,
    uplink.packets_rx,
    uplink.packets_tx,
    uplink.bytes_rx,
    uplink.bytes_tx,
    uplink.connect_errors,
    uplink.read_errors,
    uplink.write_errors,
    uplink.last_error.as_deref().unwrap_or(""),
    uplink.last_rx_time,
    uplink.last_tx_time
    );
    let s2s_peers_table = {
        let mut rows = String::new();
        for peer in &hub_guard.s2s_peers {
            let p = peer.lock().unwrap();
            rows.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{:?}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:?}</td><td>{:?}</td></tr>", p.host, p.port, p.peer_name, p.connected, p.packets_rx, p.packets_tx, p.bytes_rx, p.bytes_tx, p.connect_errors, p.read_errors, p.write_errors, p.last_error, p.last_connect));
        }
        format!("<table class='min-w-full bg-white rounded shadow overflow-hidden mb-4'><thead><tr><th class='bg-yellow-100 px-4 py-2 text-left' colspan='13'>S2S Peers</th></tr><tr><th>Host</th><th>Port</th><th>Peer Name</th><th>Connected</th><th>Packets RX</th><th>Packets TX</th><th>Bytes RX</th><th>Bytes TX</th><th>Connect Errors</th><th>Read Errors</th><th>Write Errors</th><th>Last Error</th><th>Last Connect</th></tr></thead><tbody id='s2s-peers-tbody'>{}</tbody></table>", rows)
    };
    let mut html = String::from(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>APRS Server Status</title>
  <script src="https://cdn.tailwindcss.com"></script>
</head>
<body class="bg-gray-50 text-gray-900">
<div class="max-w-4xl mx-auto p-4">
<h1 class="text-3xl font-bold mb-4">APRS Server Status</h1>
<script>
let ws = new WebSocket(`ws://${location.host}/ws`);
ws.onmessage = function(event) {
  try {
    const data = JSON.parse(event.data);
    if (data.server_name) {
      document.getElementById('uptime').textContent = data.uptime + ' seconds';
    } else if (Array.isArray(data)) {
      let tbody = '';
      for (const c of data) {
        tbody += `<tr class='hover:bg-gray-100'><td class='px-2 py-1 border'>${c.id}</td><td class='px-2 py-1 border'>${c.callsign ?? ''}</td><td class='px-2 py-1 border'>${c.filter ?? ''}</td></tr>`;
      }
      document.getElementById('clients-tbody').innerHTML = tbody;
    } else if (data.uplink) {
      for (const [k, v] of Object.entries(data.uplink)) {
        const el = document.getElementById('uplink-' + k.replace(/_/g, '-'));
        if (el) el.textContent = v ?? '';
      }
    } else if (data.s2s_peers) {
      let tbody = data.s2s_peers.map(p =>
        `<tr><td class='px-2 py-1 border'>${p.host}</td><td class='px-2 py-1 border'>${p.port}</td><td class='px-2 py-1 border'>${p.peer_name ?? ''}</td><td class='px-2 py-1 border'>${p.connected}</td><td class='px-2 py-1 border'>${p.packets_rx}</td><td class='px-2 py-1 border'>${p.packets_tx}</td><td class='px-2 py-1 border'>${p.bytes_rx}</td><td class='px-2 py-1 border'>${p.bytes_tx}</td><td class='px-2 py-1 border'>${p.connect_errors}</td><td class='px-2 py-1 border'>${p.read_errors}</td><td class='px-2 py-1 border'>${p.write_errors}</td><td class='px-2 py-1 border'>${p.last_error ?? ''}</td><td class='px-2 py-1 border'>${p.last_connect ?? ''}</td></tr>`
      ).join('');
      document.getElementById('s2s-peers-tbody').innerHTML = tbody;
    }
  } catch (e) {}
};
</script>
    html.push_str(&uplink_table);
    html.push_str(&s2s_peers_table);
    html.push_str("<div class='mb-6'>
<table class='min-w-full bg-white rounded shadow overflow-hidden mb-4'>
  <thead><tr><th class='bg-blue-100 px-4 py-2 text-left' colspan='2'>Server Info</th></tr></thead>
  <tbody>
    <tr><td class='px-4 py-2 font-semibold'>Server ID</td><td class='px-4 py-2'>aprsserver-rust</td></tr>
    <tr><td class='px-4 py-2 font-semibold'>Admin</td><td class='px-4 py-2'>admin@example.com</td></tr>
    <tr><td class='px-4 py-2 font-semibold'>Email</td><td class='px-4 py-2'>admin@example.com</td></tr>
    <tr><td class='px-4 py-2 font-semibold'>Software</td><td class='px-4 py-2'>aprsserver-rust 0.1.0</td></tr>
    <tr><td class='px-4 py-2 font-semibold'>Uptime</td><td class='px-4 py-2' id='uptime'>{} seconds</td></tr>
    <tr><td class='px-4 py-2 font-semibold'>Started</td><td class='px-4 py-2'>{}</td></tr>
    <tr><td class='px-4 py-2 font-semibold'>OS</td><td class='px-4 py-2'>{}</td></tr>
  </tbody>
</table>

<table class='min-w-full bg-white rounded shadow overflow-hidden mb-4'>
  <thead><tr><th class='bg-green-100 px-4 py-2 text-left' colspan='4'>Totals</th></tr></thead>
  <tbody>
    <tr><th class='px-4 py-2'>Packets RX</th><th class='px-4 py-2'>Packets TX</th><th class='px-4 py-2'>Bytes RX</th><th class='px-4 py-2'>Bytes TX</th></tr>
    <tr><td class='px-4 py-2'>{}</td><td class='px-4 py-2'>{}</td><td class='px-4 py-2'>{}</td><td class='px-4 py-2'>{}</td></tr>
  </tbody>
</table>

<table class='min-w-full bg-white rounded shadow overflow-hidden'>
  <thead><tr class='bg-gray-200'>
    <th class='px-2 py-1'>ID</th>
    <th class='px-2 py-1'>Callsign</th>
    <th class='px-2 py-1'>Filter</th>
    <th class='px-2 py-1'>Packets RX</th>
    <th class='px-2 py-1'>Packets TX</th>
    <th class='px-2 py-1'>Bytes RX</th>
    <th class='px-2 py-1'>Bytes TX</th>
    <th class='px-2 py-1'>Connect Time (s)</th>
  </tr></thead>
  <tbody id='clients-tbody'>
"#);
    for (id, client) in &hub_guard.clients {
        let c = client.lock().unwrap();
        let connect_secs = c.connect_time.elapsed().as_secs();
        html.push_str(&format!("<tr class='hover:bg-gray-100'><td class='px-2 py-1 border'>{}</td><td class='px-2 py-1 border'>{:?}</td><td class='px-2 py-1 border'>{}</td><td class='px-2 py-1 border'>{}</td><td class='px-2 py-1 border'>{}</td><td class='px-2 py-1 border'>{}</td><td class='px-2 py-1 border'>{}</td><td class='px-2 py-1 border'>{}</td></tr>", id, c.callsign, filter_summary(&c.filter), c.packets_rx, c.packets_tx, c.bytes_rx, c.bytes_tx, connect_secs));
    }
    html.push_str("</tbody></table>");
    html.push_str("<div class='mt-4 text-sm text-gray-500'>See <a class='underline text-blue-600' href='/status.json'>/status.json</a> and <a class='underline text-blue-600' href='/clients.json'>/clients.json</a></div>");
    html.push_str("</div></body></html>");
    Html(html)
}

async fn status(State(state): State<AppState>) -> Json<Status> {
    let hub = state.hub.lock().unwrap();
    Json(Status {
        server_name: "aprsserver-rust".to_string(),
        uptime: hub.uptime(),
        clients: hub.client_count(),
    })
}

async fn clients(State(state): State<AppState>) -> Json<Vec<ClientInfo>> {
    let hub = state.hub.lock().unwrap();
    let mut out = Vec::new();
    for (id, client) in &hub.clients {
        let c = client.lock().unwrap();
        out.push(ClientInfo {
            id: *id,
            callsign: c.callsign.clone(),
            filter: c.filter.clone(),
        });
    }
    Json(out)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let hub = state.hub.clone();
    let uplink_status = state.uplink_status.clone();
    ws.on_upgrade(move |mut socket| async move {
        loop {
            let (uptime, s2s_peers_json, uplink_json) = {
                let hub_guard = hub.lock().unwrap();
                let uptime = hub_guard.uptime();
                let s2s_peers: Vec<_> = hub_guard.s2s_peers.iter().map(|peer| {
                    let p = peer.lock().unwrap();
                    json!({
                        "host": p.host,
                        "port": p.port,
                        "peer_name": p.peer_name,
                        "connected": p.connected,
                        "packets_rx": p.packets_rx,
                        "packets_tx": p.packets_tx,
                        "bytes_rx": p.bytes_rx,
                        "bytes_tx": p.bytes_tx,
                        "connect_errors": p.connect_errors,
                        "read_errors": p.read_errors,
                        "write_errors": p.write_errors,
                        "last_error": p.last_error,
                        "last_connect": p.last_connect.map(|t| format!("{:?}", t)),
                    })
                }).collect();
                let s2s_json = json!({"s2s_peers": s2s_peers});
                let uplink = uplink_status.lock().unwrap();
                let uplink_json = json!({
                    "uplink": {
                        "host": uplink.host,
                        "port": uplink.port,
                        "connected": uplink.connected,
                        "last_connect": uplink.last_connect.map(|t| format!("{:?}", t)),
                        "packets_rx": uplink.packets_rx,
                        "packets_tx": uplink.packets_tx,
                        "bytes_rx": uplink.bytes_rx,
                        "bytes_tx": uplink.bytes_tx,
                        "connect_errors": uplink.connect_errors,
                        "read_errors": uplink.read_errors,
                        "write_errors": uplink.write_errors,
                        "last_error": uplink.last_error,
                        "last_rx_time": uplink.last_rx_time.map(|t| format!("{:?}", t)),
                        "last_tx_time": uplink.last_tx_time.map(|t| format!("{:?}", t)),
                    }
                });
                (uptime, s2s_json, uplink_json)
            };
            let stats = json!({
                "server_name": "aprsserver-rust",
                "uptime": uptime,
            });
            if socket.send(Message::Text(stats.to_string())).await.is_err() {
                break;
            }
            if socket.send(Message::Text(uplink_json.to_string())).await.is_err() {
                break;
            }
            if socket.send(Message::Text(s2s_peers_json.to_string())).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}

async fn live_reload(State(state): State<AppState>) -> String {
    let hub = state.hub.lock().unwrap();
    hub.start_time.elapsed().as_secs().to_string()
}

pub async fn serve_web_ui(addr: &str, hub: Arc<Mutex<Hub>>, uplink_status: Arc<Mutex<UplinkStatus>>) {
    let app = Router::new()
        .route("/", get(root))
        .route("/status.json", get(status))
        .route("/clients.json", get(clients))
        .route("/ws", get(ws_handler))
        .route("/live-reload", get(live_reload))
        .with_state(AppState { hub, uplink_status });
    let addr: SocketAddr = addr.parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    serve(listener, app.into_make_service()).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::task;
    use reqwest;
    use crate::hub::Hub;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn test_status_endpoint() {
        let addr = "127.0.0.1:3001";
        let hub = Arc::new(Mutex::new(Hub::new()));
        let hub2 = hub.clone();
        let dummy_cfg = UplinkConfig {
            host: "dummy".to_string(),
            port: 0,
            callsign: "dummy".to_string(),
            passcode: 0,
        };
        task::spawn(async move {
            serve_web_ui(addr, hub2, Arc::new(Mutex::new(UplinkStatus::new(&dummy_cfg)))).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let resp = reqwest::get(&format!("http://{}/status.json", addr)).await.unwrap();
        assert!(resp.status().is_success());
        let status: Status = resp.json().await.unwrap();
        assert_eq!(status.server_name, "aprsserver-rust");
    }
} 