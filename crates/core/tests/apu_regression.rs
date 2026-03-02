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
    // CH1 OP4 SL/RR: fast release.
    audio.write_ym2612(0, 0x8C);
    audio.write_ym2612(1, 0x0F);
    audio.write_ym2612(0, 0x28);
    audio.write_ym2612(1, 0xF0);
    audio.step(2_000);
    let _ = audio.drain_samples(128);

    audio.write_ym2612(0, 0x28);
    audio.write_ym2612(1, 0x00);
    audio.step(2_000);
    let release_samples = audio.drain_samples(128);
    assert!(!release_samples.is_empty());
    assert!(release_samples.iter().any(|&s| s != 0));

    audio.step(120_000);
    let tail_samples = audio.drain_samples(4096);
    assert!(!tail_samples.is_empty());
    let tail_quiet = tail_samples.iter().rev().take(256).all(|&s| s == 0);
    assert!(tail_quiet);
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

#[test]
fn ym_carrier_multiple_register_scales_channel_frequency() {
    let mut audio = AudioBus::new();
    // CH1 FNUM/BLOCK = 0x300 @ block 4.
    audio.write_ym2612(0, 0xA0);
    audio.write_ym2612(1, 0x00);
    audio.write_ym2612(0, 0xA4);
    audio.write_ym2612(1, 0x23);

    let base_hz = audio.ym2612().channel_frequency_hz_debug(0);
    // CH1 OP4 DT/MUL (reg 0x3C) MUL=4.
    audio.write_ym2612(0, 0x3C);
    audio.write_ym2612(1, 0x04);

    assert_eq!(audio.ym2612().channel_carrier_mul(0), 0x04);
    let scaled_hz = audio.ym2612().channel_frequency_hz_debug(0);
    assert!((scaled_hz / base_hz - 4.0).abs() < 0.05);
}

#[test]
fn ym_carrier_total_level_can_mute_channel_output() {
    let mut audio = AudioBus::new();
    audio.write_ym2612(0, 0xA0);
    audio.write_ym2612(1, 0x98);
    audio.write_ym2612(0, 0xA4);
    audio.write_ym2612(1, 0x22);

    // CH1 OP4 TL = 0 (loud)
    audio.write_ym2612(0, 0x4C);
    audio.write_ym2612(1, 0x00);
    audio.write_ym2612(0, 0x28);
    audio.write_ym2612(1, 0xF0);
    audio.step(2_000);
    let loud = audio.drain_samples(128);
    assert!(loud.iter().any(|&s| s != 0));

    // Key off before changing TL for deterministic restart.
    audio.write_ym2612(0, 0x28);
    audio.write_ym2612(1, 0x00);
    audio.step(200);
    let _ = audio.drain_samples(64);

    // CH1 OP4 TL = 127 (silent in this model)
    audio.write_ym2612(0, 0x4C);
    audio.write_ym2612(1, 0x7F);
    assert_eq!(audio.ym2612().channel_carrier_tl(0), 0x7F);
    audio.write_ym2612(0, 0x28);
    audio.write_ym2612(1, 0xF0);
    audio.step(2_000);
    let muted = audio.drain_samples(128);
    assert!(muted.iter().all(|&s| s == 0));
}

#[test]
fn psg_period_zero_uses_1024_divider_frequency() {
    let mut audio = AudioBus::new();
    // Tone 0 period = 0x000.
    audio.write_psg(0x80);
    audio.write_psg(0x00);

    assert_eq!(audio.psg().tone_period(0), 0x000);
    let expected = 3_579_545.0 / (32.0 * 1024.0);
    let got = audio.psg().tone_frequency_hz_debug(0);
    assert!(
        (got - expected).abs() < 0.01,
        "expected {expected}, got {got}"
    );
}

#[test]
fn ym_algorithm_and_feedback_registers_are_tracked() {
    let mut audio = AudioBus::new();
    // CH1 algorithm=5 feedback=6
    audio.write_ym2612(0, 0xB0);
    audio.write_ym2612(1, 0x35);

    assert_eq!(audio.ym2612().channel_algorithm_feedback(0), (0x05, 0x06));
}

#[test]
fn ym_feedback_setting_changes_waveform_after_key_on_restart() {
    let mut audio = AudioBus::new();

    // CH1 base pitch.
    audio.write_ym2612(0, 0xA0);
    audio.write_ym2612(1, 0x98);
    audio.write_ym2612(0, 0xA4);
    audio.write_ym2612(1, 0x22);
    // Ensure audible carrier level.
    audio.write_ym2612(0, 0x4C);
    audio.write_ym2612(1, 0x00);

    // Algorithm 0, feedback 0.
    audio.write_ym2612(0, 0xB0);
    audio.write_ym2612(1, 0x00);
    audio.write_ym2612(0, 0x28);
    audio.write_ym2612(1, 0xF0);
    audio.step(3_000);
    let baseline = audio.drain_samples(128);

    // Restart channel to reset phase, then raise feedback.
    audio.write_ym2612(0, 0x28);
    audio.write_ym2612(1, 0x00);
    audio.step(200);
    let _ = audio.drain_samples(64);
    audio.write_ym2612(0, 0xB0);
    audio.write_ym2612(1, 0x38); // algorithm 0 + max feedback (7)
    audio.write_ym2612(0, 0x28);
    audio.write_ym2612(1, 0xF0);
    audio.step(3_000);
    let feedback = audio.drain_samples(128);

    assert_eq!(baseline.len(), feedback.len());
    assert!(
        baseline != feedback,
        "feedback should alter sample stream for same note"
    );
}

#[test]
fn ym_carrier_envelope_registers_are_tracked() {
    let mut audio = AudioBus::new();
    // CH1 OP4: AR/KS, DR/AM, SR, SL/RR
    audio.write_ym2612(0, 0x5C);
    audio.write_ym2612(1, 0x1F); // AR=31
    audio.write_ym2612(0, 0x6C);
    audio.write_ym2612(1, 0x0E); // DR=14
    audio.write_ym2612(0, 0x7C);
    audio.write_ym2612(1, 0x09); // SR=9
    audio.write_ym2612(0, 0x8C);
    audio.write_ym2612(1, 0xA7); // SL=10 RR=7

    assert_eq!(
        audio.ym2612().channel_envelope_params(0),
        (31, 14, 9, 10, 7)
    );
}

#[test]
fn ym_attack_rate_affects_envelope_ramp_speed() {
    let mut slow = AudioBus::new();
    // CH1 pitch and carrier level.
    slow.write_ym2612(0, 0xA0);
    slow.write_ym2612(1, 0x98);
    slow.write_ym2612(0, 0xA4);
    slow.write_ym2612(1, 0x22);
    slow.write_ym2612(0, 0x4C);
    slow.write_ym2612(1, 0x00);
    // Slow AR=1, keep long sustain.
    slow.write_ym2612(0, 0x5C);
    slow.write_ym2612(1, 0x01);
    slow.write_ym2612(0, 0x6C);
    slow.write_ym2612(1, 0x00);
    slow.write_ym2612(0, 0x7C);
    slow.write_ym2612(1, 0x00);
    slow.write_ym2612(0, 0x8C);
    slow.write_ym2612(1, 0x00);
    slow.write_ym2612(0, 0x28);
    slow.write_ym2612(1, 0xF0);
    slow.step(5_000);
    let slow_env = slow.ym2612().channel_envelope_level(0);

    let mut fast = AudioBus::new();
    // Same setup, but fast AR=31.
    fast.write_ym2612(0, 0xA0);
    fast.write_ym2612(1, 0x98);
    fast.write_ym2612(0, 0xA4);
    fast.write_ym2612(1, 0x22);
    fast.write_ym2612(0, 0x4C);
    fast.write_ym2612(1, 0x00);
    fast.write_ym2612(0, 0x5C);
    fast.write_ym2612(1, 0x1F);
    fast.write_ym2612(0, 0x6C);
    fast.write_ym2612(1, 0x00);
    fast.write_ym2612(0, 0x7C);
    fast.write_ym2612(1, 0x00);
    fast.write_ym2612(0, 0x8C);
    fast.write_ym2612(1, 0x00);
    fast.write_ym2612(0, 0x28);
    fast.write_ym2612(1, 0xF0);
    fast.step(5_000);
    let fast_env = fast.ym2612().channel_envelope_level(0);

    assert!(
        fast_env > slow_env,
        "fast_env={fast_env}, slow_env={slow_env}"
    );
}

#[test]
fn ym_release_rate_affects_envelope_decay_speed_after_keyoff() {
    let mut slow = AudioBus::new();
    slow.write_ym2612(0, 0xA0);
    slow.write_ym2612(1, 0x98);
    slow.write_ym2612(0, 0xA4);
    slow.write_ym2612(1, 0x22);
    slow.write_ym2612(0, 0x4C);
    slow.write_ym2612(1, 0x00);
    slow.write_ym2612(0, 0x8C);
    slow.write_ym2612(1, 0x00); // RR=0 (slow)
    slow.write_ym2612(0, 0x28);
    slow.write_ym2612(1, 0xF0);
    slow.step(4_000);
    let _ = slow.drain_samples(128);
    slow.write_ym2612(0, 0x28);
    slow.write_ym2612(1, 0x00);
    slow.step(20_000);
    let slow_env = slow.ym2612().channel_envelope_level(0);

    let mut fast = AudioBus::new();
    fast.write_ym2612(0, 0xA0);
    fast.write_ym2612(1, 0x98);
    fast.write_ym2612(0, 0xA4);
    fast.write_ym2612(1, 0x22);
    fast.write_ym2612(0, 0x4C);
    fast.write_ym2612(1, 0x00);
    fast.write_ym2612(0, 0x8C);
    fast.write_ym2612(1, 0x0F); // RR=15 (fast)
    fast.write_ym2612(0, 0x28);
    fast.write_ym2612(1, 0xF0);
    fast.step(4_000);
    let _ = fast.drain_samples(128);
    fast.write_ym2612(0, 0x28);
    fast.write_ym2612(1, 0x00);
    fast.step(20_000);
    let fast_env = fast.ym2612().channel_envelope_level(0);

    assert!(
        fast_env < slow_env,
        "fast_env={fast_env}, slow_env={slow_env}"
    );
}
