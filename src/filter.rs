use std::str::FromStr;
use serde::{Serialize, Deserialize};

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum ClientFilter {
    Area { lat: f64, lon: f64, radius_km: f64 },
    Box { lat1: f64, lon1: f64, lat2: f64, lon2: f64 },
    Prefix(String),
    Type(String),
    Object(String),
    All, // matches all packets
}

impl FromStr for ClientFilter {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s == "a/*" || s == "all" {
            return Ok(ClientFilter::All);
        }
        if s.starts_with("r/") {
            // r/lat/lon/radius
            let parts: Vec<&str> = s.split('/').collect();
            if parts.len() == 4 {
                let lat = parts[1].parse().map_err(|_| "Invalid latitude")?;
                let lon = parts[2].parse().map_err(|_| "Invalid longitude")?;
                let radius_km = parts[3].parse().map_err(|_| "Invalid radius")?;
                return Ok(ClientFilter::Area { lat, lon, radius_km });
            }
        }
        if s.starts_with("a/") {
            // a/lat1/lon1/lat2/lon2
            let parts: Vec<&str> = s.split('/').collect();
            if parts.len() == 5 {
                let lat1 = parts[1].parse().map_err(|_| "Invalid lat1")?;
                let lon1 = parts[2].parse().map_err(|_| "Invalid lon1")?;
                let lat2 = parts[3].parse().map_err(|_| "Invalid lat2")?;
                let lon2 = parts[4].parse().map_err(|_| "Invalid lon2")?;
                return Ok(ClientFilter::Box { lat1, lon1, lat2, lon2 });
            }
        }
        if s.starts_with("p/") {
            // p/callsignprefix
            let prefix = s[2..].to_string();
            return Ok(ClientFilter::Prefix(prefix));
        }
        if s.starts_with("t/") {
            // t/type
            let typ = s[2..].to_string();
            return Ok(ClientFilter::Type(typ));
        }
        if s.starts_with("o/") {
            // o/objectname
            let obj = s[2..].to_string();
            return Ok(ClientFilter::Object(obj));
        }
        Err("Unknown filter type".to_string())
    }
}

impl ClientFilter {
    pub fn matches(&self, packet: &str) -> bool {
        match self {
            ClientFilter::All => true,
            ClientFilter::Area { lat, lon, radius_km } => {
                if let Some((plat, plon)) = super::server::parse_aprs_lat_lon(packet) {
                    haversine_km(*lat, *lon, plat, plon) <= *radius_km
                } else {
                    false
                }
            }
            ClientFilter::Box { lat1, lon1, lat2, lon2 } => {
                if let Some((plat, plon)) = super::server::parse_aprs_lat_lon(packet) {
                    let (min_lat, max_lat) = (lat1.min(*lat2), lat1.max(*lat2));
                    let (min_lon, max_lon) = (lon1.min(*lon2), lon1.max(*lon2));
                    plat >= min_lat && plat <= max_lat && plon >= min_lon && plon <= max_lon
                } else {
                    false
                }
            }
            ClientFilter::Prefix(prefix) => {
                packet.to_uppercase().starts_with(&prefix.to_uppercase())
            }
            ClientFilter::Type(typ) => {
                // Very basic: check if packet payload starts with the type char
                if let Some(colon) = packet.find(':') {
                    let payload = &packet[colon+1..];
                    payload.starts_with(typ)
                } else {
                    false
                }
            }
            ClientFilter::Object(obj) => {
                // Check if object name is in the packet (very basic)
                packet.contains(obj)
            }
        }
    }
}

pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0; // Earth radius in km
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_area_filter_parse() {
        let f: AreaFilter = "r/60.0/25.0/100.0".parse().unwrap();
        assert_eq!(f.lat, 60.0);
        assert_eq!(f.lon, 25.0);
        assert_eq!(f.radius_km, 100.0);
        assert!("r/60.0/25.0".parse::<AreaFilter>().is_err());
        assert!("x/60.0/25.0/100.0".parse::<AreaFilter>().is_err());
    }
    #[test]
    fn test_area_filter_match() {
        let area: AreaFilter = "r/60.0/25.0/100.0".parse().unwrap();
        assert!(area_filter_match(&area, 60.0, 25.0)); // center
        assert!(area_filter_match(&area, 60.5, 25.0)); // within 100km
        assert!(!area_filter_match(&area, 62.0, 25.0)); // outside 100km
    }
} 