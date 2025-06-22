use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Instant};
use crate::filter::ClientFilter;

#[allow(dead_code)]
#[derive(Debug)]
pub struct Client {
    pub _id: usize,
    pub stream: Arc<Mutex<TcpStream>>,
    pub filter: Option<Vec<ClientFilter>>,
    pub callsign: Option<String>,
    pub connect_time: Instant,
    pub packets_rx: u64,
    pub packets_tx: u64,
    pub bytes_rx: u64,
    pub bytes_tx: u64,
}

impl Client {
    pub fn new(id: usize, stream: TcpStream) -> Self {
        Self {
            _id: id,
            stream: Arc::new(Mutex::new(stream)),
            filter: None,
            callsign: None,
            connect_time: Instant::now(),
            packets_rx: 0,
            packets_tx: 0,
            bytes_rx: 0,
            bytes_tx: 0,
        }
    }
    pub fn inc_rx(&mut self, bytes: usize) {
        self.packets_rx += 1;
        self.bytes_rx += bytes as u64;
    }
    pub fn inc_tx(&mut self, bytes: usize) {
        self.packets_tx += 1;
        self.bytes_tx += bytes as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener};
    #[test]
    fn test_client_new() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let client = Client::new(1, stream);
        assert_eq!(client._id, 1);
        assert!(client.filter.is_none());
    }
} 