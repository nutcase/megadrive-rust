use megadrive_core::vdp::{Vdp, VideoStandard};

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
