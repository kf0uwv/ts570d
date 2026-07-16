//! TS-570D emulator state machine behind the generic CAT framework.

use framework::{
    CatCommandCatalog, CatRadio, CommandDefinition, CommandForm, CommandOperation, CommandOutcome,
    CommandRequest, CommandTable, ProtocolErrorKind, ResponseBuilder, ResponseDisposition,
};

/// TS-570D command identifier owned by the radio crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ts570dCommandId {
    Fa,
    Fb,
    Md,
    Ag,
    Rg,
    Sq,
    Pc,
    Tx,
    Rx,
    If,
    Id,
    Ai,
    Sm,
    Nb,
    Nr,
    Pa,
    Ra,
    Mg,
    Gt,
    Rt,
    Xt,
    Rc,
    Ru,
    Rd,
    Sc,
    Vx,
    Vg,
    Vd,
    Fr,
    Ft,
    Lk,
    Ps,
    By,
    Pr,
    Mc,
    An,
    Ks,
    Ky,
    Pt,
    Ca,
    Ac,
    Sh,
    Sl,
    Is,
    Cn,
    Ct,
    Tn,
    To,
    Bc,
    Rm,
    Fs,
    Sd,
    Up,
    Dn,
    Vr,
    Sr,
    Fw,
    Ex,
    Lm,
    Pb,
    Mr,
    Mw,
    Fv,
    // Commands the controller catalog knows but the emulator does not yet
    // emulate (it answers "?;"). Present so the single command table is the
    // documented superset. See docs/framework-refactor.md.
    Fc,
    Fn,
    Nl,
    St,
    Sp,
    Os,
    Bk,
    Qr,
    Mf,
}

const QUERY: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Query, 0)];
const ACTION: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Action, 0)];
const SET_1: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 1)];
const SET_2: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 2)];
const SET_3: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 3)];
const SET_4: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 4)];
const SET_5: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 5)];
const SET_7: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 7)];
const SET_11: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 11)];
const SET_ANY: &[CommandForm] = &[CommandForm::variable(CommandOperation::Set, 1, 64)];
const QUERY_SET_3: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Query, 3)];
const NONE: &[CommandForm] = &[];

macro_rules! definition {
    // Explicit controller read/write capability.
    ($id:ident, $code:literal, $name:literal, $query:expr, $set:expr, $action:expr, $readable:expr, $writable:expr) => {
        CommandDefinition {
            id: Ts570dCommandId::$id,
            code: $code,
            name: $name,
            description: $name,
            query_forms: $query,
            set_forms: $set,
            action_forms: $action,
            response_forms: NONE,
            readable: $readable,
            writable: $writable,
        }
    };
    // Derive read/write from the presence of query / set / action forms.
    ($id:ident, $code:literal, $name:literal, $query:expr, $set:expr, $action:expr) => {
        definition!(
            $id,
            $code,
            $name,
            $query,
            $set,
            $action,
            !$query.is_empty(),
            !$set.is_empty() || !$action.is_empty()
        )
    };
}

static DEFINITIONS: &[CommandDefinition<Ts570dCommandId>] = &[
    definition!(Fa, "FA", "VFO A Frequency", QUERY, SET_11, NONE),
    definition!(Fb, "FB", "VFO B Frequency", QUERY, SET_11, NONE),
    definition!(Md, "MD", "Operating Mode", QUERY, SET_1, NONE),
    definition!(Ag, "AG", "AF Gain", QUERY, SET_ANY, NONE),
    definition!(Rg, "RG", "RF Gain", QUERY, SET_3, NONE),
    definition!(Sq, "SQ", "Squelch", QUERY, SET_3, NONE),
    definition!(Pc, "PC", "Power Control", QUERY, SET_3, NONE),
    definition!(Tx, "TX", "Transmit", NONE, NONE, ACTION),
    definition!(Rx, "RX", "Receive", NONE, NONE, ACTION),
    definition!(If, "IF", "Information", QUERY, NONE, NONE),
    definition!(Id, "ID", "Identifier", QUERY, NONE, NONE),
    definition!(Ai, "AI", "Auto Information", QUERY, SET_1, NONE),
    definition!(Sm, "SM", "S Meter", QUERY, SET_1, NONE, true, false),
    definition!(Nb, "NB", "Noise Blanker", QUERY, SET_1, NONE),
    definition!(Nr, "NR", "Noise Reduction", QUERY, SET_1, NONE),
    definition!(Pa, "PA", "Preamp", QUERY, SET_1, NONE),
    definition!(Ra, "RA", "Attenuator", QUERY, SET_2, NONE),
    definition!(Mg, "MG", "Mic Gain", QUERY, SET_3, NONE),
    definition!(Gt, "GT", "AGC", QUERY, SET_3, NONE),
    definition!(Rt, "RT", "RIT", QUERY, SET_1, NONE),
    definition!(Xt, "XT", "XIT", QUERY, SET_1, NONE),
    definition!(Rc, "RC", "RIT Clear", NONE, NONE, ACTION),
    definition!(Ru, "RU", "RIT Up", NONE, NONE, ACTION),
    definition!(Rd, "RD", "RIT Down", NONE, NONE, ACTION),
    definition!(Sc, "SC", "Scan", QUERY, SET_1, NONE),
    definition!(Vx, "VX", "VOX", QUERY, SET_1, NONE),
    definition!(Vg, "VG", "VOX Gain", QUERY, SET_3, NONE),
    definition!(Vd, "VD", "VOX Delay", QUERY, SET_4, NONE),
    definition!(Fr, "FR", "RX VFO", QUERY, SET_1, NONE),
    definition!(Ft, "FT", "TX VFO", QUERY, SET_1, NONE),
    definition!(Lk, "LK", "Frequency Lock", QUERY, SET_1, NONE),
    definition!(Ps, "PS", "Power Status", QUERY, SET_1, NONE),
    definition!(By, "BY", "Busy", QUERY, NONE, NONE),
    definition!(Pr, "PR", "Speech Processor", QUERY, SET_1, NONE),
    definition!(Mc, "MC", "Memory Channel", QUERY, SET_ANY, NONE),
    definition!(An, "AN", "Antenna", QUERY, SET_1, NONE),
    definition!(Ks, "KS", "Keyer Speed", QUERY, SET_3, NONE),
    definition!(Ky, "KY", "CW Keying", QUERY, SET_ANY, NONE, false, true),
    definition!(Pt, "PT", "CW Pitch", QUERY, SET_2, NONE),
    definition!(Ca, "CA", "CW Auto Zerobeat", QUERY, SET_1, NONE),
    definition!(Ac, "AC", "Antenna Tuner", QUERY, SET_2, NONE),
    definition!(Sh, "SH", "High Cutoff", QUERY, SET_2, NONE),
    definition!(Sl, "SL", "Low Cutoff", QUERY, SET_2, NONE),
    definition!(Is, "IS", "IF Shift", QUERY, SET_5, NONE),
    definition!(Cn, "CN", "CTCSS Tone", QUERY, SET_2, NONE),
    definition!(Ct, "CT", "CTCSS", QUERY, SET_1, NONE),
    definition!(Tn, "TN", "Tone Number", QUERY, SET_2, NONE),
    definition!(To, "TO", "Tone", QUERY, SET_1, NONE),
    definition!(Bc, "BC", "Beat Cancel", QUERY, SET_1, NONE),
    // SM/KY/MR: the wire-grammar forms take a selector parameter, so their
    // documented controller read/write is stated explicitly (docs authoritative).
    definition!(Rm, "RM", "Meter", QUERY, SET_1, NONE),
    definition!(Fs, "FS", "Fine Step", QUERY, SET_1, NONE),
    definition!(Sd, "SD", "Semi Break-in Delay", QUERY, SET_4, NONE),
    definition!(Up, "UP", "Frequency Up", NONE, NONE, ACTION),
    definition!(Dn, "DN", "Frequency Down", NONE, NONE, ACTION),
    definition!(Vr, "VR", "Voice Recall", NONE, SET_1, NONE),
    definition!(Sr, "SR", "System Reset", NONE, SET_1, NONE),
    definition!(Fw, "FW", "Filter Width", QUERY, SET_4, NONE),
    definition!(Ex, "EX", "Extension Menu", QUERY_SET_3, SET_7, NONE),
    definition!(Lm, "LM", "Load Message", NONE, SET_1, NONE),
    definition!(Pb, "PB", "Playback", QUERY, SET_1, NONE),
    definition!(Mr, "MR", "Memory Read", NONE, SET_ANY, NONE, true, true),
    definition!(Mw, "MW", "Memory Write", NONE, SET_ANY, NONE),
    definition!(Fv, "FV", "Firmware Version", QUERY, NONE, NONE),
    // Controller-catalog commands not yet emulated (emulator answers "?;").
    // Widths follow the documented CAT layout; read/write per the manual.
    definition!(Fc, "FC", "Sub-receiver VFO Frequency", QUERY, SET_11, NONE),
    definition!(Fn, "FN", "VFO A/B Selection", QUERY, SET_1, NONE),
    definition!(Nl, "NL", "Noise Reduction Level", QUERY, SET_3, NONE),
    definition!(St, "ST", "Scan Type", QUERY, SET_1, NONE),
    definition!(Sp, "SP", "Split Operation", QUERY, SET_1, NONE),
    definition!(Os, "OS", "Offset", QUERY, SET_1, NONE),
    definition!(Bk, "BK", "Break-in On/Off", QUERY, SET_1, NONE),
    definition!(Qr, "QR", "Quick Memory Store", NONE, SET_1, NONE),
    definition!(Mf, "MF", "Memory Function", QUERY, SET_1, NONE),
];

/// TS-570D command table used by the generic framework.
pub static TS570D_COMMAND_TABLE: CommandTable<Ts570dCommandId> = CommandTable::new(DEFINITIONS);

/// A single memory channel entry.
#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryChannel {
    pub freq: u64,
    pub mode: u8,
    pub lockout: bool,
    pub tone: bool,
    pub tone_num: u8,
}

/// VFO / memory selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VfoSel {
    A,
    B,
    Memory,
}

/// Simulated TS-570D radio state.
#[derive(Debug, Clone)]
pub struct Ts570dState {
    pub vfo_a_hz: u64,
    pub vfo_b_hz: u64,
    pub mode: u8,
    pub tx: bool,
    pub af_gain: u8,
    pub rf_gain: u8,
    pub squelch: u8,
    pub power_control: u8,
    pub smeter: u16,
    pub auto_info: u8,
    pub antenna_tuner: bool,
    pub antenna: u8,
    pub attenuator: bool,
    pub preamp: bool,
    pub vox: bool,
    pub proc: bool,
    pub noise_blanker: bool,
    pub split: bool,
    pub fast_agc: bool,
    pub rit: bool,
    pub xit: bool,
    pub tx_eq: bool,
    pub noise_reduction: u8,
    pub beat_cancel: bool,
    pub menu_mode: bool,
    pub memory_scroll: bool,
    pub active_vfo: VfoSel,
    pub freq_lock: bool,
    pub fine_step: bool,
    pub mhz_step: bool,
    pub subtone: bool,
    pub ctcss: bool,
    pub ctrl: bool,
    pub mem_channel: u8,
    pub menu_number: u8,
    pub scan: bool,
    pub mic_gain: u8,
    pub agc: u8,
    pub vox_gain: u8,
    pub vox_delay: u16,
    pub rx_vfo: u8,
    pub tx_vfo: u8,
    pub power_on: bool,
    pub keyer_speed: u8,
    pub cw_pitch: u8,
    pub cw_auto_zerobeat: bool,
    pub ac_mode: u8,
    pub sh: u8,
    pub sl: u8,
    pub is_direction: char,
    pub is_freq: u16,
    pub ctcss_tone: u8,
    pub tone_number: u8,
    pub beat_cancel_mode: u8,
    pub semi_break_in_delay: u16,
    pub rit_offset: i32,
    pub xit_offset: i32,
    pub filter_width: u16,
    pub menu_values: [u16; 52],
    pub playback_channel: u8,
    pub memory_channels: [MemoryChannel; 100],
}

impl Default for Ts570dState {
    fn default() -> Self {
        Self {
            vfo_a_hz: 14_000_000,
            vfo_b_hz: 14_100_000,
            mode: 2,
            tx: false,
            af_gain: 128,
            rf_gain: 200,
            squelch: 0,
            power_control: 50,
            smeter: 10,
            auto_info: 0,
            antenna_tuner: false,
            antenna: 1,
            attenuator: false,
            preamp: false,
            vox: false,
            proc: false,
            noise_blanker: false,
            split: false,
            fast_agc: false,
            rit: false,
            xit: false,
            tx_eq: false,
            noise_reduction: 0,
            beat_cancel: false,
            menu_mode: false,
            memory_scroll: false,
            active_vfo: VfoSel::A,
            freq_lock: false,
            fine_step: false,
            mhz_step: false,
            subtone: false,
            ctcss: false,
            ctrl: false,
            mem_channel: 0,
            menu_number: 0,
            scan: false,
            mic_gain: 50,
            agc: 4,
            vox_gain: 5,
            vox_delay: 250,
            rx_vfo: 0,
            tx_vfo: 0,
            power_on: true,
            keyer_speed: 20,
            cw_pitch: 6,
            cw_auto_zerobeat: false,
            ac_mode: 0,
            sh: 10,
            sl: 0,
            is_direction: ' ',
            is_freq: 0,
            ctcss_tone: 0,
            tone_number: 0,
            beat_cancel_mode: 0,
            semi_break_in_delay: 0,
            rit_offset: 0,
            xit_offset: 0,
            filter_width: 0,
            menu_values: [0u16; 52],
            playback_channel: 0,
            memory_channels: [MemoryChannel::default(); 100],
        }
    }
}

/// Radio-specific state change event used by emulator logging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ts570dEvent {
    pub field: &'static str,
    pub value: String,
}

/// TS-570D emulator radio implementation.
#[derive(Debug, Default)]
pub struct Ts570dRadio {
    state: Ts570dState,
}

impl Ts570dRadio {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &Ts570dState {
        &self.state
    }

    fn handle_raw(&mut self, cmd: &str) -> (String, Vec<Ts570dEvent>) {
        crate::ts570d_radio_handlers::handle(cmd, &mut self.state)
    }
}

impl CatCommandCatalog for Ts570dRadio {
    type CommandId = Ts570dCommandId;

    fn command_table(&self) -> &'static CommandTable<Self::CommandId> {
        &TS570D_COMMAND_TABLE
    }
}

impl CatRadio for Ts570dRadio {
    type Event = Ts570dEvent;
    type Error = core::convert::Infallible;

    fn handle_command(
        &mut self,
        request: CommandRequest<'_, Self::CommandId>,
        response: &mut ResponseBuilder<'_>,
    ) -> Result<CommandOutcome<Self::Event>, Self::Error> {
        let mut raw = String::with_capacity(request.code.len() + request.parameters.raw().len());
        raw.push_str(request.code);
        raw.push_str(request.parameters.raw());
        let (wire_response, events) = self.handle_raw(&raw);
        if wire_response.is_empty() {
            Ok(CommandOutcome {
                response: ResponseDisposition::NoResponse,
                events,
            })
        } else {
            response
                .write_complete(&wire_response)
                .expect("response write cannot fail before finish");
            Ok(CommandOutcome {
                response: ResponseDisposition::ResponseWritten,
                events,
            })
        }
    }

    fn write_protocol_error(
        &mut self,
        kind: ProtocolErrorKind,
        response: &mut ResponseBuilder<'_>,
    ) -> Result<CommandOutcome<Self::Event>, Self::Error> {
        response
            .write_complete("?;")
            .expect("response write cannot fail before finish");
        Ok(CommandOutcome {
            response: ResponseDisposition::ProtocolError(kind),
            events: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use framework::CatFramework;

    use super::*;

    #[test]
    fn table_has_unique_codes_and_ids() {
        let mut codes = HashSet::new();
        let mut ids = HashSet::new();
        for definition in TS570D_COMMAND_TABLE.definitions() {
            assert!(
                codes.insert(definition.code),
                "duplicate code {}",
                definition.code
            );
            assert!(
                ids.insert(definition.id),
                "duplicate id {:?}",
                definition.id
            );
            assert!(
                !definition.query_forms.is_empty()
                    || !definition.set_forms.is_empty()
                    || !definition.action_forms.is_empty(),
                "{} has no legal operation",
                definition.code
            );
        }
    }

    #[test]
    fn framework_query_returns_wire_response() {
        let mut framework = CatFramework::new(Ts570dRadio::new());
        let mut output = Vec::new();
        framework.process_frame("FA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "FA00014000000;");
    }

    #[test]
    fn framework_set_then_query_preserves_state() {
        let mut framework = CatFramework::new(Ts570dRadio::new());
        let mut output = Vec::new();
        framework
            .process_frame("FA00014250000;", &mut output)
            .unwrap();
        assert!(output.is_empty());

        framework.process_frame("FA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "FA00014250000;");
    }

    #[test]
    fn framework_unknown_command_uses_ts570d_error_response() {
        let mut framework = CatFramework::new(Ts570dRadio::new());
        let mut output = Vec::new();
        let outcome = framework.process_frame("ZZ;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
        assert!(matches!(
            outcome.response,
            ResponseDisposition::ProtocolError(ProtocolErrorKind::UnknownCommand)
        ));
    }
}
