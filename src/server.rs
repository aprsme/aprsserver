use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::collections::{HashSet, VecDeque};
use std::time::{Instant};
use std::sync::{Arc, Mutex};
use crate::filter::ClientFilter;
use crate::client::Client;
use crate::hub::Hub;

const DUP_CACHE_SIZE: usize = 100;

fn aprs_passcode(callsign: &str) -> u16 {
    // Standard APRS-IS passcode algorithm (from aprsc/javAPRSSrvr)
    let mut hash: u32 = 0x73e2_070a;
    let mut up = callsign.to_uppercase();
    if let Some(idx) = up.find('-') {
        up.truncate(idx);
    }
    for (i, c) in up.chars().enumerate() {
        let c = c as u8;
        if i & 1 == 0 {
            hash ^= (c as u32) << 8;
        } else {
            hash ^= c as u32;
        }
    }
    (hash & 0x7fff) as u16
}

pub fn is_valid_aprs_packet(line: &str) -> bool {
    // Basic APRS-IS packet validation: must contain '>' and ':'
    // Example: CALLSIGN>DEST,PATH:payload
    let line = line.trim();
    if line.is_empty() { return false; }
    let gt = line.find('>');
    let colon = line.find(':');
    match (gt, colon) {
        (Some(gt), Some(colon)) if gt > 0 && colon > gt + 1 => true,
        _ => false,
    }
}

fn _packet_matches_filter(line: &str, filter: &Option<Vec<String>>) -> bool {
    match filter {
        Some(keywords) => {
            let l = line.to_lowercase();
            keywords.iter().any(|k| l.contains(k))
        }
        None => true, // No filter set, allow all
    }
}

fn extract_message_destination(packet: &str) -> Option<String> {
    // APRS message format: SRC>DEST,PATH::DEST     :message text
    // Message payload: :DEST     :message text
    let colon = packet.find(':')?;
    let payload = &packet[colon+1..];
    if !payload.starts_with(':') || payload.len() < 10 {
        return None;
    }
    let dest = &payload[1..10];
    let dest = dest.trim();
    if dest.is_empty() || !dest.chars().all(|c| c.is_ascii_alphanumeric()) {
        None
    } else {
        Some(dest.to_string())
    }
}

pub fn parse_aprs_lat_lon(packet: &str) -> Option<(f64, f64)> {
    // Very basic APRS position parser: looks for DDMM.hhN/DDDMM.hhE or similar
    // Example: "N0CALL>APRS,TCPIP*:!4903.50N/07201.75W>..."
    let payload_start = packet.find(':')? + 1;
    let payload = &packet[payload_start..];
    let pos = payload.find('!').or_else(|| payload.find('='))?;
    let data = &payload[pos+1..];
    if data.len() < 19 { return None; }
    // Parse latitude
    let lat_str = &data[0..8];
    let ns = &data[7..8];
    let lon_str = &data[9..18];
    let ew = &data[17..18];
    let lat_deg: f64 = lat_str[0..2].parse().ok()?;
    let lat_min: f64 = lat_str[2..7].parse().ok()?;
    let mut lat = lat_deg + lat_min / 60.0;
    if ns == "S" { lat = -lat; }
    // Parse longitude
    let lon_deg: f64 = lon_str[0..3].parse().ok()?;
    let lon_min: f64 = lon_str[3..8].parse().ok()?;
    let mut lon = lon_deg + lon_min / 60.0;
    if ew == "W" { lon = -lon; }
    Some((lat, lon))
}

pub fn handle_client(mut stream: TcpStream, hub: Arc<Mutex<Hub>>) {
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());
    println!("New connection from {}", peer);

    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    let mut filters: Option<Vec<ClientFilter>> = None;
    let callsign: Option<String> = None;
    let mut dup_cache: HashSet<u64> = HashSet::new();
    let mut dup_order: VecDeque<u64> = VecDeque::new();
    let start_time = Instant::now();
    let mut packets_received = 0u64;
    let mut packets_dropped = 0u64;
    let mut packets_duplicated = 0u64;

    // Register client in hub
    let mut hub_lock = hub.lock().unwrap();
    let id = hub_lock.next_id;
    let client = Client::new(id, stream.try_clone().unwrap());
    hub_lock.add_client(client);
    drop(hub_lock);

    // Wait for login line
    match reader.read_line(&mut line) {
        Ok(0) => {
            println!("{} disconnected before login", peer);
            return;
        }
        Ok(_) => {
            // Example login: user CALLSIGN pass 12345 vers ...
            let login = line.trim();
            let mut callsign: Option<String> = None;
            let mut passcode: Option<&str> = None;
            let mut parts = login.split_whitespace();
            while let Some(part) = parts.next() {
                if part.eq_ignore_ascii_case("user") {
                    callsign = parts.next().map(|s| s.to_string());
                } else if part.eq_ignore_ascii_case("pass") {
                    passcode = parts.next();
                }
            }
            if let (Some(ref callsign), Some(passcode)) = (callsign.as_ref(), passcode) {
                if let Ok(passcode_num) = passcode.parse::<u16>() {
                    if aprs_passcode(callsign) == passcode_num {
                        println!("{} logged in: {}", peer, login);
                        let _ = stream.write_all(b"# login ok\n");
                    } else {
                        let _ = stream.write_all(b"# invalid passcode\n");
                        return;
                    }
                } else {
                    let _ = stream.write_all(b"# invalid passcode\n");
                    return;
                }
            } else {
                let _ = stream.write_all(b"# invalid login\n");
                return;
            }
        }
        Err(e) => {
            eprintln!("{} error reading login: {}", peer, e);
            return;
        }
    }

    // Main loop: handle filter commands and packets
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                println!("{} disconnected", peer);
                break;
            }
            Ok(n) => {
                let trimmed = line.trim();
                if trimmed.to_lowercase().starts_with("# filter ") {
                    // Parse filter command(s)
                    let filter_str = &trimmed[8..].trim();
                    let mut new_filters = Vec::new();
                    for part in filter_str.split_whitespace() {
                        match part.parse::<ClientFilter>() {
                            Ok(f) => new_filters.push(f),
                            Err(e) => {
                                let _ = stream.write_all(format!("# invalid filter: {}\n", e).as_bytes());
                            }
                        }
                    }
                    if !new_filters.is_empty() {
                        filters = Some(new_filters);
                        let _ = stream.write_all(b"# filter set\n");
                        println!("{} set filter: {}", peer, filter_str);
                    }
                    continue;
                } else if trimmed.to_lowercase() == "# stats" {
                    let uptime = start_time.elapsed().as_secs();
                    let stats = format!(
                        "# stats: uptime={}s received={} dropped={} duplicated={}\n",
                        uptime, packets_received, packets_dropped, packets_duplicated
                    );
                    let _ = stream.write_all(stats.as_bytes());
                    continue;
                }
                packets_received += 1;
                // Increment per-client RX stats
                if let Some(client) = hub.lock().unwrap().clients.get(&id) {
                    let mut c = client.lock().unwrap();
                    c.inc_rx(n);
                }
                // Duplicate detection
                let hash = seahash::hash(trimmed.as_bytes());
                if dup_cache.contains(&hash) {
                    packets_duplicated += 1;
                    continue;
                }
                dup_cache.insert(hash);
                dup_order.push_back(hash);
                if dup_order.len() > DUP_CACHE_SIZE {
                    if let Some(old) = dup_order.pop_front() {
                        dup_cache.remove(&old);
                    }
                }
                // Filtering
                let mut pass = true;
                if let Some(ref fs) = filters {
                    pass = fs.iter().any(|f| f.matches(trimmed));
                }
                if pass {
                    // Broadcast to all other clients and increment their TX stats
                    let hub_lock = hub.lock().unwrap();
                    for (other_id, client) in &hub_lock.clients {
                        if *other_id != id {
                            let mut c = client.lock().unwrap();
                            c.inc_tx(n);
                        }
                    }
                    hub_lock.broadcast_packet(id, line.as_str());
                    drop(hub_lock);
                } else {
                    packets_dropped += 1;
                }
                // Message routing placeholder
                if let Some(dest) = extract_message_destination(trimmed) {
                    println!("Message packet for destination: {}", dest);
                    // Future: route to correct client
                }
                // On filter or login, update client in hub with new filter/callsign
                let mut hub_lock = hub.lock().unwrap();
                hub_lock.update_client(id, callsign.clone(), filters.clone());
                drop(hub_lock);
            }
            Err(e) => {
                eprintln!("{} error reading: {}", peer, e);
                break;
            }
        }
    }

    // Remove client from hub on disconnect
    let mut hub_lock = hub.lock().unwrap();
    hub_lock.remove_client(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aprs_passcode() {
        // SSID ignored
        assert_eq!(aprs_passcode("N0CALL"), aprs_passcode("N0CALL-1"));
        // Case-insensitive
        assert_eq!(aprs_passcode("TEST"), aprs_passcode("test"));
        // Different callsigns yield different passcodes
        assert_ne!(aprs_passcode("N0CALL"), aprs_passcode("N1CALL"));
    }

    #[test]
    fn test_is_valid_aprs_packet() {
        assert!(is_valid_aprs_packet("N0CALL>APRS,TCPIP*:payload"));
        assert!(is_valid_aprs_packet("CALL>DEST:msg"));
        assert!(!is_valid_aprs_packet("") );
        assert!(!is_valid_aprs_packet("N0CALL payload"));
        assert!(!is_valid_aprs_packet(":no source address"));
    }

    #[test]
    fn test_packet_matches_filter() {
        let filter = Some(vec!["foo".to_string(), "bar".to_string()]);
        assert!(packet_matches_filter("this is foo", &filter));
        assert!(packet_matches_filter("BAR test", &filter));
        assert!(!packet_matches_filter("baz", &filter));
        assert!(packet_matches_filter("anything", &None));
    }

    #[test]
    fn test_extract_message_destination() {
        assert_eq!(extract_message_destination("N0CALL>APRS,TCPIP*::DEST     :Hello"), Some("DEST".to_string()));
        assert_eq!(extract_message_destination("N0CALL>APRS,TCPIP*::FOO      :Test msg"), Some("FOO".to_string()));
        assert_eq!(extract_message_destination("N0CALL>APRS,TCPIP*:payload"), None);
        assert_eq!(extract_message_destination("N0CALL>APRS,TCPIP*::   :No dest"), None);
    }

    #[test]
    fn test_parse_aprs_lat_lon() {
        let pkt = "N0CALL>APRS,TCPIP*:!4903.50N/07201.75W>Test";
        let (lat, lon) = parse_aprs_lat_lon(pkt).unwrap();
        assert!((lat - 49.0583).abs() < 0.01);
        assert!((lon + 72.0291).abs() < 0.01);
    }
} 