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
