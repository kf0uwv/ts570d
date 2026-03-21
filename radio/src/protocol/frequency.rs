use std::fmt;

use crate::error::RadioError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frequency(u64);

impl Frequency {
    pub const MIN_HZ: u64 = 500_000;
    pub const MAX_HZ: u64 = 60_000_000;

    pub fn new(hz: u64) -> Result<Self, RadioError> {
        if !(Self::MIN_HZ..=Self::MAX_HZ).contains(&hz) {
            return Err(RadioError::FrequencyOutOfRange(hz));
        }
        Ok(Frequency(hz))
    }

    pub fn hz(self) -> u64 {
        self.0
    }

    /// Format as 11-digit zero-padded protocol string e.g. "00014230000"
    pub fn to_protocol_string(self) -> String {
        format!("{:011}", self.0)
    }

    /// Parse from 11-digit protocol string
    pub fn from_protocol_str(s: &str) -> Result<Self, RadioError> {
        let hz = s
            .parse::<u64>()
            .map_err(|_| RadioError::InvalidProtocolString(s.to_string()))?;
        Self::new(hz)
    }
}

impl fmt::Display for Frequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mhz = self.0 as f64 / 1_000_000.0;
        write!(f, "{:.3} MHz", mhz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_frequency() {
        let freq = Frequency::new(14_230_000).unwrap();
        assert_eq!(freq.hz(), 14_230_000);
    }

    #[test]
    fn test_boundary_min() {
        let freq = Frequency::new(Frequency::MIN_HZ).unwrap();
        assert_eq!(freq.hz(), 500_000);
    }

    #[test]
    fn test_boundary_max() {
        let freq = Frequency::new(Frequency::MAX_HZ).unwrap();
        assert_eq!(freq.hz(), 60_000_000);
    }

    #[test]
    fn test_out_of_range_below() {
        let result = Frequency::new(499_999);
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::FrequencyOutOfRange(v) => assert_eq!(v, 499_999),
            _ => panic!("expected FrequencyOutOfRange"),
        }
    }

    #[test]
    fn test_out_of_range_above() {
        let result = Frequency::new(60_000_001);
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::FrequencyOutOfRange(v) => assert_eq!(v, 60_000_001),
            _ => panic!("expected FrequencyOutOfRange"),
        }
    }

    #[test]
    fn test_out_of_range_zero() {
        let result = Frequency::new(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_protocol_string_format() {
        let freq = Frequency::new(14_230_000).unwrap();
        assert_eq!(freq.to_protocol_string(), "00014230000");
    }

    #[test]
    fn test_protocol_string_min() {
        let freq = Frequency::new(500_000).unwrap();
        assert_eq!(freq.to_protocol_string(), "00000500000");
    }

    #[test]
    fn test_protocol_string_max() {
        let freq = Frequency::new(60_000_000).unwrap();
        assert_eq!(freq.to_protocol_string(), "00060000000");
    }

    #[test]
    fn test_from_protocol_str_round_trip() {
        let freq = Frequency::new(14_230_000).unwrap();
        let s = freq.to_protocol_string();
        let recovered = Frequency::from_protocol_str(&s).unwrap();
        assert_eq!(recovered, freq);
    }

    #[test]
    fn test_from_protocol_str_invalid() {
        let result = Frequency::from_protocol_str("not_a_number");
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::InvalidProtocolString(_) => {}
            _ => panic!("expected InvalidProtocolString"),
        }
    }

    #[test]
    fn test_from_protocol_str_out_of_range() {
        // A valid number but out of range
        let result = Frequency::from_protocol_str("00000000001");
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::FrequencyOutOfRange(_) => {}
            _ => panic!("expected FrequencyOutOfRange"),
        }
    }

    #[test]
    fn test_display_format() {
        let freq = Frequency::new(14_230_000).unwrap();
        assert_eq!(freq.to_string(), "14.230 MHz");
    }

    #[test]
    fn test_display_format_min() {
        let freq = Frequency::new(500_000).unwrap();
        assert_eq!(freq.to_string(), "0.500 MHz");
    }

    #[test]
    fn test_ordering() {
        let low = Frequency::new(7_000_000).unwrap();
        let high = Frequency::new(14_000_000).unwrap();
        assert!(low < high);
        assert!(high > low);
        assert_eq!(low, low);
    }
}
