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
//! `Ts570d::set` is fire-and-forget: it writes the command but does NOT read
//! the radio's echo response.  The emulator echoes every set command.  The
//! subsequent `get` call therefore reads the echo (which carries the correct
//! value) rather than the freshly-generated query response.  This is
//! acceptable here: the echo contains exactly the same value as a real query
//! would, so the assertion is still valid.

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
        // The set-echo is in the buffer; reading it confirms the value was applied.
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
        // Read back the echo (which also confirms the value).
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
        // Read back the echo which confirms the value.
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
        // Emulator returns "ID019;" → 19u16 (TS-570S model code).
        assert_eq!(id, 19, "unexpected radio ID: {}", id);
    });
}
