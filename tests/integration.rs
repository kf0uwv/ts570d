//! Integration tests for Ts570d<SerialPort> against the built-in emulator.
//!
//! Each test starts a fresh emulator instance (backed by a new PTY pair),
//! opens a real SerialPort on the slave PTY, and exercises the full
//! command/response round-trip through the io_uring serial driver.
//!
//! ## Why each test owns its emulator
//!
//! Starting a fresh emulator per test gives clean RadioState (default values)
//! and avoids ordering dependencies between tests.
//!
//! ## Set-then-get behaviour
//!
//! `Ts570d::set` is fire-and-forget: it writes the SET command but does NOT
//! read a response (per Kenwood CAT protocol, SET commands are silent — the
//! radio produces no response).  The subsequent `get` call sends a GET query
//! and reads the proper query response, confirming the value was applied.

use std::time::Duration;

use emulator::emulator::Emulator;
use framework::radio::{Frequency, Mode};
use monoio::RuntimeBuilder;
use radio::Ts570d;
use serial::{SerialConfig, SerialPort};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Start an emulator in a background thread and return the slave PTY path.
///
/// Waits 150 ms for the emulator thread to enter its read loop before returning.
fn start_emulator() -> String {
    let mut emu = Emulator::new().expect("Emulator::new failed");
    let slave_path = emu.slave_path().to_string();
    std::thread::spawn(move || {
        let _ = emu.run();
    });
    // Give the background thread time to enter its read loop.
    std::thread::sleep(Duration::from_millis(150));
    slave_path
}

/// Open a `Ts570d<SerialPort>` on the given slave PTY path at 4800 baud (8N2).
///
/// Must be called from within an active monoio runtime context.
fn open_radio(slave_path: &str) -> Ts570d<SerialPort> {
    let cfg = SerialConfig {
        baud_rate: 4800,
        ..SerialConfig::default()
    };
    let port = SerialPort::open(slave_path, cfg)
        .unwrap_or_else(|e| panic!("SerialPort::open({}) failed: {}", slave_path, e));
    Ts570d::new(port)
}

/// Build a monoio IoUring runtime for use in tests.
fn make_runtime() -> monoio::Runtime<monoio::IoUringDriver> {
    RuntimeBuilder::<monoio::IoUringDriver>::new()
        .build()
        .expect("monoio IoUring runtime build failed")
}

/// Run an async test body inside a monoio io_uring runtime.
macro_rules! async_test {
    ($body:expr) => {
        make_runtime().block_on($body)
    };
}

// ---------------------------------------------------------------------------
// VFO A frequency
// ---------------------------------------------------------------------------

#[test]
fn test_get_vfo_a() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let freq = radio.get_vfo_a().await.expect("get_vfo_a");
        // Default state: 14.000 MHz; valid range 500 kHz – 60 MHz.
        assert!(
            freq.hz() >= 500_000 && freq.hz() <= 60_000_000,
            "VFO A out of range: {} Hz",
            freq.hz()
        );
    });
}

#[test]
fn test_set_vfo_a() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let target = Frequency::new(14_195_000).expect("Frequency::new");
        radio.set_vfo_a(target).await.expect("set_vfo_a");
        // SET is silent; get_vfo_a sends a GET query and reads the response.
        let got = radio.get_vfo_a().await.expect("get_vfo_a after set");
        assert_eq!(got.hz(), 14_195_000, "VFO A mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// VFO B frequency
// ---------------------------------------------------------------------------

#[test]
fn test_get_vfo_b() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let freq = radio.get_vfo_b().await.expect("get_vfo_b");
        assert!(
            freq.hz() >= 500_000 && freq.hz() <= 60_000_000,
            "VFO B out of range: {} Hz",
            freq.hz()
        );
    });
}

#[test]
fn test_set_vfo_b() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let target = Frequency::new(7_100_000).expect("Frequency::new");
        radio.set_vfo_b(target).await.expect("set_vfo_b");
        let got = radio.get_vfo_b().await.expect("get_vfo_b after set");
        assert_eq!(got.hz(), 7_100_000, "VFO B mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Fine step
// ---------------------------------------------------------------------------

#[test]
fn test_get_fine_step() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_fine_step().await.expect("get_fine_step");
        // Default state: false
        assert!(!v, "expected fine_step default=false");
    });
}

#[test]
fn test_set_fine_step_on() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_fine_step(true).await.expect("set_fine_step(true)");
        let v = radio.get_fine_step().await.expect("get_fine_step after set true");
        assert!(v, "expected fine_step=true after set");
    });
}

/// Verify that set_fine_step(false) is accepted by the emulator (no error).
/// The default state is already false, so this confirms the command is handled.
#[test]
fn test_set_fine_step_off() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        // Default state is fine_step=false.  Setting it to false should succeed.
        radio
            .set_fine_step(false)
            .await
            .expect("set_fine_step(false) should not error");
        // Read back via GET query (SET is silent).
        let v = radio
            .get_fine_step()
            .await
            .expect("get_fine_step after set_false");
        assert!(!v, "expected fine_step=false");
    });
}

// ---------------------------------------------------------------------------
// Frequency lock
// ---------------------------------------------------------------------------

#[test]
fn test_get_frequency_lock() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_frequency_lock().await.expect("get_frequency_lock");
        assert!(!v, "expected freq_lock default=false");
    });
}

#[test]
fn test_set_frequency_lock() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_frequency_lock(true)
            .await
            .expect("set_frequency_lock(true)");
        let v = radio
            .get_frequency_lock()
            .await
            .expect("get_frequency_lock after set");
        assert!(v, "expected freq_lock=true after set");
    });
}

// ---------------------------------------------------------------------------
// Operating mode
// ---------------------------------------------------------------------------

#[test]
fn test_get_mode() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let m = radio.get_mode().await.expect("get_mode");
        // Default state: USB (2)
        assert_eq!(m, Mode::Usb, "expected default mode USB");
    });
}

#[test]
fn test_set_mode_usb() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_mode(Mode::Usb).await.expect("set_mode(Usb)");
        let m = radio.get_mode().await.expect("get_mode after set Usb");
        assert_eq!(m, Mode::Usb);
    });
}

#[test]
fn test_set_mode_lsb() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_mode(Mode::Lsb).await.expect("set_mode(Lsb)");
        let m = radio.get_mode().await.expect("get_mode after set Lsb");
        assert_eq!(m, Mode::Lsb);
    });
}

#[test]
fn test_set_mode_cw() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_mode(Mode::Cw).await.expect("set_mode(Cw)");
        let m = radio.get_mode().await.expect("get_mode after set Cw");
        assert_eq!(m, Mode::Cw);
    });
}

// ---------------------------------------------------------------------------
// RIT
// ---------------------------------------------------------------------------

#[test]
fn test_get_rit() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_rit().await.expect("get_rit");
        assert!(!v, "expected rit default=false");
    });
}

#[test]
fn test_set_rit_on() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_rit(true).await.expect("set_rit(true)");
        let v = radio.get_rit().await.expect("get_rit after set");
        assert!(v, "expected rit=true after set");
    });
}

/// Verify that set_rit(false) is accepted by the emulator (no error).
#[test]
fn test_set_rit_off() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        // Default state is rit=false.  Setting it to false should succeed.
        radio
            .set_rit(false)
            .await
            .expect("set_rit(false) should not error");
        // Read back via GET query (SET is silent).
        let v = radio.get_rit().await.expect("get_rit after set_false");
        assert!(!v, "expected rit=false");
    });
}

// ---------------------------------------------------------------------------
// XIT
// ---------------------------------------------------------------------------

#[test]
fn test_get_xit() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_xit().await.expect("get_xit");
        assert!(!v, "expected xit default=false");
    });
}

#[test]
fn test_set_xit_on() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_xit(true).await.expect("set_xit(true)");
        let v = radio.get_xit().await.expect("get_xit after set");
        assert!(v, "expected xit=true after set");
    });
}

// ---------------------------------------------------------------------------
// AF gain
// ---------------------------------------------------------------------------

#[test]
fn test_get_af_gain() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_af_gain().await.expect("get_af_gain");
        // Default: 128
        assert_eq!(v, 128, "expected af_gain default=128");
    });
}

#[test]
fn test_set_af_gain() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_af_gain(200).await.expect("set_af_gain(200)");
        let v = radio.get_af_gain().await.expect("get_af_gain after set");
        assert_eq!(v, 200, "af_gain mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// RF gain
// ---------------------------------------------------------------------------

#[test]
fn test_get_rf_gain() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_rf_gain().await.expect("get_rf_gain");
        // Default: 200
        assert_eq!(v, 200, "expected rf_gain default=200");
    });
}

#[test]
fn test_set_rf_gain() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_rf_gain(100).await.expect("set_rf_gain(100)");
        let v = radio.get_rf_gain().await.expect("get_rf_gain after set");
        assert_eq!(v, 100, "rf_gain mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Squelch
// ---------------------------------------------------------------------------

#[test]
fn test_get_squelch() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_squelch().await.expect("get_squelch");
        // Default: 0
        assert_eq!(v, 0, "expected squelch default=0");
    });
}

#[test]
fn test_set_squelch() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_squelch(30).await.expect("set_squelch(30)");
        let v = radio.get_squelch().await.expect("get_squelch after set");
        assert_eq!(v, 30, "squelch mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Microphone gain
// ---------------------------------------------------------------------------

#[test]
fn test_get_mic_gain() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_mic_gain().await.expect("get_mic_gain");
        // Default: 50
        assert_eq!(v, 50, "expected mic_gain default=50");
    });
}

#[test]
fn test_set_mic_gain() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_mic_gain(80).await.expect("set_mic_gain(80)");
        let v = radio.get_mic_gain().await.expect("get_mic_gain after set");
        assert_eq!(v, 80, "mic_gain mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Transmit power
// ---------------------------------------------------------------------------

#[test]
fn test_get_power() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_power().await.expect("get_power");
        // Default: 50
        assert_eq!(v, 50, "expected power default=50");
    });
}

#[test]
fn test_set_power() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_power(75).await.expect("set_power(75)");
        let v = radio.get_power().await.expect("get_power after set");
        assert_eq!(v, 75, "power mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Noise blanker
// ---------------------------------------------------------------------------

#[test]
fn test_get_noise_blanker() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_noise_blanker().await.expect("get_noise_blanker");
        assert!(!v, "expected noise_blanker default=false");
    });
}

#[test]
fn test_set_noise_blanker() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_noise_blanker(true)
            .await
            .expect("set_noise_blanker(true)");
        let v = radio
            .get_noise_blanker()
            .await
            .expect("get_noise_blanker after set");
        assert!(v, "expected noise_blanker=true after set");
    });
}

// ---------------------------------------------------------------------------
// Noise reduction
// ---------------------------------------------------------------------------

#[test]
fn test_get_noise_reduction() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio
            .get_noise_reduction()
            .await
            .expect("get_noise_reduction");
        assert_eq!(v, 0, "expected noise_reduction default=0");
    });
}

#[test]
fn test_set_noise_reduction() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_noise_reduction(1)
            .await
            .expect("set_noise_reduction(1)");
        let v = radio
            .get_noise_reduction()
            .await
            .expect("get_noise_reduction after set");
        assert_eq!(v, 1, "noise_reduction mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Pre-amplifier
// ---------------------------------------------------------------------------

#[test]
fn test_get_preamp() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_preamp().await.expect("get_preamp");
        assert!(!v, "expected preamp default=false");
    });
}

#[test]
fn test_set_preamp() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_preamp(true).await.expect("set_preamp(true)");
        let v = radio.get_preamp().await.expect("get_preamp after set");
        assert!(v, "expected preamp=true after set");
    });
}

// ---------------------------------------------------------------------------
// Attenuator
// ---------------------------------------------------------------------------

#[test]
fn test_get_attenuator() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_attenuator().await.expect("get_attenuator");
        assert!(!v, "expected attenuator default=false");
    });
}

#[test]
fn test_set_attenuator() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_attenuator(true)
            .await
            .expect("set_attenuator(true)");
        let v = radio
            .get_attenuator()
            .await
            .expect("get_attenuator after set");
        assert!(v, "expected attenuator=true after set");
    });
}

// ---------------------------------------------------------------------------
// AGC
// ---------------------------------------------------------------------------

#[test]
fn test_get_agc() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_agc().await.expect("get_agc");
        // Default: 4 (slow)
        assert_eq!(v, 4, "expected agc default=4");
    });
}

#[test]
fn test_set_agc() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_agc(2).await.expect("set_agc(2)");
        let v = radio.get_agc().await.expect("get_agc after set");
        assert_eq!(v, 2, "agc mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// VOX
// ---------------------------------------------------------------------------

#[test]
fn test_get_vox() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_vox().await.expect("get_vox");
        assert!(!v, "expected vox default=false");
    });
}

#[test]
fn test_set_vox() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_vox(true).await.expect("set_vox(true)");
        let v = radio.get_vox().await.expect("get_vox after set");
        assert!(v, "expected vox=true after set");
    });
}

// ---------------------------------------------------------------------------
// Speech processor
// ---------------------------------------------------------------------------

#[test]
fn test_get_speech_processor() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio
            .get_speech_processor()
            .await
            .expect("get_speech_processor");
        assert!(!v, "expected speech_processor default=false");
    });
}

#[test]
fn test_set_speech_processor() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_speech_processor(true)
            .await
            .expect("set_speech_processor(true)");
        let v = radio
            .get_speech_processor()
            .await
            .expect("get_speech_processor after set");
        assert!(v, "expected speech_processor=true after set");
    });
}

// ---------------------------------------------------------------------------
// Beat cancel
// ---------------------------------------------------------------------------

#[test]
fn test_get_beat_cancel() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_beat_cancel().await.expect("get_beat_cancel");
        // Default: 0 (off)
        assert_eq!(v, 0, "expected beat_cancel default=0");
    });
}

// ---------------------------------------------------------------------------
// Antenna
// ---------------------------------------------------------------------------

#[test]
fn test_get_antenna() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_antenna().await.expect("get_antenna");
        // Default: 1
        assert_eq!(v, 1, "expected antenna default=1");
    });
}

#[test]
fn test_set_antenna() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_antenna(2).await.expect("set_antenna(2)");
        let v = radio.get_antenna().await.expect("get_antenna after set");
        assert_eq!(v, 2, "antenna mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// S-meter (read-only)
// ---------------------------------------------------------------------------

#[test]
fn test_get_smeter() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_smeter().await.expect("get_smeter");
        assert!(v <= 30, "smeter out of range: {}", v);
    });
}

// ---------------------------------------------------------------------------
// Meter (RM command — SWR/COMP/ALC/power)
// ---------------------------------------------------------------------------

#[test]
fn test_get_meter() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_meter(1).await.expect("get_meter(1)");
        // RM1 returns power_control proxy (0–100 range as u16)
        assert!(v <= 9999, "meter value out of expected range: {}", v);
    });
}

// ---------------------------------------------------------------------------
// Full information (IF command)
// ---------------------------------------------------------------------------

#[test]
fn test_get_information() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let info = radio.get_information().await.expect("get_information");
        // Verify the fields parse into plausible values.
        let freq_hz = info.frequency.hz();
        assert!(
            freq_hz >= 500_000 && freq_hz <= 60_000_000,
            "IF frequency out of range: {} Hz",
            freq_hz
        );
        // Mode must be a valid variant (the parser would have returned an error
        // for unknown modes, so any Ok here implies a valid mode).
        let _ = info.mode; // successfully parsed → valid
    });
}

// ---------------------------------------------------------------------------
// Radio identification (ID command)
// ---------------------------------------------------------------------------

#[test]
fn test_get_id() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let id = radio.get_id().await.expect("get_id");
        // Emulator returns "ID017;" → 17u16 (TS-570D model code).
        assert_eq!(id, 17, "unexpected radio ID: {}", id);
    });
}

// ---------------------------------------------------------------------------
// PTT — transmit / receive
// ---------------------------------------------------------------------------

#[test]
fn test_transmit() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.transmit().await.expect("transmit");
    });
}

#[test]
fn test_receive() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        // transmit first so receive is a meaningful state change.
        radio.transmit().await.expect("transmit");
        radio.receive().await.expect("receive");
    });
}

// ---------------------------------------------------------------------------
// Power on/off
// ---------------------------------------------------------------------------

#[test]
fn test_get_power_on() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_power_on().await.expect("get_power_on");
        // Default state: true (radio is on when emulator starts)
        assert!(v, "expected power_on default=true");
    });
}

#[test]
fn test_set_power_on() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_power_on(false).await.expect("set_power_on(false)");
        let v = radio.get_power_on().await.expect("get_power_on after set");
        assert!(!v, "expected power_on=false after set");
    });
}

// ---------------------------------------------------------------------------
// Busy status (read-only)
// ---------------------------------------------------------------------------

#[test]
fn test_is_busy() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.is_busy().await.expect("is_busy");
        // Emulator always returns BY0 (not busy).
        assert!(!v, "expected busy=false from emulator");
    });
}

// ---------------------------------------------------------------------------
// RIT clear / up / down
// ---------------------------------------------------------------------------

#[test]
fn test_clear_rit() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.clear_rit().await.expect("clear_rit");
    });
}

#[test]
fn test_rit_up() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.rit_up().await.expect("rit_up");
    });
}

#[test]
fn test_rit_down() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.rit_down().await.expect("rit_down");
    });
}

// ---------------------------------------------------------------------------
// Scan
// ---------------------------------------------------------------------------

#[test]
fn test_get_scan() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_scan().await.expect("get_scan");
        assert!(!v, "expected scan default=false");
    });
}

#[test]
fn test_set_scan() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_scan(true).await.expect("set_scan(true)");
        let v = radio.get_scan().await.expect("get_scan after set");
        assert!(v, "expected scan=true after set");
    });
}

// ---------------------------------------------------------------------------
// VOX gain
// ---------------------------------------------------------------------------

#[test]
fn test_get_vox_gain() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_vox_gain().await.expect("get_vox_gain");
        // Default: 128
        assert_eq!(v, 128, "expected vox_gain default=128");
    });
}

#[test]
fn test_set_vox_gain() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_vox_gain(200).await.expect("set_vox_gain(200)");
        let v = radio.get_vox_gain().await.expect("get_vox_gain after set");
        assert_eq!(v, 200, "vox_gain mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// VOX delay
// ---------------------------------------------------------------------------

#[test]
fn test_get_vox_delay() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_vox_delay().await.expect("get_vox_delay");
        // Default: 250 ms
        assert_eq!(v, 250, "expected vox_delay default=250");
    });
}

#[test]
fn test_set_vox_delay() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_vox_delay(500).await.expect("set_vox_delay(500)");
        let v = radio.get_vox_delay().await.expect("get_vox_delay after set");
        assert_eq!(v, 500, "vox_delay mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// RX VFO selection (FR)
// ---------------------------------------------------------------------------

#[test]
fn test_get_rx_vfo() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_rx_vfo().await.expect("get_rx_vfo");
        // Default: 0 (VFO A)
        assert_eq!(v, 0, "expected rx_vfo default=0");
    });
}

#[test]
fn test_set_rx_vfo() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_rx_vfo(1).await.expect("set_rx_vfo(1)");
        let v = radio.get_rx_vfo().await.expect("get_rx_vfo after set");
        assert_eq!(v, 1, "rx_vfo mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// TX VFO selection (FT)
// ---------------------------------------------------------------------------

#[test]
fn test_get_tx_vfo() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_tx_vfo().await.expect("get_tx_vfo");
        // Default: 0 (VFO A)
        assert_eq!(v, 0, "expected tx_vfo default=0");
    });
}

#[test]
fn test_set_tx_vfo() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_tx_vfo(1).await.expect("set_tx_vfo(1)");
        let v = radio.get_tx_vfo().await.expect("get_tx_vfo after set");
        assert_eq!(v, 1, "tx_vfo mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Memory channel (MC)
// ---------------------------------------------------------------------------

#[test]
fn test_get_memory_channel() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_memory_channel().await.expect("get_memory_channel");
        // Default: 0
        assert_eq!(v, 0, "expected mem_channel default=0");
    });
}

#[test]
fn test_set_memory_channel() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_memory_channel(42)
            .await
            .expect("set_memory_channel(42)");
        let v = radio
            .get_memory_channel()
            .await
            .expect("get_memory_channel after set");
        assert_eq!(v, 42, "mem_channel mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Keyer speed (KS)
// ---------------------------------------------------------------------------

#[test]
fn test_get_keyer_speed() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_keyer_speed().await.expect("get_keyer_speed");
        // Default: 20 WPM
        assert_eq!(v, 20, "expected keyer_speed default=20");
    });
}

#[test]
fn test_set_keyer_speed() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_keyer_speed(25).await.expect("set_keyer_speed(25)");
        let v = radio
            .get_keyer_speed()
            .await
            .expect("get_keyer_speed after set");
        assert_eq!(v, 25, "keyer_speed mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// CW pitch (PT)
// ---------------------------------------------------------------------------

#[test]
fn test_get_cw_pitch() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_cw_pitch().await.expect("get_cw_pitch");
        // Default: 6 (700 Hz)
        assert_eq!(v, 6, "expected cw_pitch default=6");
    });
}

#[test]
fn test_set_cw_pitch() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_cw_pitch(8).await.expect("set_cw_pitch(8)");
        let v = radio.get_cw_pitch().await.expect("get_cw_pitch after set");
        assert_eq!(v, 8, "cw_pitch mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// CW auto zero-beat (CA)
// ---------------------------------------------------------------------------

#[test]
fn test_get_cw_auto_zerobeat() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio
            .get_cw_auto_zerobeat()
            .await
            .expect("get_cw_auto_zerobeat");
        assert!(!v, "expected cw_auto_zerobeat default=false");
    });
}

#[test]
fn test_set_cw_auto_zerobeat() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_cw_auto_zerobeat(true)
            .await
            .expect("set_cw_auto_zerobeat(true)");
        let v = radio
            .get_cw_auto_zerobeat()
            .await
            .expect("get_cw_auto_zerobeat after set");
        assert!(v, "expected cw_auto_zerobeat=true after set");
    });
}

// ---------------------------------------------------------------------------
// Antenna tuner thru / start tuning (AC)
// ---------------------------------------------------------------------------

#[test]
fn test_set_antenna_tuner_thru() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_antenna_tuner_thru(true)
            .await
            .expect("set_antenna_tuner_thru(true)");
    });
}

#[test]
fn test_start_antenna_tuning() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .start_antenna_tuning()
            .await
            .expect("start_antenna_tuning");
    });
}

// ---------------------------------------------------------------------------
// High cutoff filter (SH)
// ---------------------------------------------------------------------------

#[test]
fn test_get_high_cutoff() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_high_cutoff().await.expect("get_high_cutoff");
        // Default: 10
        assert_eq!(v, 10, "expected high_cutoff default=10");
    });
}

#[test]
fn test_set_high_cutoff() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_high_cutoff(5).await.expect("set_high_cutoff(5)");
        let v = radio
            .get_high_cutoff()
            .await
            .expect("get_high_cutoff after set");
        assert_eq!(v, 5, "high_cutoff mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Low cutoff filter (SL)
// ---------------------------------------------------------------------------

#[test]
fn test_get_low_cutoff() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_low_cutoff().await.expect("get_low_cutoff");
        // Default: 0
        assert_eq!(v, 0, "expected low_cutoff default=0");
    });
}

#[test]
fn test_set_low_cutoff() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_low_cutoff(3).await.expect("set_low_cutoff(3)");
        let v = radio
            .get_low_cutoff()
            .await
            .expect("get_low_cutoff after set");
        assert_eq!(v, 3, "low_cutoff mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// IF shift (IS)
// ---------------------------------------------------------------------------

#[test]
fn test_get_if_shift() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let (dir, freq) = radio.get_if_shift().await.expect("get_if_shift");
        // Default: direction='+', freq=0
        assert_eq!(dir, '+', "expected if_shift direction default='+'");
        assert_eq!(freq, 0, "expected if_shift freq default=0");
    });
}

#[test]
fn test_set_if_shift() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_if_shift('+', 600)
            .await
            .expect("set_if_shift(+, 600)");
        let (dir, freq) = radio.get_if_shift().await.expect("get_if_shift after set");
        assert_eq!(dir, '+', "if_shift direction mismatch after set");
        assert_eq!(freq, 600, "if_shift freq mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// CTCSS tone number (CN)
// ---------------------------------------------------------------------------

#[test]
fn test_get_ctcss_tone_number() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio
            .get_ctcss_tone_number()
            .await
            .expect("get_ctcss_tone_number");
        // Default: 0
        assert_eq!(v, 0, "expected ctcss_tone_number default=0");
    });
}

#[test]
fn test_set_ctcss_tone_number() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_ctcss_tone_number(8)
            .await
            .expect("set_ctcss_tone_number(8)");
        let v = radio
            .get_ctcss_tone_number()
            .await
            .expect("get_ctcss_tone_number after set");
        assert_eq!(v, 8, "ctcss_tone_number mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// CTCSS on/off (CT)
// ---------------------------------------------------------------------------

#[test]
fn test_get_ctcss() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_ctcss().await.expect("get_ctcss");
        assert!(!v, "expected ctcss default=false");
    });
}

#[test]
fn test_set_ctcss() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_ctcss(true).await.expect("set_ctcss(true)");
        let v = radio.get_ctcss().await.expect("get_ctcss after set");
        assert!(v, "expected ctcss=true after set");
    });
}

// ---------------------------------------------------------------------------
// Tone number (TN)
// ---------------------------------------------------------------------------

#[test]
fn test_get_tone_number() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_tone_number().await.expect("get_tone_number");
        // Default: 0
        assert_eq!(v, 0, "expected tone_number default=0");
    });
}

#[test]
fn test_set_tone_number() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_tone_number(5).await.expect("set_tone_number(5)");
        let v = radio
            .get_tone_number()
            .await
            .expect("get_tone_number after set");
        assert_eq!(v, 5, "tone_number mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Tone on/off (TO)
// ---------------------------------------------------------------------------

#[test]
fn test_get_tone() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_tone().await.expect("get_tone");
        assert!(!v, "expected tone default=false");
    });
}

#[test]
fn test_set_tone() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_tone(true).await.expect("set_tone(true)");
        let v = radio.get_tone().await.expect("get_tone after set");
        assert!(v, "expected tone=true after set");
    });
}

// ---------------------------------------------------------------------------
// Beat cancel mode (BC)
// ---------------------------------------------------------------------------

#[test]
fn test_set_beat_cancel() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_beat_cancel(1).await.expect("set_beat_cancel(1)");
        let v = radio.get_beat_cancel().await.expect("get_beat_cancel after set");
        assert_eq!(v, 1, "beat_cancel mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Semi break-in delay (SD)
// ---------------------------------------------------------------------------

#[test]
fn test_get_semi_break_in_delay() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio
            .get_semi_break_in_delay()
            .await
            .expect("get_semi_break_in_delay");
        // Default: 0
        assert_eq!(v, 0, "expected semi_break_in_delay default=0");
    });
}

#[test]
fn test_set_semi_break_in_delay() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_semi_break_in_delay(200)
            .await
            .expect("set_semi_break_in_delay(200)");
        let v = radio
            .get_semi_break_in_delay()
            .await
            .expect("get_semi_break_in_delay after set");
        assert_eq!(v, 200, "semi_break_in_delay mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Auto information (AI)
// ---------------------------------------------------------------------------

#[test]
fn test_set_auto_info() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_auto_info(1).await.expect("set_auto_info(1)");
    });
}

// ---------------------------------------------------------------------------
// MIC up / down (UP / DN — write-only, adjusts VFO A by 10 Hz)
// ---------------------------------------------------------------------------

#[test]
fn test_mic_up() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.mic_up().await.expect("mic_up");
    });
}

#[test]
fn test_mic_down() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.mic_down().await.expect("mic_down");
    });
}

// ---------------------------------------------------------------------------
// Send CW (KY) — write-only
// ---------------------------------------------------------------------------

#[test]
fn test_send_cw() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.send_cw("CQ").await.expect("send_cw");
    });
}

// ---------------------------------------------------------------------------
// Voice recall (VR) — write-only
// ---------------------------------------------------------------------------

#[test]
fn test_voice_recall() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.voice_recall(1).await.expect("voice_recall(1)");
    });
}

// ---------------------------------------------------------------------------
// System reset (SR) — write-only
// ---------------------------------------------------------------------------

#[test]
fn test_reset() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        // Partial reset (false → "1")
        radio.reset(false).await.expect("reset(false)");
    });
}
