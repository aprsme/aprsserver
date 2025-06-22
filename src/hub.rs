use crate::client::Client;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

pub struct S2SPeerHandle {
    pub peer_name: Option<String>,
    pub sender: UnboundedSender<String>,
}

pub struct Hub {
    pub clients: HashMap<usize, Arc<Mutex<Client>>>,
    pub start_time: Instant,
    pub next_id: usize,
    pub total_packets_rx: u64,
    pub total_packets_tx: u64,
    pub total_bytes_rx: u64,
    pub total_bytes_tx: u64,
    pub s2s_peers: Vec<Arc<Mutex<S2SPeerStatus>>>,
    pub s2s_peer_handles: Vec<S2SPeerHandle>,
    pub dupe_cache: HashSet<u64>,
    pub dupe_order: VecDeque<u64>,
}

const DUPE_CACHE_SIZE: usize = 1000;

#[derive(Debug, Clone)]
pub struct S2SPeerStatus {
    pub host: String,
    pub port: u16,
    pub peer_name: Option<String>,
    pub connected: bool,
    pub last_connect: Option<std::time::SystemTime>,
    pub packets_rx: u64,
    pub packets_tx: u64,
    pub bytes_rx: u64,
    pub bytes_tx: u64,
    pub connect_errors: u64,
    pub read_errors: u64,
    pub write_errors: u64,
    pub last_error: Option<String>,
    pub last_rx_time: Option<std::time::SystemTime>,
    pub last_tx_time: Option<std::time::SystemTime>,
}

impl S2SPeerStatus {
    pub fn new(host: String, port: u16, peer_name: Option<String>) -> Self {
        Self {
            host,
            port,
            peer_name,
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

impl Hub {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            start_time: Instant::now(),
            next_id: 1,
            total_packets_rx: 0,
            total_packets_tx: 0,
            total_bytes_rx: 0,
            total_bytes_tx: 0,
            s2s_peers: Vec::new(),
            s2s_peer_handles: Vec::new(),
            dupe_cache: HashSet::new(),
            dupe_order: VecDeque::new(),
        }
    }
    pub fn add_client(&mut self, client: Client) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.clients.insert(id, Arc::new(Mutex::new(client)));
        id
    }
    pub fn remove_client(&mut self, id: usize) {
        self.clients.remove(&id);
    }
    pub fn update_client(
        &mut self,
        id: usize,
        callsign: Option<String>,
        filter: Option<Vec<crate::filter::ClientFilter>>,
    ) {
        if let Some(client) = self.clients.get(&id) {
            let mut c = client.lock().unwrap();
            c.callsign = callsign;
            c.filter = filter;
        }
    }
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }
    pub fn uptime(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
    pub fn update_totals(&mut self) {
        self.total_packets_rx = 0;
        self.total_packets_tx = 0;
        self.total_bytes_rx = 0;
        self.total_bytes_tx = 0;
        for client in self.clients.values() {
            let c = client.lock().unwrap();
            self.total_packets_rx += c.packets_rx;
            self.total_packets_tx += c.packets_tx;
            self.total_bytes_rx += c.bytes_rx;
            self.total_bytes_tx += c.bytes_tx;
        }
    }
    pub fn get_totals(&self) -> (u64, u64, u64, u64) {
        (
            self.total_packets_rx,
            self.total_packets_tx,
            self.total_bytes_rx,
            self.total_bytes_tx,
        )
    }
    pub fn broadcast_packet(&self, sender_id: usize, packet: &str) {
        for (id, client) in &self.clients {
            if *id != sender_id {
                let c = client.lock().unwrap();
                if let Ok(mut stream) = c.stream.lock() {
                    let _ = stream.write_all(packet.as_bytes());
                }
            }
        }
    }
    pub fn check_and_insert_dupe(&mut self, packet: &str) -> bool {
        let hash = seahash::hash(packet.as_bytes());
        if self.dupe_cache.contains(&hash) {
            return true;
        }
        self.dupe_cache.insert(hash);
        self.dupe_order.push_back(hash);
        if self.dupe_order.len() > DUPE_CACHE_SIZE {
            if let Some(old) = self.dupe_order.pop_front() {
                self.dupe_cache.remove(&old);
            }
        }
        false
    }
    pub fn broadcast_to_s2s_peers(&self, sender: Option<&str>, packet: &str) {
        for handle in &self.s2s_peer_handles {
            if let Some(name) = &handle.peer_name {
                if let Some(sender_name) = sender {
                    if name == sender_name { continue; }
                }
            }
            let _ = handle.sender.send(packet.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};
    #[test]
    fn test_hub_add_remove() {
        let mut hub = Hub::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let client = Client::new(1, stream);
        let id = hub.add_client(client);
        assert_eq!(hub.client_count(), 1);
        hub.remove_client(id);
        assert_eq!(hub.client_count(), 0);
    }
    #[test]
    fn test_hub_update_client() {
        let mut hub = Hub::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let client = Client::new(1, stream);
        let id = hub.add_client(client);
        hub.update_client(
            id,
            Some("N0CALL".to_string()),
            Some(vec![crate::filter::ClientFilter::Prefix("foo".to_string())]),
        );
        let c = hub.clients.get(&id).unwrap().lock().unwrap();
        assert_eq!(c.callsign, Some("N0CALL".to_string()));
        assert_eq!(c.filter, Some(vec![crate::filter::ClientFilter::Prefix("foo".to_string())]));
    }
    #[test]
    fn test_hub_uptime() {
        let hub = Hub::new();
        assert!(hub.uptime() < 2);
    }
    #[test]
    fn test_broadcast_packet() {
        let mut hub = Hub::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stream1 = TcpStream::connect(addr).unwrap();
        let stream2 = TcpStream::connect(addr).unwrap();
        let client1 = Client::new(1, stream1.try_clone().unwrap());
        let client2 = Client::new(2, stream2.try_clone().unwrap());
        let id1 = hub.add_client(client1);
        let id2 = hub.add_client(client2);
        hub.broadcast_packet(id1, "test123\n");
        let mut buf = [0u8; 128];
        let mut s2 = stream2.try_clone().unwrap();
        let n = s2.read(&mut buf).unwrap_or(0);
        assert!(std::str::from_utf8(&buf[..n]).unwrap().contains("test123"));
        // Sender should not receive its own packet
        let mut s1 = stream1.try_clone().unwrap();
        let n = s1.read(&mut buf).unwrap_or(0);
        assert_eq!(n, 0);
        hub.remove_client(id1);
        hub.remove_client(id2);
    }
}
