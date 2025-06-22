use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct UplinkConfig {
    pub host: String,
    pub port: u16,
    pub callsign: String,
    pub passcode: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct S2SPeerConfig {
    pub host: String,
    pub port: u16,
    pub passcode: u16,
    pub peer_name: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Config {
    pub server_name: String,
    pub user_port: u16,
    pub server_port: u16,
    pub s2s_port: Option<u16>,
    pub _allow_callsigns: Option<Vec<String>>,
    pub _deny_callsigns: Option<Vec<String>>,
    pub uplink: Option<UplinkConfig>,
    pub s2s_peers: Option<Vec<S2SPeerConfig>>,
}

impl Config {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        toml::from_str(&content).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_load_from_file() {
        let toml = r#"
            server_name = "test-server"
            user_port = 1234
            server_port = 5678
            allow_callsigns = ["N0CALL"]
            deny_callsigns = ["BADGUY"]
            [uplink]
            host = "rotate.aprs2.net"
            port = 14580
            callsign = "N0CALL"
            passcode = 12345
        "#;
        let path = "test_config.toml";
        fs::write(path, toml).unwrap();
        let cfg = Config::load_from_file(path).unwrap();
        assert_eq!(cfg.server_name, "test-server");
        assert_eq!(cfg.user_port, 1234);
        assert_eq!(cfg.server_port, 5678);
        assert_eq!(cfg._allow_callsigns.as_ref().unwrap()[0], "N0CALL");
        assert_eq!(cfg._deny_callsigns.as_ref().unwrap()[0], "BADGUY");
        let uplink = cfg.uplink.as_ref().unwrap();
        assert_eq!(uplink.host, "rotate.aprs2.net");
        assert_eq!(uplink.port, 14580);
        assert_eq!(uplink.callsign, "N0CALL");
        assert_eq!(uplink.passcode, 12345);
        let _ = fs::remove_file(path);
    }
} 