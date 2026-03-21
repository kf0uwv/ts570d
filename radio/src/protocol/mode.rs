use std::fmt;

use crate::error::RadioError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Mode {
    Lsb = 1,
    Usb = 2,
    Cw = 3,
    Fm = 4,
    Am = 5,
    Fsk = 6,
    CwReverse = 7,
    FskReverse = 9,
}

impl Mode {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Lsb => "LSB",
            Mode::Usb => "USB",
            Mode::Cw => "CW",
            Mode::Fm => "FM",
            Mode::Am => "AM",
            Mode::Fsk => "FSK",
            Mode::CwReverse => "CW-R",
            Mode::FskReverse => "FSK-R",
        }
    }
}

impl TryFrom<u8> for Mode {
    type Error = RadioError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Mode::Lsb),
            2 => Ok(Mode::Usb),
            3 => Ok(Mode::Cw),
            4 => Ok(Mode::Fm),
            5 => Ok(Mode::Am),
            6 => Ok(Mode::Fsk),
            7 => Ok(Mode::CwReverse),
            9 => Ok(Mode::FskReverse),
            _ => Err(RadioError::InvalidMode(value)),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_valid_modes() {
        assert_eq!(Mode::try_from(1).unwrap(), Mode::Lsb);
        assert_eq!(Mode::try_from(2).unwrap(), Mode::Usb);
        assert_eq!(Mode::try_from(3).unwrap(), Mode::Cw);
        assert_eq!(Mode::try_from(4).unwrap(), Mode::Fm);
        assert_eq!(Mode::try_from(5).unwrap(), Mode::Am);
        assert_eq!(Mode::try_from(6).unwrap(), Mode::Fsk);
        assert_eq!(Mode::try_from(7).unwrap(), Mode::CwReverse);
        assert_eq!(Mode::try_from(9).unwrap(), Mode::FskReverse);
    }

    #[test]
    fn test_invalid_mode_zero() {
        let result = Mode::try_from(0u8);
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::InvalidMode(v) => assert_eq!(v, 0),
            _ => panic!("expected InvalidMode"),
        }
    }

    #[test]
    fn test_invalid_mode_eight() {
        let result = Mode::try_from(8u8);
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::InvalidMode(v) => assert_eq!(v, 8),
            _ => panic!("expected InvalidMode"),
        }
    }

    #[test]
    fn test_invalid_mode_ten() {
        let result = Mode::try_from(10u8);
        assert!(result.is_err());
        match result.unwrap_err() {
            RadioError::InvalidMode(v) => assert_eq!(v, 10),
            _ => panic!("expected InvalidMode"),
        }
    }

    #[test]
    fn test_display() {
        assert_eq!(Mode::Lsb.to_string(), "LSB");
        assert_eq!(Mode::Usb.to_string(), "USB");
        assert_eq!(Mode::Cw.to_string(), "CW");
        assert_eq!(Mode::Fm.to_string(), "FM");
        assert_eq!(Mode::Am.to_string(), "AM");
        assert_eq!(Mode::Fsk.to_string(), "FSK");
        assert_eq!(Mode::CwReverse.to_string(), "CW-R");
        assert_eq!(Mode::FskReverse.to_string(), "FSK-R");
    }

    #[test]
    fn test_round_trip() {
        let modes = [
            Mode::Lsb,
            Mode::Usb,
            Mode::Cw,
            Mode::Fm,
            Mode::Am,
            Mode::Fsk,
            Mode::CwReverse,
            Mode::FskReverse,
        ];
        for mode in modes {
            let byte = mode.as_u8();
            let recovered = Mode::try_from(byte).expect("round-trip should succeed");
            assert_eq!(recovered, mode);
        }
    }

    #[test]
    fn test_as_u8_values() {
        assert_eq!(Mode::Lsb.as_u8(), 1);
        assert_eq!(Mode::Usb.as_u8(), 2);
        assert_eq!(Mode::Cw.as_u8(), 3);
        assert_eq!(Mode::Fm.as_u8(), 4);
        assert_eq!(Mode::Am.as_u8(), 5);
        assert_eq!(Mode::Fsk.as_u8(), 6);
        assert_eq!(Mode::CwReverse.as_u8(), 7);
        assert_eq!(Mode::FskReverse.as_u8(), 9);
    }
}
