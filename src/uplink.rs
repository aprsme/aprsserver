use crate::config::UplinkConfig;
use crate::hub::Hub;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[derive(Debug, Clone)]
pub struct UplinkStatus {
    pub host: String,
    pub port: u16,
    pub connected: bool,
    pub last_connect: Option<SystemTime>,
    pub packets_rx: u64,
    pub packets_tx: u64,
    pub bytes_rx: u64,
    pub bytes_tx: u64,
    pub connect_errors: u64,
    pub read_errors: u64,
    pub write_errors: u64,
    pub last_error: Option<String>,
    pub last_rx_time: Option<SystemTime>,
    pub last_tx_time: Option<SystemTime>,
}

impl UplinkStatus {
    pub fn new(cfg: &UplinkConfig) -> Self {
        Self {
            host: cfg.host.clone(),
            port: cfg.port,
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
        }
    }
}

pub async fn connect_and_run(uplink: UplinkConfig, _hub: Arc<Mutex<Hub>>, status: Arc<Mutex<UplinkStatus>>) {
    let addr = format!("{}:{}", uplink.host, uplink.port);
    loop {
        match TcpStream::connect(&addr).await {
            Ok(stream) => {
                {
                    let mut s = status.lock().unwrap();
                    s.connected = true;
                    s.last_connect = Some(SystemTime::now());
                    s.last_error = None;
                }
                println!("Connected to uplink {}", addr);
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                let login = format!("user {} pass {} vers aprsserver-rust 0.1.0\n", uplink.callsign, uplink.passcode);
                match writer.write_all(login.as_bytes()).await {
                    Ok(_) => {
                        let mut s = status.lock().unwrap();
                        s.packets_tx += 1;
                        s.bytes_tx += login.len() as u64;
                        s.last_tx_time = Some(SystemTime::now());
                    }
                    Err(e) => {
                        let mut s = status.lock().unwrap();
                        s.write_errors += 1;
                        s.last_error = Some(format!("login send: {}", e));
                        s.connected = false;
                        continue;
                    }
                }
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => {
                            println!("Uplink disconnected");
                            let mut s = status.lock().unwrap();
                            s.connected = false;
                            break;
                        }
                        Ok(n) => {
                            let mut s = status.lock().unwrap();
                            s.packets_rx += 1;
                            s.bytes_rx += n as u64;
                            s.last_rx_time = Some(SystemTime::now());
                            print!("Uplink RX: {}", line);
                        }
                        Err(e) => {
                            eprintln!("Uplink read error: {}", e);
                            let mut s = status.lock().unwrap();
                            s.connected = false;
                            s.read_errors += 1;
                            s.last_error = Some(format!("read: {}", e));
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Uplink connect error: {}", e);
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