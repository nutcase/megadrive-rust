use megadrive_core::audio::AudioBus;

#[test]
fn writes_ym2612_registers_via_address_and_data_ports() {
    let mut audio = AudioBus::new();

    audio.write_ym2612(0, 0x22);
    audio.write_ym2612(1, 0x0F);
    audio.write_ym2612(2, 0x2B);
    audio.write_ym2612(3, 0x80);

    assert_eq!(audio.ym2612().register(0, 0x22), 0x0F);
    assert_eq!(audio.ym2612().register(1, 0x2B), 0x80);
}

#[test]
fn ym2612_status_reports_busy_for_short_window_after_data_write() {
    let mut audio = AudioBus::new();
    audio.write_ym2612(0, 0x22);
    audio.write_ym2612(1, 0x0F);
    assert_eq!(audio.read_ym2612(0) & 0x80, 0x80);

    audio.step_z80_cycles(8);
    assert_eq!(audio.read_ym2612(0) & 0x80, 0x80);

    audio.step_z80_cycles(8);
    assert_eq!(audio.read_ym2612(0) & 0x80, 0x00);
}

#[test]
fn ym2612_timer_a_sets_status_bit0_when_enabled() {
    let mut audio = AudioBus::new();
    // Timer A = 1023 => shortest period in this model.
    audio.write_ym2612(0, 0x24);
    audio.write_ym2612(1, 0xFF);
    audio.write_ym2612(0, 0x25);
    audio.write_ym2612(1, 0x03);
    // Load + enable timer A.
    audio.write_ym2612(0, 0x27);
    audio.write_ym2612(1, 0x05);

    // Advance enough Z80 cycles to overflow timer A at least once.
    audio.step_z80_cycles(80);
    assert_ne!(audio.read_ym2612(0) & 0x01, 0);

    // Reset timer A status bit.
    audio.write_ym2612(0, 0x27);
    audio.write_ym2612(1, 0x15);
    assert_eq!(audio.read_ym2612(0) & 0x01, 0);
}

#[test]
fn captures_psg_writes() {
    let mut audio = AudioBus::new();
    audio.write_psg(0x9F);
    assert_eq!(audio.psg().last_data(), 0x9F);
    assert_eq!(audio.psg().writes(), 1);
}

#[test]
fn generates_silence_samples_without_psg_writes() {
    let mut audio = AudioBus::new();
    audio.step(2_000);
    assert!(audio.pending_samples() > 0);
    let samples = audio.drain_samples(64);
    assert!(samples.iter().all(|&s| s == 0));
}

#[test]
fn generates_nonzero_samples_after_psg_write() {
    let mut audio = AudioBus::new();
    audio.write_psg(0x90); // low attenuation -> larger amplitude
    audio.step(2_000);

    let samples = audio.drain_samples(64);
    assert!(!samples.is_empty());
    assert!(samples.iter().any(|&s| s > 0));
    assert!(samples.iter().any(|&s| s < 0));
}

#[test]
fn psg_latch_and_data_bytes_update_tone_period() {
    let mut audio = AudioBus::new();
    // Latch tone 0 low nibble = 0xA.
    audio.write_psg(0x8A);
    // Data byte sets high bits = 0x12.
    audio.write_psg(0x12);
    assert_eq!(audio.psg().tone_period(0), 0x12A);
}

#[test]
fn psg_noise_latch_updates_control_register() {
    let mut audio = AudioBus::new();
    // Latch noise control: white noise + clock mode 2.
    audio.write_psg(0xE6);
    assert_eq!(audio.psg().noise_control(), 0x06);
}

#[test]
fn ym2612_dac_outputs_pcm_when_enabled() {
    let mut audio = AudioBus::new();
    audio.write_ym2612(0, 0x2B);
    audio.write_ym2612(1, 0x80);
    audio.write_ym2612(0, 0x2A);
    audio.write_ym2612(1, 0xFF);
    audio.step(2_000);

    let samples = audio.drain_samples(64);
    assert!(!samples.is_empty());
    assert!(audio.ym2612().dac_enabled());
    assert!(samples.iter().all(|&s| s > 0));
}

#[test]
fn ym2612_dac_is_silent_when_disabled() {
    let mut audio = AudioBus::new();
    audio.write_ym2612(0, 0x2A);
    audio.write_ym2612(1, 0xFF);
    audio.step(2_000);

    let samples = audio.drain_samples(64);
    assert!(!samples.is_empty());
    assert!(samples.iter().all(|&s| s == 0));
}

#[test]
fn ym2612_dac_respects_channel6_pan() {
    let mut audio = AudioBus::new();
    // CH6 pan: left only (bank1 reg B6).
    audio.write_ym2612(2, 0xB6);
    audio.write_ym2612(3, 0x80);
    audio.write_ym2612(0, 0x2B);
    audio.write_ym2612(1, 0x80);
    audio.write_ym2612(0, 0x2A);
    audio.write_ym2612(1, 0xFF);
    audio.step(2_000);

    let samples = audio.drain_samples(128);
    assert!(!samples.is_empty());
    let mut left_nonzero = false;
    let mut right_nonzero = false;
    for pair in samples.chunks_exact(2) {
        if pair[0] != 0 {
            left_nonzero = true;
        }
        if pair[1] != 0 {
            right_nonzero = true;
        }
    }
    assert!(left_nonzero);
    assert!(!right_nonzero);
}

#[test]
fn ym2612_key_on_generates_nonzero_without_dac() {
    let mut audio = AudioBus::new();
    // CH1 FNUM/BLOCK
    audio.write_ym2612(0, 0xA0);
    audio.write_ym2612(1, 0x98);
    audio.write_ym2612(0, 0xA4);
    audio.write_ym2612(1, 0x22);
    // Key on CH1
    audio.write_ym2612(0, 0x28);
    audio.write_ym2612(1, 0xF0);
    audio.step(2_000);

    let samples = audio.drain_samples(128);
    assert!(!samples.is_empty());
    assert!(audio.ym2612().channel_key_on(0));
    assert!(samples.iter().any(|&s| s != 0));
}

#[test]
fn ym2612_key_off_silences_channel() {
    let mut audio = AudioBus::new();
    audio.write_ym2612(0, 0xA0);
    audio.write_ym2612(1, 0xA0);
    audio.write_ym2612(0, 0xA4);
    audio.write_ym2612(1, 0x24);
    audio.write_ym2612(0, 0x28);
    audio.write_ym2612(1, 0xF0);
    audio.step(2_000);
    let _ = audio.drain_samples(128);

    audio.write_ym2612(0, 0x28);
    audio.write_ym2612(1, 0x00);
    audio.step(2_000);
    let samples = audio.drain_samples(128);
    assert!(!samples.is_empty());
    assert!(samples.iter().all(|&s| s == 0));
}

#[test]
fn supports_runtime_output_sample_rate_configuration() {
    let mut audio = AudioBus::new();
    audio.set_output_sample_rate_hz(22_050);
    assert_eq!(audio.output_sample_rate_hz(), 22_050);

    audio.step(7_670_454);
    assert!((audio.pending_samples() as i32 - (22_050 * 2)).abs() <= 2);
}

#[test]
fn output_is_stereo_interleaved() {
    let mut audio = AudioBus::new();
    assert_eq!(audio.output_channels(), 2);
    audio.step(2_000);
    assert_eq!(audio.pending_samples() % 2, 0);
}

#[test]
fn ym_pan_register_routes_channel_to_left_only() {
    let mut audio = AudioBus::new();
    // CH1 FNUM/BLOCK
    audio.write_ym2612(0, 0xA0);
    audio.write_ym2612(1, 0x98);
    audio.write_ym2612(0, 0xA4);
    audio.write_ym2612(1, 0x22);
    // CH1 pan: left only
    audio.write_ym2612(0, 0xB4);
    audio.write_ym2612(1, 0x80);
    // Key on CH1
    audio.write_ym2612(0, 0x28);
    audio.write_ym2612(1, 0xF0);
    audio.step(2_000);

    let samples = audio.drain_samples(256);
    assert!(!samples.is_empty());
    let mut left_nonzero = false;
    let mut right_nonzero = false;
    for pair in samples.chunks_exact(2) {
        if pair[0] != 0 {
            left_nonzero = true;
        }
        if pair[1] != 0 {
            right_nonzero = true;
        }
    }
    assert!(left_nonzero);
    assert!(!right_nonzero);
}
