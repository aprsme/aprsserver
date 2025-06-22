use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use crate::server::is_valid_aprs_packet;
use tokio::sync::mpsc::unbounded_channel;
use crate::hub::S2SPeerHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc as StdArc;
use signal_hook::consts::signal::SIGHUP;
use signal_hook::flag;
use tokio::sync::Mutex as TokioMutex;

mod server;
mod config;
mod filter;
mod client;
mod hub;
mod web;
mod uplink;

#[tokio::main]
async fn main() {
    // SIGHUP reload flag
    let reload_flag = StdArc::new(AtomicBool::new(false));
    flag::register(SIGHUP, reload_flag.clone()).unwrap();

    let config = match config::Config::load_from_file("aprsserver.toml") {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
            std::process::exit(1);
        }
    };

    let hub = Arc::new(Mutex::new(hub::Hub::new()));
    let uplink_status = Arc::new(Mutex::new(
        config.uplink.as_ref().map(|cfg| uplink::UplinkStatus::new(cfg)).unwrap_or_else(|| uplink::UplinkStatus {
            host: "".to_string(),
            port: 0,
            connected: false,
            last_connect: None,
            packets_rx: 0,
            packets_tx: 0,
            bytes_rx: 0,
            bytes_tx: 0,
            connect_errors: 0,
            read_errors: 0,
            write_errors: 0,
            last_error: None,
            last_rx_time: None,
            last_tx_time: None,
        })
    ));
    let hub_web = hub.clone();
    let uplink_status_web = uplink_status.clone();

    // Start web UI in background
    tokio::spawn(web::serve_web_ui("0.0.0.0:14501", hub_web, uplink_status_web));

    // Start uplink in background if configured
    if let Some(uplink_cfg) = config.uplink.clone() {
        let hub_uplink = hub.clone();
        let uplink_status_uplink = uplink_status.clone();
        tokio::spawn(uplink::connect_and_run(uplink_cfg, hub_uplink, uplink_status_uplink));
    }

    // Start S2S peers in background if configured
    if let Some(s2s_peers) = config.s2s_peers.clone() {
        for peer_cfg in s2s_peers {
            let status = Arc::new(Mutex::new(hub::S2SPeerStatus::new(
                peer_cfg.host.clone(),
                peer_cfg.port,
                peer_cfg.peer_name.clone(),
            )));
            hub.lock().unwrap().s2s_peers.push(status.clone());
            let hub_s2s = hub.clone();
            tokio::spawn(connect_s2s_peer(peer_cfg, status, hub_s2s));
        }
    }

    // Start S2S listener for incoming peers
    let s2s_port = config.s2s_port.unwrap_or(14579);
    let s2s_listener = TcpListener::bind(("0.0.0.0", s2s_port)).expect("Could not bind to S2S port");
    println!("S2S listener on port {}", s2s_port);
    let hub_s2s_listener = hub.clone();
    std::thread::spawn(move || {
        for stream in s2s_listener.incoming() {
            match stream {
                Ok(stream) => {
                    let hub = hub_s2s_listener.clone();
                    std::thread::spawn(|| {
                        s2s_server_handler(stream, hub);
                    });
                }
                Err(e) => {
                    eprintln!("S2S port connection failed: {}", e);
                }
            }
        }
    });

    let user_listener = TcpListener::bind(("0.0.0.0", config.user_port)).expect("Could not bind to user port");
    let server_listener = TcpListener::bind(("0.0.0.0", config.server_port)).expect("Could not bind to server port");
    println!("{} listening on ports {} (user) and {} (server)", config.server_name, config.user_port, config.server_port);

    let hub_server = hub.clone();
    let server_thread = std::thread::spawn(move || {
        for stream in server_listener.incoming() {
            match stream {
                Ok(stream) => {
                    let hub = hub_server.clone();
                    std::thread::spawn(|| {
                        server::handle_client(stream, hub);
                    });
                }
                Err(e) => {
                    eprintln!("Server port connection failed: {}", e);
                }
            }
        }
    });

    for stream in user_listener.incoming() {
        match stream {
            Ok(stream) => {
                let hub = hub.clone();
                std::thread::spawn(|| {
                    server::handle_client(stream, hub);
                });
            }
            Err(e) => {
                eprintln!("User port connection failed: {}", e);
            }
        }
    }

    let _ = server_thread.join();

    // Main server loop (after all listeners started)
    loop {
        if reload_flag.load(Ordering::Relaxed) {
            println!("SIGHUP received: would reload config here");
            reload_flag.store(false, Ordering::Relaxed);
            // TODO: actually reload config and update state
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[allow(unused)]
pub async fn connect_s2s_peer(cfg: config::S2SPeerConfig, status: Arc<Mutex<hub::S2SPeerStatus>>, hub: Arc<Mutex<hub::Hub>>) {
    let addr = format!("{}:{}", cfg.host, cfg.port);
    loop {
        match TcpStream::connect(&addr).await {
            Ok(stream) => {
                {
                    let mut s = status.lock().unwrap();
                    s.connected = true;
                    s.last_connect = Some(std::time::SystemTime::now());
                    s.last_error = None;
                }
                println!("Connected to S2S peer {}", addr);
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                // Outgoing channel for this peer
                let (tx, mut rx) = unbounded_channel::<String>();
                // Register handle in hub
                {
                    let mut hub = hub.lock().unwrap();
                    hub.s2s_peer_handles.push(S2SPeerHandle {
                        peer_name: cfg.peer_name.clone(),
                        sender: tx.clone(),
                    });
                }
                let writer = Arc::new(TokioMutex::new(writer));
                // Spawn task to forward outgoing packets
                let writer_clone = writer.clone();
                tokio::spawn(async move {
                    while let Some(pkt) = rx.recv().await {
                        let mut w = writer_clone.lock().await;
                        let _ = w.write_all(pkt.as_bytes()).await;
                    }
                });
                // Send S2S login line (aprsc style)
                let login = format!("# aprsc 2.1.5 s2s {} {} 14579\n", cfg.peer_name.clone().unwrap_or("aprsserver-rust".to_string()), cfg.passcode);
                let mut w = writer.lock().await;
                match w.write_all(login.as_bytes()).await {
                    Ok(_) => {
                        let mut s = status.lock().unwrap();
                        s.packets_tx += 1;
                        s.bytes_tx += login.len() as u64;
                        s.last_tx_time = Some(std::time::SystemTime::now());
                    }
                    Err(e) => {
                        let mut s = status.lock().unwrap();
                        s.write_errors += 1;
                        s.last_error = Some(format!("login send: {}", e));
                        s.connected = false;
                        // Remove handle on disconnect
                        let mut hub = hub.lock().unwrap();
                        hub.s2s_peer_handles.retain(|h| h.peer_name != cfg.peer_name);
                        continue;
                    }
                }
                // Wait for peer's login/ack
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        let mut s = status.lock().unwrap();
                        s.connected = false;
                        s.read_errors += 1;
                        s.last_error = Some("peer closed connection".to_string());
                        continue;
                    }
                    Ok(n) => {
                        let mut s = status.lock().unwrap();
                        s.packets_rx += 1;
                        s.bytes_rx += n as u64;
                        s.last_rx_time = Some(std::time::SystemTime::now());
                        println!("S2S peer login/ack: {}", line.trim());
                    }
                    Err(e) => {
                        let mut s = status.lock().unwrap();
                        s.connected = false;
                        s.read_errors += 1;
                        s.last_error = Some(format!("read: {}", e));
                        continue;
                    }
                }
                // Main loop: keepalive and relay
                loop {
                    // Read from peer
                    let mut line = String::new();
                    tokio::select! {
                        read = reader.read_line(&mut line) => {
                            match read {
                                Ok(0) => break, // peer closed
                                Ok(n) => {
                                    let packet = line.trim();
                                    if is_valid_aprs_packet(packet) {
                                        let mut hub = hub.lock().unwrap();
                                        if !hub.check_and_insert_dupe(packet) {
                                            hub.broadcast_packet(0, packet); // 0 = S2S sender
                                            hub.broadcast_to_s2s_peers(cfg.peer_name.as_deref(), packet);
                                        }
                                    }
                                    let mut s = status.lock().unwrap();
                                    s.packets_rx += 1;
                                    s.bytes_rx += n as u64;
                                    s.last_rx_time = Some(std::time::SystemTime::now());
                                }
                                Err(e) => {
                                    let mut s = status.lock().unwrap();
                                    s.connected = false;
                                    s.read_errors += 1;
                                    s.last_error = Some(format!("read: {}", e));
                                    break;
                                }
                            }
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                            let keepalive = b"# keepalive\n";
                            let mut w = writer.lock().await;
                            if let Err(e) = w.write_all(keepalive).await {
                                let mut s = status.lock().unwrap();
                                s.connected = false;
                                s.write_errors += 1;
                                s.last_error = Some(format!("keepalive: {}", e));
                                break;
                            }
                        }
                    }
                }
                // Remove handle on disconnect
                let mut hub = hub.lock().unwrap();
                hub.s2s_peer_handles.retain(|h| h.peer_name != cfg.peer_name);
            }
            Err(e) => {
                {
                    let mut s = status.lock().unwrap();
                    s.connected = false;
                    s.connect_errors += 1;
                    s.last_error = Some(format!("connect: {}", e));
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

#[allow(unused)]
pub fn s2s_server_handler(mut stream: std::net::TcpStream, hub: std::sync::Arc<std::sync::Mutex<hub::Hub>>) {
    use std::io::{BufRead, BufReader, Write};
    use std::time::Duration;
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());
    println!("Incoming S2S connection from {}", peer);
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    // Outgoing channel for this peer
    let (tx, rx) = unbounded_channel::<String>();
    // Register handle in hub
    {
        let mut hub = hub.lock().unwrap();
        hub.s2s_peer_handles.push(S2SPeerHandle {
            peer_name: Some(peer.clone()),
            sender: tx.clone(),
        });
    }
    // Spawn thread to forward outgoing packets
    let mut writer = stream.try_clone().unwrap();
    std::thread::spawn(move || {
        let mut rx = rx;
        while let Some(pkt) = rx.blocking_recv() {
            let _ = writer.write_all(pkt.as_bytes());
        }
    });
    // Wait for S2S login line
    match reader.read_line(&mut line) {
        Ok(0) => {
            println!("S2S peer {} disconnected before login", peer);
            // Remove handle on disconnect
            let mut hub = hub.lock().unwrap();
            hub.s2s_peer_handles.retain(|h| h.peer_name.as_deref() != Some(&peer));
            return;
        }
        Ok(_) => {
            println!("S2S peer login: {}", line.trim());
            // TODO: parse and validate login line
            // Send our own login/ack
            let login = format!("# aprsc 2.1.5 s2s aprsserver-rust 12345 14579\n");
            if let Err(e) = stream.write_all(login.as_bytes()) {
                eprintln!("S2S send login error: {}", e);
                // Remove handle on disconnect
                let mut hub = hub.lock().unwrap();
                hub.s2s_peer_handles.retain(|h| h.peer_name.as_deref() != Some(&peer));
                return;
            }
        }
        Err(e) => {
            eprintln!("S2S read login error: {}", e);
            // Remove handle on disconnect
            let mut hub = hub.lock().unwrap();
            hub.s2s_peer_handles.retain(|h| h.peer_name.as_deref() != Some(&peer));
            return;
        }
    }
    // Main loop: keepalive and relay
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(n) => {
                let packet = line.trim();
                if is_valid_aprs_packet(packet) {
                    let mut hub = hub.lock().unwrap();
                    if !hub.check_and_insert_dupe(packet) {
                        hub.broadcast_packet(0, packet); // 0 = S2S sender
                        hub.broadcast_to_s2s_peers(Some(&peer), packet);
                    }
                }
            }
            Err(e) => {
                eprintln!("S2S read error: {}", e);
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Remove handle on disconnect
    let mut hub = hub.lock().unwrap();
    hub.s2s_peer_handles.retain(|h| h.peer_name.as_deref() != Some(&peer));
}
