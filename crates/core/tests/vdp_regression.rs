use megadrive_core::vdp::{Vdp, VideoStandard};

fn capture_v_counter_on_line_starts(vdp: &mut Vdp, line_count: usize) -> Vec<u8> {
    let [v0, h0] = vdp.read_hv_counter().to_be_bytes();
    let mut out = Vec::with_capacity(line_count);
    out.push(v0);
    let mut prev_h = h0;

    for _ in 0..2_000_000 {
        if out.len() >= line_count {
            break;
        }
        vdp.step(1);
        let [v, h] = vdp.read_hv_counter().to_be_bytes();
        if h < prev_h {
            out.push(v);
        }
        prev_h = h;
    }

    assert_eq!(
        out.len(),
        line_count,
        "expected to capture {line_count} line starts"
    );
    out
}

#[test]
fn supports_vram_read_write() {
    let mut vdp = Vdp::new();
    vdp.write_vram_u8(0x1234, 0xAB);
    assert_eq!(vdp.read_vram_u8(0x1234), 0xAB);
}

#[test]
fn supports_control_and_data_ports_for_vram_write() {
    let mut vdp = Vdp::new();
    vdp.write_control_port(0x4000);
    vdp.write_control_port(0x0000);
    vdp.write_data_port(0xABCD);
    assert_eq!(vdp.read_vram_u8(0), 0xAB);
    assert_eq!(vdp.read_vram_u8(1), 0xCD);
}

#[test]
fn display_disable_register_blacks_out_frame() {
    let mut vdp = Vdp::new();
    // Register 1 = 0x00 (display disable)
    vdp.write_control_port(0x8100);
    let frame_ready = vdp.step(130_000);
    assert!(frame_ready);
    assert!(vdp.frame_buffer().iter().all(|&b| b == 0));
}

#[test]
fn supports_pal_video_standard_timing() {
    let mut vdp = Vdp::with_video_standard(VideoStandard::Pal);
    assert_eq!(vdp.video_standard(), VideoStandard::Pal);
    assert_eq!(vdp.total_lines(), 313);

    // PAL frame budget is larger than NTSC.
    assert!(!vdp.step(127_800));
    assert!(vdp.step(30_000));
}

#[test]
fn dma_copy_updates_line0_latch_when_triggered_at_frame_start() {
    let mut vdp = Vdp::new();
    // Register 1: display + DMA enable.
    vdp.write_control_port(0x8150);
    // Auto-increment = 1 byte.
    vdp.write_control_port(0x8F01);
    // DMA length = 4 bytes.
    vdp.write_control_port(0x9304);
    vdp.write_control_port(0x9400);
    // DMA source = 0x0100.
    vdp.write_control_port(0x9500);
    vdp.write_control_port(0x9601);
    // DMA mode = copy.
    vdp.write_control_port(0x97C0);

    vdp.write_vram_u8(0x0100, 0xDE);
    vdp.write_vram_u8(0x0101, 0xAD);
    vdp.write_vram_u8(0x0102, 0xBE);
    vdp.write_vram_u8(0x0103, 0xEF);

    // VRAM write DMA command @ 0x0200 (code with DMA bit set).
    vdp.write_control_port(0x4200);
    vdp.write_control_port(0x0080);

    assert_eq!(vdp.read_vram_u8(0x0200), 0xDE);
    assert_eq!(vdp.read_vram_u8(0x0201), 0xAD);
    assert_eq!(vdp.read_vram_u8(0x0202), 0xBE);
    assert_eq!(vdp.read_vram_u8(0x0203), 0xEF);
    assert_eq!(vdp.line_vram_u8(0, 0x0200), 0xDE);
    assert_eq!(vdp.line_vram_u8(0, 0x0201), 0xAD);
    assert_eq!(vdp.line_vram_u8(0, 0x0202), 0xBE);
    assert_eq!(vdp.line_vram_u8(0, 0x0203), 0xEF);
}

#[test]
fn hv_counter_resets_h_when_v_increments_to_next_line() {
    let mut vdp = Vdp::new();
    let mut prev = vdp.read_hv_counter();
    let mut saw_line_increment = false;

    for _ in 0..2_000 {
        vdp.step(1);
        let cur = vdp.read_hv_counter();
        let [prev_v, _prev_h] = prev.to_be_bytes();
        let [cur_v, cur_h] = cur.to_be_bytes();

        if cur_v != prev_v {
            assert_eq!(cur_v, prev_v.wrapping_add(1));
            assert!(cur_h <= 2, "H counter should reset near 0 at line start");
            saw_line_increment = true;
            break;
        }
        prev = cur;
    }

    assert!(saw_line_increment, "expected a scanline transition");
}

#[test]
fn hblank_status_turns_on_late_in_line_and_off_at_next_line_start() {
    let mut vdp = Vdp::new();
    let mut entered_hblank_on_line0 = false;

    for _ in 0..2_000 {
        let status = vdp.read_control_port();
        let [v, _h] = vdp.read_hv_counter().to_be_bytes();
        if v == 0 && (status & 0x0004) != 0 {
            entered_hblank_on_line0 = true;
            break;
        }
        vdp.step(1);
    }
    assert!(entered_hblank_on_line0, "expected HBlank during line 0");

    let mut exited_hblank_on_line1 = false;
    for _ in 0..2_000 {
        vdp.step(1);
        let status = vdp.read_control_port();
        let [v, _h] = vdp.read_hv_counter().to_be_bytes();
        if v == 1 && (status & 0x0004) == 0 {
            exited_hblank_on_line1 = true;
            break;
        }
    }
    assert!(
        exited_hblank_on_line1,
        "expected HBlank to clear at the next line start"
    );
}

#[test]
fn hblank_status_h40_turns_on_near_b3_boundary() {
    let mut vdp = Vdp::new();
    // Reg 12 bit0 = 1 => H40 mode.
    vdp.write_control_port(0x8C01);

    let mut entered_hblank_on_line0 = false;
    for _ in 0..4_000 {
        let status = vdp.read_control_port();
        let [v, h] = vdp.read_hv_counter().to_be_bytes();
        // Skip the early wrapped hblank tail (H small values just after line start)
        // and assert the main hblank entry near 0xB3.
        if v == 0 && h > 0x20 && (status & 0x0004) != 0 {
            assert!(h >= 0xB0, "expected H40 hblank near 0xB3, got {h:02X}");
            entered_hblank_on_line0 = true;
            break;
        }
        vdp.step(1);
    }
    assert!(entered_hblank_on_line0, "expected H40 HBlank during line 0");
}

#[test]
fn h_interrupt_counter_continues_through_vblank_lines() {
    let mut vdp = Vdp::new();
    // Enable H-INT, period = 4 lines (R10=3).
    vdp.write_control_port(0x8010);
    vdp.write_control_port(0x8A03);

    fn first_hint_line_for_frame(vdp: &mut Vdp, target_frame: u64) -> Option<u8> {
        let mut entered_target_frame = false;
        let mut cleared_stale = false;
        for _ in 0..600_000 {
            if vdp.frame_count() == target_frame {
                entered_target_frame = true;
                if !cleared_stale {
                    while vdp.pending_interrupt_level() == Some(4) {
                        vdp.acknowledge_interrupt(4);
                    }
                    cleared_stale = true;
                } else if vdp.pending_interrupt_level() == Some(4) {
                    let [line, _h] = vdp.read_hv_counter().to_be_bytes();
                    vdp.acknowledge_interrupt(4);
                    return Some(line);
                }
            } else if entered_target_frame {
                // Moved past target frame without finding the first H-INT.
                return None;
            }
            vdp.step(1);
        }
        None
    }

    let frame0 = first_hint_line_for_frame(&mut vdp, 0).expect("frame0 hint");
    let frame1 = first_hint_line_for_frame(&mut vdp, 1).expect("frame1 hint");
    let frame2 = first_hint_line_for_frame(&mut vdp, 2).expect("frame2 hint");
    let frame3 = first_hint_line_for_frame(&mut vdp, 3).expect("frame3 hint");

    // NTSC has 262 total lines. With a 4-line H-INT period, frame phase
    // advances across frames because the counter runs through vblank lines.
    // We only require non-constant per-frame positions here.
    assert_ne!(frame0, frame1);
    let mut uniq = std::collections::BTreeSet::new();
    uniq.insert(frame0);
    uniq.insert(frame1);
    uniq.insert(frame2);
    uniq.insert(frame3);
    assert!(
        uniq.len() >= 2,
        "expected frame-to-frame phase drift, got {uniq:?}"
    );
}

#[test]
fn h_interrupt_asserts_near_hblank_not_line_start() {
    let mut vdp = Vdp::new();
    // Enable H-INT every line.
    vdp.write_control_port(0x8010);
    vdp.write_control_port(0x8A00);

    let mut reached_line_1_start = false;
    for _ in 0..5_000 {
        let [v, h] = vdp.read_hv_counter().to_be_bytes();
        if v == 1 && h <= 2 {
            reached_line_1_start = true;
            break;
        }
        vdp.step(1);
    }
    assert!(reached_line_1_start, "expected to reach line 1 start");
    if vdp.pending_interrupt_level() == Some(4) {
        vdp.acknowledge_interrupt(4);
    }
    assert_eq!(vdp.pending_interrupt_level(), None);

    let mut asserted = None;
    for _ in 0..5_000 {
        if vdp.pending_interrupt_level() == Some(4) {
            asserted = Some(vdp.read_hv_counter());
            break;
        }
        vdp.step(1);
    }

    let hv = asserted.expect("expected H-INT pending");
    let [v, h] = hv.to_be_bytes();
    assert_eq!(v, 1);
    assert!(h >= 120, "expected H counter near hblank, got {h}");
}

#[test]
fn h_interrupt_asserts_near_hblank_not_line_start_in_h40() {
    let mut vdp = Vdp::new();
    // Reg 12 bit0 = 1 => H40 mode.
    vdp.write_control_port(0x8C01);
    // Enable H-INT every line.
    vdp.write_control_port(0x8010);
    vdp.write_control_port(0x8A00);

    let mut reached_line_1_start = false;
    for _ in 0..6_000 {
        let [v, h] = vdp.read_hv_counter().to_be_bytes();
        if v == 1 && h <= 2 {
            reached_line_1_start = true;
            break;
        }
        vdp.step(1);
    }
    assert!(reached_line_1_start, "expected to reach line 1 start");
    if vdp.pending_interrupt_level() == Some(4) {
        vdp.acknowledge_interrupt(4);
    }
    assert_eq!(vdp.pending_interrupt_level(), None);

    let mut asserted = None;
    for _ in 0..6_000 {
        if vdp.pending_interrupt_level() == Some(4) {
            asserted = Some(vdp.read_hv_counter());
            break;
        }
        vdp.step(1);
    }

    let hv = asserted.expect("expected H-INT pending in H40");
    let [v, h] = hv.to_be_bytes();
    assert_eq!(v, 1);
    assert!(h >= 0xB0, "expected H40 H counter near hblank, got {h:02X}");
}

#[test]
fn hv_counter_h_component_tracks_half_dot_range() {
    let mut vdp = Vdp::new();
    let mut saw_hblank_edge = false;
    let mut saw_high_wrap_band = false;
    let mut saw_gap_value = false;
    let mut line_advanced = false;
    let mut prev_v = vdp.read_hv_counter().to_be_bytes()[0];

    for _ in 0..4_000 {
        let [v, h] = vdp.read_hv_counter().to_be_bytes();
        if v != prev_v {
            line_advanced = true;
            break;
        }
        if (0x90..=0x93).contains(&h) {
            saw_hblank_edge = true;
        }
        if h >= 0xE9 {
            saw_high_wrap_band = true;
        }
        if (0x94..=0xE8).contains(&h) {
            saw_gap_value = true;
        }
        vdp.step(1);
        prev_v = v;
    }

    assert!(line_advanced, "expected at least one scanline transition");
    assert!(saw_hblank_edge, "expected to observe H32 hblank edge band");
    assert!(saw_high_wrap_band, "expected to observe H32 high wrap band");
    assert!(!saw_gap_value, "H32 H-counter should skip gap 0x94..=0xE8");
}

#[test]
fn hv_counter_h40_component_tracks_discontinuous_range() {
    let mut vdp = Vdp::new();
    // Reg 12 bit0 = 1 => H40 mode.
    vdp.write_control_port(0x8C01);

    let mut saw_hblank_edge = false;
    let mut saw_high_wrap_band = false;
    let mut saw_gap_value = false;
    let mut line_advanced = false;
    let mut prev_v = vdp.read_hv_counter().to_be_bytes()[0];

    for _ in 0..4_000 {
        let [v, h] = vdp.read_hv_counter().to_be_bytes();
        if v != prev_v {
            line_advanced = true;
            break;
        }
        if (0xB0..=0xB3).contains(&h) {
            saw_hblank_edge = true;
        }
        if h >= 0xE4 {
            saw_high_wrap_band = true;
        }
        if (0xB7..=0xE3).contains(&h) {
            saw_gap_value = true;
        }
        vdp.step(1);
        prev_v = v;
    }

    assert!(line_advanced, "expected at least one scanline transition");
    assert!(saw_hblank_edge, "expected to observe H40 hblank edge band");
    assert!(saw_high_wrap_band, "expected to observe H40 high wrap band");
    assert!(!saw_gap_value, "H40 H-counter should skip gap 0xB7..=0xE3");
}

#[test]
fn ntsc_v28_v_counter_uses_ea_to_e5_wrap_pattern() {
    let mut vdp = Vdp::new();
    assert_eq!(vdp.total_lines(), 262);
    let v = capture_v_counter_on_line_starts(&mut vdp, 262);

    assert_eq!(v[0], 0x00);
    assert_eq!(v[0xEA], 0xEA);
    assert_eq!(v[0xEB], 0xE5);
    assert_eq!(v[261], 0xFF);
}

#[test]
fn pal_v28_v_counter_uses_ff_00_02_ca_pattern() {
    let mut vdp = Vdp::with_video_standard(VideoStandard::Pal);
    assert_eq!(vdp.total_lines(), 313);
    let v = capture_v_counter_on_line_starts(&mut vdp, 313);

    assert_eq!(v[255], 0xFF);
    assert_eq!(v[256], 0x00);
    assert_eq!(v[258], 0x02);
    assert_eq!(v[259], 0xCA);
    assert_eq!(v[312], 0xFF);
}

#[test]
fn pal_v30_v_counter_uses_ff_00_0a_d2_pattern() {
    let mut vdp = Vdp::with_video_standard(VideoStandard::Pal);
    // Register 1 bit3 enables 240-line (V30) mode.
    vdp.write_control_port(0x8148);
    assert_eq!(vdp.total_lines(), 313);
    let v = capture_v_counter_on_line_starts(&mut vdp, 313);

    assert_eq!(v[255], 0xFF);
    assert_eq!(v[256], 0x00);
    assert_eq!(v[266], 0x0A);
    assert_eq!(v[267], 0xD2);
    assert_eq!(v[312], 0xFF);
}

#[test]
fn vint_latches_in_vblank_even_if_disabled_then_appears_when_enabled() {
    let mut vdp = Vdp::new();
    // Keep V-INT disabled (reg1 default: display on only).
    vdp.write_control_port(0x8140);

    // Reach first VBlank line start.
    let mut reached_vblank = false;
    for _ in 0..200_000 {
        let [v, h] = vdp.read_hv_counter().to_be_bytes();
        if v == 224 && h <= 2 {
            reached_vblank = true;
            break;
        }
        vdp.step(1);
    }
    assert!(reached_vblank, "expected to reach NTSC VBlank start");

    // Latch exists, but IRQ output is gated while disabled.
    assert_eq!(vdp.pending_interrupt_level(), None);

    // Enabling during the same VBlank should expose pending V-INT.
    vdp.write_control_port(0x8160);
    assert_eq!(vdp.pending_interrupt_level(), Some(6));
}

#[test]
fn vint_output_is_gated_by_enable_bit_without_clearing_latch() {
    let mut vdp = Vdp::new();
    vdp.write_control_port(0x8160);

    // Reach first VBlank line start with V-INT enabled.
    for _ in 0..200_000 {
        let [v, h] = vdp.read_hv_counter().to_be_bytes();
        if v == 224 && h <= 2 {
            break;
        }
        vdp.step(1);
    }
    assert_eq!(vdp.pending_interrupt_level(), Some(6));

    // Disable V-INT output: pending latch should be hidden.
    vdp.write_control_port(0x8140);
    assert_eq!(vdp.pending_interrupt_level(), None);

    // Re-enable: same pending latch should become visible again.
    vdp.write_control_port(0x8160);
    assert_eq!(vdp.pending_interrupt_level(), Some(6));
}

#[test]
fn hint_latches_at_hblank_even_if_disabled_then_appears_when_enabled() {
    let mut vdp = Vdp::new();
    // H-INT every line, but keep H-INT output disabled.
    vdp.write_control_port(0x8000);
    vdp.write_control_port(0x8A00);

    // Reach line 1 HBlank while H-INT output is disabled.
    let mut reached_line1_hblank = false;
    for _ in 0..10_000 {
        let [v, h] = vdp.read_hv_counter().to_be_bytes();
        let status = vdp.read_control_port();
        if v == 1 && h >= 1 && (status & 0x0004) != 0 {
            reached_line1_hblank = true;
            break;
        }
        vdp.step(1);
    }
    assert!(reached_line1_hblank, "expected to reach line 1 hblank");
    assert_eq!(vdp.pending_interrupt_level(), None);

    // Enabling H-INT during the same frame should expose the latched request.
    vdp.write_control_port(0x8010);
    assert_eq!(vdp.pending_interrupt_level(), Some(4));
}

#[test]
fn hint_output_is_gated_by_enable_bit_without_clearing_latch() {
    let mut vdp = Vdp::new();
    vdp.write_control_port(0x8010);
    vdp.write_control_port(0x8A00);

    // Wait until an H-INT becomes pending.
    let mut saw_hint = false;
    for _ in 0..10_000 {
        if vdp.pending_interrupt_level() == Some(4) {
            saw_hint = true;
            break;
        }
        vdp.step(1);
    }
    assert!(saw_hint, "expected H-INT pending");

    // Disable output: pending latch should be hidden.
    vdp.write_control_port(0x8000);
    assert_eq!(vdp.pending_interrupt_level(), None);

    // Re-enable: same pending latch should become visible again.
    vdp.write_control_port(0x8010);
    assert_eq!(vdp.pending_interrupt_level(), Some(4));
}

#[test]
fn hint_r10_write_applies_on_next_reload_not_mid_countdown() {
    let mut vdp = Vdp::new();
    vdp.write_control_port(0x8010); // H-INT enable
    vdp.write_control_port(0x8A03); // period = 4 lines (R10=3)

    // Skip startup artifacts (line 0) and synchronize at line 1 start.
    let mut synced_line1_start = false;
    for _ in 0..20_000 {
        if vdp.pending_interrupt_level() == Some(4) {
            vdp.acknowledge_interrupt(4);
        }
        let [v, h] = vdp.read_hv_counter().to_be_bytes();
        if v == 1 && h <= 2 {
            synced_line1_start = true;
            break;
        }
        vdp.step(1);
    }
    assert!(
        synced_line1_start,
        "expected to synchronize at line 1 start"
    );

    let next_hint_line = |vdp: &mut Vdp| -> u8 {
        for _ in 0..200_000 {
            if vdp.pending_interrupt_level() == Some(4) {
                let [line, _h] = vdp.read_hv_counter().to_be_bytes();
                vdp.acknowledge_interrupt(4);
                return line;
            }
            vdp.step(1);
        }
        panic!("expected H-INT");
    };

    let line0 = next_hint_line(&mut vdp);
    let line1 = next_hint_line(&mut vdp);
    assert_eq!(
        line1.wrapping_sub(line0),
        4,
        "baseline period from R10=3 should be 4 lines (line0={line0:#04X}, line1={line1:#04X})"
    );

    // Change R10 to 0 (every line). This should not affect the current
    // countdown that was already reloaded at the previous H-INT line start.
    vdp.write_control_port(0x8A00);

    let line2 = next_hint_line(&mut vdp);
    assert_eq!(
        line2.wrapping_sub(line1),
        4,
        "R10 update must not apply mid-countdown (line1={line1:#04X}, line2={line2:#04X})"
    );

    // New R10 should be observed after the next reload.
    let line3 = next_hint_line(&mut vdp);
    assert_eq!(
        line3.wrapping_sub(line2),
        1,
        "R10 update should apply on the following reload (line2={line2:#04X}, line3={line3:#04X})"
    );
}
