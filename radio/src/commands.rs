//! TS-570D CAT Command Table and Metadata
//!
//! This module provides a comprehensive, data-driven command table for the
//! Kenwood TS-570D CAT protocol. Each command is defined with rich metadata
//! that describes its capabilities, parameter format, and response format.
//!
//! # Design Philosophy
//!
//! The command table is the single source of truth for all TS-570D commands.
//! By centralizing command metadata, we can:
//! - Validate commands before sending them
//! - Parse responses based on command type
//! - Generate protocol documentation
//! - Implement type-safe command builders
//!
//! # Example
//!
//! ```
//! use radio::commands::CommandMetadata;
//!
//! // Look up command metadata
//! let fa = CommandMetadata::find("FA").expect("FA command should exist");
//! assert!(fa.supports_read);
//! assert!(fa.supports_write);
//! assert_eq!(fa.description, "VFO A Frequency");
//! ```

/// Parameter format specification for TS-570D commands
///
/// Defines how command parameters should be formatted and validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamFormat {
    /// No parameters (e.g., TX, RX)
    None,

    /// 11-digit frequency in Hz (e.g., "00014250000" for 14.250 MHz)
    Frequency11Digit,

    /// Single digit mode selector (1-9)
    /// 1=LSB, 2=USB, 3=CW, 4=FM, 5=AM, 6=FSK, 7=CW-R, 8=N/A, 9=FSK-R
    Mode,

    /// Single digit VFO selector (0-2)
    /// 0=VFO A, 1=VFO B, 2=Memory
    VfoSelect,

    /// 3-digit numeric level (000-255 or 000-100 depending on command)
    Level3Digit,

    /// 4-digit numeric value (0000-9999)
    Value4Digit,

    /// 5-digit numeric value (00000-99999)
    Value5Digit,

    /// 2-digit memory channel (00-99)
    MemoryChannel,

    /// Single digit on/off or enable/disable (0=off, 1=on)
    OnOff,

    /// Single digit selection from enumerated options
    SingleDigit,

    /// 2-digit selection from enumerated options
    TwoDigit,

    /// CTCSS tone number (00-42)
    CtcssTone,

    /// Text string (variable length, command-specific)
    Text,

    /// Composite: Multiple fields packed together
    /// Used for complex commands like IF, EX
    Composite,
}

/// Response format specification for TS-570D commands
///
/// Defines the expected response format from the radio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseFormat {
    /// Command echo with no additional data (e.g., TX;)
    Echo,

    /// 11-digit frequency in Hz
    Frequency11Digit,

    /// Single digit mode code
    Mode,

    /// Single digit VFO selection
    VfoSelect,

    /// 3-digit numeric level
    Level3Digit,

    /// 4-digit numeric value
    Value4Digit,

    /// 5-digit numeric value
    Value5Digit,

    /// 2-digit memory channel
    MemoryChannel,

    /// Single digit on/off state
    OnOff,

    /// Single digit selection
    SingleDigit,

    /// 2-digit selection
    TwoDigit,

    /// CTCSS tone number
    CtcssTone,

    /// Text string response
    Text,

    /// IF command response: composite data with multiple fields
    /// Format: IFxxxxxxxxx rrrrrrrrrrr mmm vvvvv rrr fff tt ss gggg;
    /// - xxxxxxxxx: N/A
    /// - rrrrrrrrrrr: VFO A frequency (11 digits)
    /// - mmm: 5 spaces
    /// - vvvvv: clarifier offset/RIT (5 digits)
    /// - rrr: RIT on/off, XIT on/off, channel number
    /// - fff: memory channel
    /// - tt: TX/RX status
    /// - ss: mode
    /// - gggg: VFO/Memory/Channel scan
    IfComposite,

    /// EX command response: menu parameter value
    ExComposite,

    /// SM/RM command response: S-meter or signal level
    SignalLevel,
}

/// Metadata for a single TS-570D CAT command
///
/// This structure contains all information needed to send and receive
/// a specific command, including its capabilities, parameter format,
/// and expected response format.
#[derive(Debug, Clone, Copy)]
pub struct CommandMetadata {
    /// 2-3 letter command code (e.g., "FA", "MD", "TX")
    pub code: &'static str,

    /// Whether this command supports read (query) operations
    /// Query format: `<CMD>;` (e.g., `FA;`)
    pub supports_read: bool,

    /// Whether this command supports write (set) operations
    /// Set format: `<CMD><params>;` (e.g., `FA00014250000;`)
    pub supports_write: bool,

    /// Parameter format specification for write operations
    pub param_format: ParamFormat,

    /// Response format specification for read operations
    pub response_format: ResponseFormat,

    /// Human-readable description of the command
    pub description: &'static str,
}

impl CommandMetadata {
    /// Find command metadata by command code
    ///
    /// # Arguments
    ///
    /// * `code` - The 2-3 letter command code (case-sensitive)
    ///
    /// # Returns
    ///
    /// `Some(&CommandMetadata)` if found, `None` otherwise
    ///
    /// # Example
    ///
    /// ```
    /// use radio::commands::CommandMetadata;
    ///
    /// let fa = CommandMetadata::find("FA").unwrap();
    /// assert_eq!(fa.code, "FA");
    /// ```
    pub fn find(code: &str) -> Option<&'static CommandMetadata> {
        COMMAND_TABLE.iter().find(|cmd| cmd.code == code)
    }

    /// Check if this command supports query (read) operations
    ///
    /// # Returns
    ///
    /// `true` if the command can be queried with `<CMD>;` format
    pub fn is_query_command(&self) -> bool {
        self.supports_read
    }

    /// Check if this command supports set (write) operations
    ///
    /// # Returns
    ///
    /// `true` if the command can be sent with parameters
    pub fn is_set_command(&self) -> bool {
        self.supports_write
    }

    /// Check if this command is read-only (query only)
    pub fn is_read_only(&self) -> bool {
        self.supports_read && !self.supports_write
    }

    /// Check if this command is write-only (set only)
    pub fn is_write_only(&self) -> bool {
        self.supports_write && !self.supports_read
    }

    /// Check if this command has no parameters (action commands)
    pub fn is_parameterless(&self) -> bool {
        matches!(self.param_format, ParamFormat::None)
    }
}

/// Complete TS-570D CAT command table
///
/// This table includes all documented commands from the TS-570D manual.
/// Commands are organized by functional category for easier navigation.
pub static COMMAND_TABLE: &[CommandMetadata] = &[
    // ===================================================================
    // FREQUENCY COMMANDS
    // ===================================================================
    CommandMetadata {
        code: "FA",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Frequency11Digit,
        response_format: ResponseFormat::Frequency11Digit,
        description: "VFO A Frequency",
    },
    CommandMetadata {
        code: "FB",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Frequency11Digit,
        response_format: ResponseFormat::Frequency11Digit,
        description: "VFO B Frequency",
    },
    CommandMetadata {
        code: "FC",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Frequency11Digit,
        response_format: ResponseFormat::Frequency11Digit,
        description: "Sub-receiver VFO Frequency",
    },
    // ===================================================================
    // MODE COMMANDS
    // ===================================================================
    CommandMetadata {
        code: "MD",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Mode,
        response_format: ResponseFormat::Mode,
        description: "Operating Mode",
    },
    // ===================================================================
    // VFO AND MEMORY COMMANDS
    // ===================================================================
    CommandMetadata {
        code: "FR",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::VfoSelect,
        response_format: ResponseFormat::VfoSelect,
        description: "Receiver VFO/Memory Selection",
    },
    CommandMetadata {
        code: "FT",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::VfoSelect,
        response_format: ResponseFormat::VfoSelect,
        description: "Transmitter VFO/Memory Selection",
    },
    CommandMetadata {
        code: "FN",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::SingleDigit,
        response_format: ResponseFormat::SingleDigit,
        description: "VFO A/B Selection",
    },
    // ===================================================================
    // MEMORY COMMANDS
    // ===================================================================
    CommandMetadata {
        code: "MC",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::MemoryChannel,
        response_format: ResponseFormat::MemoryChannel,
        description: "Memory Channel Number",
    },
    CommandMetadata {
        code: "MR",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Composite,
        response_format: ResponseFormat::Text,
        description: "Memory Read",
    },
    CommandMetadata {
        code: "MW",
        supports_read: false,
        supports_write: true,
        param_format: ParamFormat::Composite,
        response_format: ResponseFormat::Echo,
        description: "Memory Write",
    },
    // ===================================================================
    // AUDIO AND RF GAIN COMMANDS
    // ===================================================================
    CommandMetadata {
        code: "AG",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Level3Digit,
        response_format: ResponseFormat::Level3Digit,
        description: "AF Gain",
    },
    CommandMetadata {
        code: "RG",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Level3Digit,
        response_format: ResponseFormat::Level3Digit,
        description: "RF Gain",
    },
    CommandMetadata {
        code: "MG",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Level3Digit,
        response_format: ResponseFormat::Level3Digit,
        description: "Microphone Gain",
    },
    CommandMetadata {
        code: "PC",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Level3Digit,
        response_format: ResponseFormat::Level3Digit,
        description: "Power Control (Transmit Power)",
    },
    CommandMetadata {
        code: "SQ",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Level3Digit,
        response_format: ResponseFormat::Level3Digit,
        description: "Squelch Level",
    },
    CommandMetadata {
        code: "VG",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Level3Digit,
        response_format: ResponseFormat::Level3Digit,
        description: "VOX Gain",
    },
    // ===================================================================
    // FILTER COMMANDS
    // ===================================================================
    CommandMetadata {
        code: "FW",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Value4Digit,
        response_format: ResponseFormat::Value4Digit,
        description: "Filter Width",
    },
    CommandMetadata {
        code: "SH",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::TwoDigit,
        response_format: ResponseFormat::TwoDigit,
        description: "Filter High Frequency",
    },
    CommandMetadata {
        code: "SL",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::TwoDigit,
        response_format: ResponseFormat::TwoDigit,
        description: "Filter Low Frequency",
    },
    // ===================================================================
    // TRANSMIT/RECEIVE CONTROL
    // ===================================================================
    CommandMetadata {
        code: "TX",
        supports_read: false,
        supports_write: true,
        param_format: ParamFormat::None,
        response_format: ResponseFormat::Echo,
        description: "Transmit Mode (PTT On)",
    },
    CommandMetadata {
        code: "RX",
        supports_read: false,
        supports_write: true,
        param_format: ParamFormat::None,
        response_format: ResponseFormat::Echo,
        description: "Receive Mode (PTT Off)",
    },
    // ===================================================================
    // TUNING COMMANDS
    // ===================================================================
    CommandMetadata {
        code: "GT",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::SingleDigit,
        response_format: ResponseFormat::SingleDigit,
        description: "AGC Time Constant",
    },
    CommandMetadata {
        code: "RA",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Value4Digit,
        response_format: ResponseFormat::Value4Digit,
        description: "Attenuator",
    },
    CommandMetadata {
        code: "PA",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::SingleDigit,
        response_format: ResponseFormat::SingleDigit,
        description: "Pre-amplifier",
    },
    // ===================================================================
    // NOISE REDUCTION AND BLANKER
    // ===================================================================
    CommandMetadata {
        code: "NB",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "Noise Blanker",
    },
    CommandMetadata {
        code: "NR",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "Noise Reduction",
    },
    CommandMetadata {
        code: "NL",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Level3Digit,
        response_format: ResponseFormat::Level3Digit,
        description: "Noise Reduction Level",
    },
    // ===================================================================
    // CLARIFIER AND RIT/XIT
    // ===================================================================
    CommandMetadata {
        code: "RT",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "RIT On/Off",
    },
    CommandMetadata {
        code: "XT",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "XIT On/Off",
    },
    CommandMetadata {
        code: "RC",
        supports_read: false,
        supports_write: true,
        param_format: ParamFormat::None,
        response_format: ResponseFormat::Echo,
        description: "RIT/XIT Frequency Clear",
    },
    CommandMetadata {
        code: "RD",
        supports_read: false,
        supports_write: true,
        param_format: ParamFormat::Value5Digit,
        response_format: ResponseFormat::Echo,
        description: "RIT/XIT Frequency Down",
    },
    CommandMetadata {
        code: "RU",
        supports_read: false,
        supports_write: true,
        param_format: ParamFormat::Value5Digit,
        response_format: ResponseFormat::Echo,
        description: "RIT/XIT Frequency Up",
    },
    // ===================================================================
    // SCAN COMMANDS
    // ===================================================================
    CommandMetadata {
        code: "SC",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "Scan Mode",
    },
    CommandMetadata {
        code: "ST",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::SingleDigit,
        response_format: ResponseFormat::SingleDigit,
        description: "Scan Type",
    },
    // ===================================================================
    // SPLIT OPERATION
    // ===================================================================
    CommandMetadata {
        code: "SP",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "Split Operation",
    },
    // ===================================================================
    // OFFSET AND REPEATER
    // ===================================================================
    CommandMetadata {
        code: "OS",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "Offset",
    },
    // ===================================================================
    // CTCSS AND TONE
    // ===================================================================
    CommandMetadata {
        code: "CT",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "CTCSS On/Off",
    },
    CommandMetadata {
        code: "CN",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::CtcssTone,
        response_format: ResponseFormat::CtcssTone,
        description: "CTCSS Tone Frequency",
    },
    CommandMetadata {
        code: "TO",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "Tone Frequency On/Off",
    },
    CommandMetadata {
        code: "TN",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::CtcssTone,
        response_format: ResponseFormat::CtcssTone,
        description: "Tone Frequency",
    },
    // ===================================================================
    // INFORMATION AND STATUS COMMANDS
    // ===================================================================
    CommandMetadata {
        code: "IF",
        supports_read: true,
        supports_write: false,
        param_format: ParamFormat::None,
        response_format: ResponseFormat::IfComposite,
        description: "Transceiver Information",
    },
    CommandMetadata {
        code: "ID",
        supports_read: true,
        supports_write: false,
        param_format: ParamFormat::None,
        response_format: ResponseFormat::TwoDigit,
        description: "Transceiver ID (Returns 019 for TS-570D)",
    },
    CommandMetadata {
        code: "AI",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "Auto Information Mode",
    },
    CommandMetadata {
        code: "PS",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "Power Status",
    },
    // ===================================================================
    // METER COMMANDS
    // ===================================================================
    CommandMetadata {
        code: "SM",
        supports_read: true,
        supports_write: false,
        param_format: ParamFormat::None,
        response_format: ResponseFormat::SignalLevel,
        description: "S-Meter Reading",
    },
    CommandMetadata {
        code: "RM",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::SingleDigit,
        response_format: ResponseFormat::SignalLevel,
        description: "Meter Function Selection",
    },
    // ===================================================================
    // LOCK COMMANDS
    // ===================================================================
    CommandMetadata {
        code: "LK",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "Lock",
    },
    // ===================================================================
    // VFO OPERATIONS
    // ===================================================================
    CommandMetadata {
        code: "UP",
        supports_read: false,
        supports_write: true,
        param_format: ParamFormat::None,
        response_format: ResponseFormat::Echo,
        description: "Frequency Up (VFO)",
    },
    CommandMetadata {
        code: "DN",
        supports_read: false,
        supports_write: true,
        param_format: ParamFormat::None,
        response_format: ResponseFormat::Echo,
        description: "Frequency Down (VFO)",
    },
    // ===================================================================
    // BAND SELECT
    // ===================================================================
    CommandMetadata {
        code: "BY",
        supports_read: false,
        supports_write: true,
        param_format: ParamFormat::TwoDigit,
        response_format: ResponseFormat::Echo,
        description: "Band Select",
    },
    // ===================================================================
    // VOICE AND KEYER
    // ===================================================================
    CommandMetadata {
        code: "VX",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "VOX On/Off",
    },
    CommandMetadata {
        code: "VD",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Value4Digit,
        response_format: ResponseFormat::Value4Digit,
        description: "VOX Delay",
    },
    CommandMetadata {
        code: "KS",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Level3Digit,
        response_format: ResponseFormat::Level3Digit,
        description: "Keying Speed",
    },
    CommandMetadata {
        code: "KY",
        supports_read: false,
        supports_write: true,
        param_format: ParamFormat::Text,
        response_format: ResponseFormat::Echo,
        description: "CW Keying",
    },
    CommandMetadata {
        code: "BK",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "Break-in On/Off",
    },
    // ===================================================================
    // EXTENDED MENU
    // ===================================================================
    CommandMetadata {
        code: "EX",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Composite,
        response_format: ResponseFormat::ExComposite,
        description: "Extended Menu",
    },
    // ===================================================================
    // QUICK MEMORY
    // ===================================================================
    CommandMetadata {
        code: "QR",
        supports_read: false,
        supports_write: true,
        param_format: ParamFormat::None,
        response_format: ResponseFormat::Echo,
        description: "Quick Memory Store",
    },
    CommandMetadata {
        code: "MF",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::OnOff,
        response_format: ResponseFormat::OnOff,
        description: "Memory Function",
    },
    // ===================================================================
    // ANTENNA SELECT
    // ===================================================================
    CommandMetadata {
        code: "AC",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::SingleDigit,
        response_format: ResponseFormat::SingleDigit,
        description: "Antenna Selection",
    },
    // ===================================================================
    // DISPLAY AND USER INTERFACE
    // ===================================================================
    CommandMetadata {
        code: "IS",
        supports_read: true,
        supports_write: true,
        param_format: ParamFormat::Value4Digit,
        response_format: ResponseFormat::Value4Digit,
        description: "IF Shift",
    },
    // ===================================================================
    // FIRMWARE AND SYSTEM
    // ===================================================================
    CommandMetadata {
        code: "FV",
        supports_read: true,
        supports_write: false,
        param_format: ParamFormat::None,
        response_format: ResponseFormat::Text,
        description: "Firmware Version",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_command_codes_are_unique() {
        let mut seen = HashSet::new();
        for cmd in COMMAND_TABLE.iter() {
            assert!(
                seen.insert(cmd.code),
                "Duplicate command code found: {}",
                cmd.code
            );
        }
    }

    #[test]
    fn test_all_commands_have_descriptions() {
        for cmd in COMMAND_TABLE.iter() {
            assert!(
                !cmd.description.is_empty(),
                "Command {} has no description",
                cmd.code
            );
        }
    }

    #[test]
    fn test_command_lookup() {
        // Test finding existing commands
        assert!(CommandMetadata::find("FA").is_some());
        assert!(CommandMetadata::find("TX").is_some());
        assert!(CommandMetadata::find("IF").is_some());

        // Test non-existent command
        assert!(CommandMetadata::find("ZZ").is_none());
        assert!(CommandMetadata::find("").is_none());
    }

    #[test]
    fn test_frequency_commands() {
        let fa = CommandMetadata::find("FA").unwrap();
        assert_eq!(fa.code, "FA");
        assert!(fa.supports_read);
        assert!(fa.supports_write);
        assert!(matches!(fa.param_format, ParamFormat::Frequency11Digit));
        assert!(matches!(
            fa.response_format,
            ResponseFormat::Frequency11Digit
        ));
        assert!(!fa.is_read_only());
        assert!(!fa.is_write_only());

        let fb = CommandMetadata::find("FB").unwrap();
        assert_eq!(fb.code, "FB");
        assert!(fb.supports_read);
        assert!(fb.supports_write);
    }

    #[test]
    fn test_mode_command() {
        let md = CommandMetadata::find("MD").unwrap();
        assert_eq!(md.code, "MD");
        assert!(md.supports_read);
        assert!(md.supports_write);
        assert!(matches!(md.param_format, ParamFormat::Mode));
        assert!(matches!(md.response_format, ResponseFormat::Mode));
        assert!(md.is_query_command());
        assert!(md.is_set_command());
    }

    #[test]
    fn test_transmit_commands() {
        let tx = CommandMetadata::find("TX").unwrap();
        assert_eq!(tx.code, "TX");
        assert!(!tx.supports_read);
        assert!(tx.supports_write);
        assert!(matches!(tx.param_format, ParamFormat::None));
        assert!(tx.is_write_only());
        assert!(tx.is_parameterless());

        let rx = CommandMetadata::find("RX").unwrap();
        assert_eq!(rx.code, "RX");
        assert!(!rx.supports_read);
        assert!(rx.supports_write);
        assert!(rx.is_write_only());
    }

    #[test]
    fn test_info_command() {
        let if_cmd = CommandMetadata::find("IF").unwrap();
        assert_eq!(if_cmd.code, "IF");
        assert!(if_cmd.supports_read);
        assert!(!if_cmd.supports_write);
        assert!(matches!(
            if_cmd.response_format,
            ResponseFormat::IfComposite
        ));
        assert!(if_cmd.is_read_only());

        let id = CommandMetadata::find("ID").unwrap();
        assert_eq!(id.code, "ID");
        assert!(id.supports_read);
        assert!(!id.supports_write);
        assert!(id.is_read_only());
    }

    #[test]
    fn test_gain_commands() {
        let ag = CommandMetadata::find("AG").unwrap();
        assert_eq!(ag.code, "AG");
        assert!(ag.supports_read);
        assert!(ag.supports_write);
        assert!(matches!(ag.param_format, ParamFormat::Level3Digit));

        let rg = CommandMetadata::find("RG").unwrap();
        assert_eq!(rg.code, "RG");
        assert!(matches!(rg.param_format, ParamFormat::Level3Digit));

        let mg = CommandMetadata::find("MG").unwrap();
        assert_eq!(mg.code, "MG");
        assert!(matches!(mg.param_format, ParamFormat::Level3Digit));
    }

    #[test]
    fn test_filter_commands() {
        let fw = CommandMetadata::find("FW").unwrap();
        assert_eq!(fw.code, "FW");
        assert!(matches!(fw.param_format, ParamFormat::Value4Digit));

        let sh = CommandMetadata::find("SH").unwrap();
        assert_eq!(sh.code, "SH");
        assert!(matches!(sh.param_format, ParamFormat::TwoDigit));

        let sl = CommandMetadata::find("SL").unwrap();
        assert_eq!(sl.code, "SL");
        assert!(matches!(sl.param_format, ParamFormat::TwoDigit));
    }

    #[test]
    fn test_vfo_commands() {
        let fr = CommandMetadata::find("FR").unwrap();
        assert_eq!(fr.code, "FR");
        assert!(matches!(fr.param_format, ParamFormat::VfoSelect));

        let ft = CommandMetadata::find("FT").unwrap();
        assert_eq!(ft.code, "FT");
        assert!(matches!(ft.param_format, ParamFormat::VfoSelect));
    }

    #[test]
    fn test_memory_commands() {
        let mc = CommandMetadata::find("MC").unwrap();
        assert_eq!(mc.code, "MC");
        assert!(matches!(mc.param_format, ParamFormat::MemoryChannel));

        let mr = CommandMetadata::find("MR").unwrap();
        assert_eq!(mr.code, "MR");
        assert!(mr.supports_read);
        assert!(mr.supports_write);

        let mw = CommandMetadata::find("MW").unwrap();
        assert_eq!(mw.code, "MW");
        assert!(!mw.supports_read);
        assert!(mw.supports_write);
        assert!(mw.is_write_only());
    }

    #[test]
    fn test_ctcss_commands() {
        let ct = CommandMetadata::find("CT").unwrap();
        assert_eq!(ct.code, "CT");
        assert!(matches!(ct.param_format, ParamFormat::OnOff));

        let cn = CommandMetadata::find("CN").unwrap();
        assert_eq!(cn.code, "CN");
        assert!(matches!(cn.param_format, ParamFormat::CtcssTone));
    }

    #[test]
    fn test_all_commands_have_valid_read_write_combination() {
        for cmd in COMMAND_TABLE.iter() {
            // At least one of read or write must be true
            assert!(
                cmd.supports_read || cmd.supports_write,
                "Command {} supports neither read nor write",
                cmd.code
            );
        }
    }

    #[test]
    fn test_parameterless_commands_have_no_param_format() {
        for cmd in COMMAND_TABLE.iter() {
            if !cmd.supports_write {
                // Read-only commands should have None param format
                assert!(
                    matches!(cmd.param_format, ParamFormat::None),
                    "Read-only command {} should have ParamFormat::None",
                    cmd.code
                );
            }
        }
    }

    #[test]
    fn test_command_table_coverage() {
        // Verify we have commands in all major categories
        let codes: Vec<&str> = COMMAND_TABLE.iter().map(|c| c.code).collect();

        // Frequency commands
        assert!(codes.contains(&"FA"));
        assert!(codes.contains(&"FB"));
        assert!(codes.contains(&"FC"));

        // Mode commands
        assert!(codes.contains(&"MD"));

        // VFO commands
        assert!(codes.contains(&"FR"));
        assert!(codes.contains(&"FT"));
        assert!(codes.contains(&"FN"));

        // Memory commands
        assert!(codes.contains(&"MC"));
        assert!(codes.contains(&"MR"));
        assert!(codes.contains(&"MW"));

        // Gain commands
        assert!(codes.contains(&"AG"));
        assert!(codes.contains(&"RG"));
        assert!(codes.contains(&"MG"));
        assert!(codes.contains(&"PC"));
        assert!(codes.contains(&"SQ"));

        // Filter commands
        assert!(codes.contains(&"FW"));
        assert!(codes.contains(&"SH"));
        assert!(codes.contains(&"SL"));

        // TX/RX commands
        assert!(codes.contains(&"TX"));
        assert!(codes.contains(&"RX"));

        // Info commands
        assert!(codes.contains(&"IF"));
        assert!(codes.contains(&"ID"));

        // RIT/XIT commands
        assert!(codes.contains(&"RT"));
        assert!(codes.contains(&"XT"));
        assert!(codes.contains(&"RC"));

        // Scan commands
        assert!(codes.contains(&"SC"));
        assert!(codes.contains(&"ST"));

        // CTCSS commands
        assert!(codes.contains(&"CT"));
        assert!(codes.contains(&"CN"));

        // Meter commands
        assert!(codes.contains(&"SM"));
        assert!(codes.contains(&"RM"));

        // Verify minimum command count (should have at least 60 commands)
        assert!(
            COMMAND_TABLE.len() >= 60,
            "Command table should have at least 60 commands, found {}",
            COMMAND_TABLE.len()
        );
    }

    #[test]
    fn test_helper_methods() {
        let fa = CommandMetadata::find("FA").unwrap();

        // Test is_query_command
        assert!(fa.is_query_command());

        // Test is_set_command
        assert!(fa.is_set_command());

        // Test is_read_only
        assert!(!fa.is_read_only());

        // Test is_write_only
        assert!(!fa.is_write_only());

        // Test is_parameterless
        assert!(!fa.is_parameterless());

        // Test with parameterless command
        let tx = CommandMetadata::find("TX").unwrap();
        assert!(tx.is_parameterless());
        assert!(tx.is_write_only());
        assert!(!tx.is_read_only());
    }
}
