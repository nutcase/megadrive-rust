use crate::cartridge::Cartridge;
use crate::cpu::{CCR_C, CCR_N, CCR_V, CCR_X, CCR_Z, M68k, SR_INT_MASK, SR_SUPERVISOR};
use crate::memory::MemoryMap;

#[test]
fn executes_move_word_immediate_to_absolute_long() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.w #$ABCD, $00FF0002
    rom[0x100..0x102].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0xABCDu16.to_be_bytes());
    rom[0x104..0x108].copy_from_slice(&0x00FF_0002u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    let cycles = cpu.step(&mut memory);
    assert_eq!(cycles, 16);
    assert_eq!(cpu.pc(), 0x0000_0108);
    assert_eq!(memory.read_u16(0xFF0002), 0xABCD);
}

#[test]
fn executes_move_l_imm_dn_and_move_w_dn_abs_l() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.l #$0000ABCD, d0
    rom[0x100..0x102].copy_from_slice(&0x203Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x0000_ABCDu32.to_be_bytes());
    // move.w d0, $00FF0004
    rom[0x106..0x108].copy_from_slice(&0x33C0u16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x00FF_0004u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);

    assert_eq!(memory.read_u16(0xFF0004), 0xABCD);
    assert_eq!(cpu.pc(), 0x0000_010C);
}

#[test]
fn move_word_supports_immediate_to_dn_and_displacement_addressing() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0030, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0030u32.to_be_bytes());
    // move.w #$ABCD, d0
    rom[0x106..0x108].copy_from_slice(&0x303Cu16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0xABCDu16.to_be_bytes());
    // move.w d0, (2,a0)
    rom[0x10A..0x10C].copy_from_slice(&0x3140u16.to_be_bytes());
    rom[0x10C..0x10E].copy_from_slice(&0x0002u16.to_be_bytes());
    // move.w (2,a0), d1
    rom[0x10E..0x110].copy_from_slice(&0x3228u16.to_be_bytes());
    rom[0x110..0x112].copy_from_slice(&0x0002u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.read_u16(0x00FF_0032), 0xABCD);
    assert_eq!(cpu.d_regs[0] & 0xFFFF, 0xABCD);
    assert_eq!(cpu.d_regs[1] & 0xFFFF, 0xABCD);
}

#[test]
fn move_word_supports_absolute_word_and_long_sources() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    rom[0x20..0x22].copy_from_slice(&0x2468u16.to_be_bytes());

    // move.w #$1357, $00FF0040
    rom[0x100..0x102].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0x1357u16.to_be_bytes());
    rom[0x104..0x108].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // move.w $0020.w, d2
    rom[0x108..0x10A].copy_from_slice(&0x3438u16.to_be_bytes());
    rom[0x10A..0x10C].copy_from_slice(&0x0020u16.to_be_bytes());
    // move.w $00FF0040.l, d3
    rom[0x10C..0x10E].copy_from_slice(&0x3639u16.to_be_bytes());
    rom[0x10E..0x112].copy_from_slice(&0x00FF_0040u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..3 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[2] & 0xFFFF, 0x2468);
    assert_eq!(cpu.d_regs[3] & 0xFFFF, 0x1357);
}

#[test]
fn move_long_supports_displacement_source_and_destination() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0060, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0060u32.to_be_bytes());
    // move.l #$11223344, (4,a0)
    rom[0x106..0x108].copy_from_slice(&0x217Cu16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x1122_3344u32.to_be_bytes());
    rom[0x10C..0x10E].copy_from_slice(&0x0004u16.to_be_bytes());
    // move.l (4,a0), d1
    rom[0x10E..0x110].copy_from_slice(&0x2228u16.to_be_bytes());
    rom[0x110..0x112].copy_from_slice(&0x0004u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..3 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.read_u32(0x00FF_0064), 0x1122_3344);
    assert_eq!(cpu.d_regs[1], 0x1122_3344);
    assert_eq!(cpu.sr() & CCR_Z, 0);
}

#[test]
fn executes_move_byte_immediate_to_absolute_long() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.b #$5A, $00FF0003
    rom[0x100..0x102].copy_from_slice(&0x13FCu16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0x005Au16.to_be_bytes());
    rom[0x104..0x108].copy_from_slice(&0x00FF_0003u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);

    assert_eq!(memory.read_u8(0x00FF_0003), 0x5A);
}

#[test]
fn executes_move_byte_with_predecrement_and_postincrement() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0010, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0010u32.to_be_bytes());
    // moveq #$7F, d0
    rom[0x106..0x108].copy_from_slice(&0x707Fu16.to_be_bytes());
    // move.b d0, (a0)+
    rom[0x108..0x10A].copy_from_slice(&0x10C0u16.to_be_bytes());
    // move.b -(a0), d1
    rom[0x10A..0x10C].copy_from_slice(&0x1220u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.read_u8(0x00FF_0010), 0x7F);
    assert_eq!(cpu.d_regs[1] & 0xFF, 0x7F);
    assert_eq!(cpu.a_regs[0], 0x00FF_0010);
}

#[test]
fn move_byte_supports_displacement_absolute_and_immediate_to_register() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0030, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0030u32.to_be_bytes());
    // move.b #$80, d0
    rom[0x106..0x108].copy_from_slice(&0x103Cu16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x0080u16.to_be_bytes());
    // move.b d0, (2,a0)
    rom[0x10A..0x10C].copy_from_slice(&0x1140u16.to_be_bytes());
    rom[0x10C..0x10E].copy_from_slice(&0x0002u16.to_be_bytes());
    // move.b (2,a0), d1
    rom[0x10E..0x110].copy_from_slice(&0x1228u16.to_be_bytes());
    rom[0x110..0x112].copy_from_slice(&0x0002u16.to_be_bytes());
    // move.b d1, $00FF0034
    rom[0x112..0x114].copy_from_slice(&0x13C1u16.to_be_bytes());
    rom[0x114..0x118].copy_from_slice(&0x00FF_0034u32.to_be_bytes());
    // move.b $00FF0034, d2
    rom[0x118..0x11A].copy_from_slice(&0x1439u16.to_be_bytes());
    rom[0x11A..0x11E].copy_from_slice(&0x00FF_0034u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..7 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.read_u8(0x00FF_0032), 0x80);
    assert_eq!(memory.read_u8(0x00FF_0034), 0x80);
    assert_eq!(cpu.d_regs[1] & 0xFF, 0x80);
    assert_eq!(cpu.d_regs[2] & 0xFF, 0x80);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
}

#[test]
fn move_byte_handles_a7_byte_step_as_two() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0100, a7
    rom[0x100..0x102].copy_from_slice(&0x2E7Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0100u32.to_be_bytes());
    // moveq #$55, d0
    rom[0x106..0x108].copy_from_slice(&0x7055u16.to_be_bytes());
    // move.b d0, -(a7)
    rom[0x108..0x10A].copy_from_slice(&0x1F00u16.to_be_bytes());
    // move.b (a7)+, d1
    rom[0x10A..0x10C].copy_from_slice(&0x121Fu16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.read_u8(0x00FF_00FE), 0x55);
    assert_eq!(cpu.d_regs[1] & 0xFF, 0x55);
    assert_eq!(cpu.a_regs[7], 0x00FF_0100);
}

#[test]
fn executes_ori_and_andi_for_data_register_and_memory() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #0, d0
    rom[0x100..0x102].copy_from_slice(&0x7000u16.to_be_bytes());
    // ori.b #$80, d0
    rom[0x102..0x104].copy_from_slice(&0x0000u16.to_be_bytes());
    rom[0x104..0x106].copy_from_slice(&0x0080u16.to_be_bytes());
    // andi.b #$0F, d0
    rom[0x106..0x108].copy_from_slice(&0x0200u16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x000Fu16.to_be_bytes());
    // move.l #$00F0000F, $00FF0020
    rom[0x10A..0x10C].copy_from_slice(&0x23FCu16.to_be_bytes());
    rom[0x10C..0x110].copy_from_slice(&0x00F0_000Fu32.to_be_bytes());
    rom[0x110..0x114].copy_from_slice(&0x00FF_0020u32.to_be_bytes());
    // ori.l #$0000F000, $00FF0020
    rom[0x114..0x116].copy_from_slice(&0x00B9u16.to_be_bytes());
    rom[0x116..0x11A].copy_from_slice(&0x0000_F000u32.to_be_bytes());
    rom[0x11A..0x11E].copy_from_slice(&0x00FF_0020u32.to_be_bytes());
    // andi.l #$0000FF00, $00FF0020
    rom[0x11E..0x120].copy_from_slice(&0x02B9u16.to_be_bytes());
    rom[0x120..0x124].copy_from_slice(&0x0000_FF00u32.to_be_bytes());
    rom[0x124..0x128].copy_from_slice(&0x00FF_0020u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..3 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0] & 0xFF, 0x00);
    assert_ne!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);

    for _ in 0..3 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(memory.read_u32(0x00FF_0020), 0x0000_F000);
}

#[test]
fn executes_eori_for_data_register_and_memory() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #$55, d0
    rom[0x100..0x102].copy_from_slice(&0x7055u16.to_be_bytes());
    // eori.b #$FF, d0
    rom[0x102..0x104].copy_from_slice(&0x0A00u16.to_be_bytes());
    rom[0x104..0x106].copy_from_slice(&0x00FFu16.to_be_bytes());
    // move.l #$00FF00FF, $00FF0020
    rom[0x106..0x108].copy_from_slice(&0x23FCu16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x00FF_00FFu32.to_be_bytes());
    rom[0x10C..0x110].copy_from_slice(&0x00FF_0020u32.to_be_bytes());
    // eori.l #$00FF0000, $00FF0020
    rom[0x110..0x112].copy_from_slice(&0x0AB9u16.to_be_bytes());
    rom[0x112..0x116].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
    rom[0x116..0x11A].copy_from_slice(&0x00FF_0020u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0] & 0xFF, 0xAA);
    assert_eq!(memory.read_u32(0x00FF_0020), 0x0000_00FF);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn executes_eor_dn_to_ea_for_register_and_memory_destinations() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #$0F, d0
    rom[0x100..0x102].copy_from_slice(&0x700Fu16.to_be_bytes());
    // moveq #$33, d1
    rom[0x102..0x104].copy_from_slice(&0x7233u16.to_be_bytes());
    // eor.b d1, d0
    rom[0x104..0x106].copy_from_slice(&0xB300u16.to_be_bytes());
    // movea.l #$00FF0040, a0
    rom[0x106..0x108].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // move.w #$00F0, $00FF0042
    rom[0x10C..0x10E].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x10E..0x110].copy_from_slice(&0x00F0u16.to_be_bytes());
    rom[0x110..0x114].copy_from_slice(&0x00FF_0042u32.to_be_bytes());
    // eor.w d0, (2,a0)
    rom[0x114..0x116].copy_from_slice(&0xB168u16.to_be_bytes());
    rom[0x116..0x118].copy_from_slice(&0x0002u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..6 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0] & 0xFF, 0x3C);
    assert_eq!(memory.read_u16(0x00FF_0042), 0x00CC);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn executes_movea_adda_and_an_addressing_modes() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0010, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0010u32.to_be_bytes());
    // moveq #2, d0 (keep word accesses aligned)
    rom[0x106..0x108].copy_from_slice(&0x7002u16.to_be_bytes());
    // adda.l d0, a0
    rom[0x108..0x10A].copy_from_slice(&0xD1C0u16.to_be_bytes());
    // move.w d0, (a0)+
    rom[0x10A..0x10C].copy_from_slice(&0x30C0u16.to_be_bytes());
    // move.w d0, -(a0)
    rom[0x10C..0x10E].copy_from_slice(&0x3100u16.to_be_bytes());
    // move.w (a0)+, d1
    rom[0x10E..0x110].copy_from_slice(&0x3218u16.to_be_bytes());
    // move.w -(a0), d2
    rom[0x110..0x112].copy_from_slice(&0x3420u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..7 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.read_u16(0x00FF_0012), 0x0002);
    assert_eq!(cpu.d_regs[1] & 0xFFFF, 0x0002);
    assert_eq!(cpu.d_regs[2] & 0xFFFF, 0x0002);
}

#[test]
fn executes_jsr_and_rts() {
    let mut rom = vec![0u8; 0x600];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // jsr $00000120
    rom[0x100..0x102].copy_from_slice(&0x4EB9u16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x0000_0120u32.to_be_bytes());
    // nop
    rom[0x106..0x108].copy_from_slice(&0x4E71u16.to_be_bytes());

    // subroutine: move.w #$BEEF, $00FF0008 ; rts
    rom[0x120..0x122].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x122..0x124].copy_from_slice(&0xBEEFu16.to_be_bytes());
    rom[0x124..0x128].copy_from_slice(&0x00FF_0008u32.to_be_bytes());
    rom[0x128..0x12A].copy_from_slice(&0x4E75u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory); // jsr
    assert_eq!(cpu.pc(), 0x0000_0120);

    cpu.step(&mut memory); // move.w
    assert_eq!(memory.read_u16(0xFF0008), 0xBEEF);

    cpu.step(&mut memory); // rts
    assert_eq!(cpu.pc(), 0x0000_0106);
}

#[test]
fn executes_jsr_pc_relative_and_rts() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // jsr (18,pc) -> 0x00000114
    rom[0x100..0x102].copy_from_slice(&0x4EBAu16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0x0012u16.to_be_bytes());
    // move.w #$1111, $00FF0000
    rom[0x104..0x106].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x106..0x108].copy_from_slice(&0x1111u16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x00FF_0000u32.to_be_bytes());

    // subroutine: move.w #$2222, $00FF0002 ; rts
    rom[0x114..0x116].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x116..0x118].copy_from_slice(&0x2222u16.to_be_bytes());
    rom[0x118..0x11C].copy_from_slice(&0x00FF_0002u32.to_be_bytes());
    rom[0x11C..0x11E].copy_from_slice(&0x4E75u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory); // jsr (d16,pc)
    assert_eq!(cpu.pc(), 0x0000_0114);

    cpu.step(&mut memory); // subroutine move.w
    assert_eq!(memory.read_u16(0x00FF_0002), 0x2222);

    cpu.step(&mut memory); // rts
    assert_eq!(cpu.pc(), 0x0000_0104);

    cpu.step(&mut memory); // mainline move.w
    assert_eq!(memory.read_u16(0x00FF_0000), 0x1111);
}

#[test]
fn executes_jmp_an_and_pc_relative_modes() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00000120, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x0000_0120u32.to_be_bytes());
    // jmp (a0)
    rom[0x106..0x108].copy_from_slice(&0x4ED0u16.to_be_bytes());
    // move.w #$1111, $00FF0000 (skipped)
    rom[0x108..0x10A].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x10A..0x10C].copy_from_slice(&0x1111u16.to_be_bytes());
    rom[0x10C..0x110].copy_from_slice(&0x00FF_0000u32.to_be_bytes());

    // move.w #$2222, $00FF0002
    rom[0x120..0x122].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x122..0x124].copy_from_slice(&0x2222u16.to_be_bytes());
    rom[0x124..0x128].copy_from_slice(&0x00FF_0002u32.to_be_bytes());
    // jmp (10,pc) -> 0x00000134
    rom[0x128..0x12A].copy_from_slice(&0x4EFAu16.to_be_bytes());
    rom[0x12A..0x12C].copy_from_slice(&0x000Au16.to_be_bytes());
    // move.w #$3333, $00FF0004 (skipped)
    rom[0x12C..0x12E].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x12E..0x130].copy_from_slice(&0x3333u16.to_be_bytes());
    rom[0x130..0x134].copy_from_slice(&0x00FF_0004u32.to_be_bytes());
    // move.w #$4444, $00FF0006
    rom[0x134..0x136].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x136..0x138].copy_from_slice(&0x4444u16.to_be_bytes());
    rom[0x138..0x13C].copy_from_slice(&0x00FF_0006u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..5 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.read_u16(0x00FF_0000), 0x0000);
    assert_eq!(memory.read_u16(0x00FF_0002), 0x2222);
    assert_eq!(memory.read_u16(0x00FF_0004), 0x0000);
    assert_eq!(memory.read_u16(0x00FF_0006), 0x4444);
}

#[test]
fn updates_flags_for_cmpi_tst_and_branches_with_bne_beq() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #1, d0
    rom[0x100..0x102].copy_from_slice(&0x7001u16.to_be_bytes());
    // cmpi.w #1, d0   (Z=1)
    rom[0x102..0x104].copy_from_slice(&0x0C40u16.to_be_bytes());
    rom[0x104..0x106].copy_from_slice(&0x0001u16.to_be_bytes());
    // bne.s +8 (not taken)
    rom[0x106..0x108].copy_from_slice(&0x6608u16.to_be_bytes());
    // tst.w d0 (Z=0)
    rom[0x108..0x10A].copy_from_slice(&0x4A40u16.to_be_bytes());
    // beq.s +8 (not taken)
    rom[0x10A..0x10C].copy_from_slice(&0x6708u16.to_be_bytes());
    // move.w #$1111, $00FF0000
    rom[0x10C..0x10E].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x10E..0x110].copy_from_slice(&0x1111u16.to_be_bytes());
    rom[0x110..0x114].copy_from_slice(&0x00FF_0000u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..7 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.read_u16(0xFF0000), 0x1111);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn executes_bra_short() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // bra.s -2 (branch to self)
    rom[0x100..0x102].copy_from_slice(&0x60FEu16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    let cycles = cpu.step(&mut memory);
    assert_eq!(cycles, 10);
    assert_eq!(cpu.pc(), 0x0000_0100);
}

#[test]
fn executes_bra_word_using_extension_word_base_pc() {
    let mut rom = vec![0u8; 0x500];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // bra.w +0x0A -> 0x0000010C (base PC = 0x00000102)
    rom[0x100..0x102].copy_from_slice(&0x6000u16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0x000Au16.to_be_bytes());
    // move.w #$1111, $00FF0000 (skipped)
    rom[0x104..0x106].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x106..0x108].copy_from_slice(&0x1111u16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
    // move.w #$2222, $00FF0002
    rom[0x10C..0x10E].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x10E..0x110].copy_from_slice(&0x2222u16.to_be_bytes());
    rom[0x110..0x114].copy_from_slice(&0x00FF_0002u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory); // bra.w
    assert_eq!(cpu.pc(), 0x0000_010C);

    cpu.step(&mut memory); // move.w #$2222
    assert_eq!(memory.read_u16(0x00FF_0000), 0x0000);
    assert_eq!(memory.read_u16(0x00FF_0002), 0x2222);
}

#[test]
fn executes_bsr_short_and_returns_with_rts() {
    let mut rom = vec![0u8; 0x600];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // bsr.s +0x10 -> 0x00000112
    rom[0x100..0x102].copy_from_slice(&0x6110u16.to_be_bytes());
    // move.w #$1111, $00FF0000
    rom[0x102..0x104].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x104..0x106].copy_from_slice(&0x1111u16.to_be_bytes());
    rom[0x106..0x10A].copy_from_slice(&0x00FF_0000u32.to_be_bytes());

    // subroutine: move.w #$2222, $00FF0002 ; rts
    rom[0x112..0x114].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x114..0x116].copy_from_slice(&0x2222u16.to_be_bytes());
    rom[0x116..0x11A].copy_from_slice(&0x00FF_0002u32.to_be_bytes());
    rom[0x11A..0x11C].copy_from_slice(&0x4E75u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    let bsr_cycles = cpu.step(&mut memory);
    assert_eq!(bsr_cycles, 18);
    assert_eq!(cpu.pc(), 0x0000_0112);

    cpu.step(&mut memory); // subroutine move.w
    assert_eq!(memory.read_u16(0x00FF_0002), 0x2222);

    cpu.step(&mut memory); // rts
    assert_eq!(cpu.pc(), 0x0000_0102);

    cpu.step(&mut memory); // mainline move.w
    assert_eq!(memory.read_u16(0x00FF_0000), 0x1111);
}

#[test]
fn executes_bsr_word_and_returns_to_post_extension_address() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // bsr.w +0x10 -> 0x00000112 (base PC = 0x00000102)
    rom[0x100..0x102].copy_from_slice(&0x6100u16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0x0010u16.to_be_bytes());
    // move.w #$1111, $00FF0000
    rom[0x104..0x106].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x106..0x108].copy_from_slice(&0x1111u16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x00FF_0000u32.to_be_bytes());

    // subroutine: move.w #$2222, $00FF0002 ; rts
    rom[0x112..0x114].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x114..0x116].copy_from_slice(&0x2222u16.to_be_bytes());
    rom[0x116..0x11A].copy_from_slice(&0x00FF_0002u32.to_be_bytes());
    rom[0x11A..0x11C].copy_from_slice(&0x4E75u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory); // bsr.w
    assert_eq!(cpu.pc(), 0x0000_0112);

    cpu.step(&mut memory); // subroutine move.w
    assert_eq!(memory.read_u16(0x00FF_0002), 0x2222);

    cpu.step(&mut memory); // rts
    assert_eq!(cpu.pc(), 0x0000_0104);

    cpu.step(&mut memory); // mainline move.w
    assert_eq!(memory.read_u16(0x00FF_0000), 0x1111);
}

#[test]
fn executes_bcc_and_bcs_for_taken_and_not_taken_paths() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #0, d0
    rom[0x100..0x102].copy_from_slice(&0x7000u16.to_be_bytes());
    // cmpi.w #1, d0 (C=1)
    rom[0x102..0x104].copy_from_slice(&0x0C40u16.to_be_bytes());
    rom[0x104..0x106].copy_from_slice(&0x0001u16.to_be_bytes());
    // bcs.s +8 (taken)
    rom[0x106..0x108].copy_from_slice(&0x6508u16.to_be_bytes());
    // move.w #$1111, $00FF0000 (skipped)
    rom[0x108..0x10A].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x10A..0x10C].copy_from_slice(&0x1111u16.to_be_bytes());
    rom[0x10C..0x110].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
    // bcc.s +8 (not taken)
    rom[0x110..0x112].copy_from_slice(&0x6408u16.to_be_bytes());
    // move.w #$2222, $00FF0002
    rom[0x112..0x114].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x114..0x116].copy_from_slice(&0x2222u16.to_be_bytes());
    rom[0x116..0x11A].copy_from_slice(&0x00FF_0002u32.to_be_bytes());
    // moveq #1, d1
    rom[0x11A..0x11C].copy_from_slice(&0x7201u16.to_be_bytes());
    // cmpi.w #1, d1 (C=0)
    rom[0x11C..0x11E].copy_from_slice(&0x0C41u16.to_be_bytes());
    rom[0x11E..0x120].copy_from_slice(&0x0001u16.to_be_bytes());
    // bcc.s +8 (taken)
    rom[0x120..0x122].copy_from_slice(&0x6408u16.to_be_bytes());
    // move.w #$3333, $00FF0004 (skipped)
    rom[0x122..0x124].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x124..0x126].copy_from_slice(&0x3333u16.to_be_bytes());
    rom[0x126..0x12A].copy_from_slice(&0x00FF_0004u32.to_be_bytes());
    // bcs.s +8 (not taken)
    rom[0x12A..0x12C].copy_from_slice(&0x6508u16.to_be_bytes());
    // move.w #$4444, $00FF0006
    rom[0x12C..0x12E].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x12E..0x130].copy_from_slice(&0x4444u16.to_be_bytes());
    rom[0x130..0x134].copy_from_slice(&0x00FF_0006u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..10 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.read_u16(0x00FF_0000), 0x0000);
    assert_eq!(memory.read_u16(0x00FF_0002), 0x2222);
    assert_eq!(memory.read_u16(0x00FF_0004), 0x0000);
    assert_eq!(memory.read_u16(0x00FF_0006), 0x4444);
}

#[test]
fn bcc_cycle_counts_differ_for_short_and_word_not_taken() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #0, d0
    rom[0x100..0x102].copy_from_slice(&0x7000u16.to_be_bytes());
    // cmpi.w #1, d0 (C=1)
    rom[0x102..0x104].copy_from_slice(&0x0C40u16.to_be_bytes());
    rom[0x104..0x106].copy_from_slice(&0x0001u16.to_be_bytes());
    // bcc.s +2 (not taken)
    rom[0x106..0x108].copy_from_slice(&0x6402u16.to_be_bytes());
    // bcc.w +2 (not taken)
    rom[0x108..0x10A].copy_from_slice(&0x6400u16.to_be_bytes());
    rom[0x10A..0x10C].copy_from_slice(&0x0002u16.to_be_bytes());
    // bcs.s +2 (taken)
    rom[0x10C..0x10E].copy_from_slice(&0x6502u16.to_be_bytes());
    // nop (skipped by bcs)
    rom[0x10E..0x110].copy_from_slice(&0x4E71u16.to_be_bytes());
    // nop
    rom[0x110..0x112].copy_from_slice(&0x4E71u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory); // moveq
    cpu.step(&mut memory); // cmpi
    let c1 = cpu.step(&mut memory); // bcc.s not taken
    let c2 = cpu.step(&mut memory); // bcc.w not taken
    let c3 = cpu.step(&mut memory); // bcs.s taken

    assert_eq!(c1, 8);
    assert_eq!(c2, 12);
    assert_eq!(c3, 10);
    assert_eq!(cpu.pc(), 0x0000_0110);
}

#[test]
fn executes_bmi_and_bpl_for_taken_and_not_taken_paths() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #0, d0
    rom[0x100..0x102].copy_from_slice(&0x7000u16.to_be_bytes());
    // cmpi.w #1, d0 (N=1)
    rom[0x102..0x104].copy_from_slice(&0x0C40u16.to_be_bytes());
    rom[0x104..0x106].copy_from_slice(&0x0001u16.to_be_bytes());
    // bmi.s +8 (taken)
    rom[0x106..0x108].copy_from_slice(&0x6B08u16.to_be_bytes());
    // move.w #$1111, $00FF0010 (skipped)
    rom[0x108..0x10A].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x10A..0x10C].copy_from_slice(&0x1111u16.to_be_bytes());
    rom[0x10C..0x110].copy_from_slice(&0x00FF_0010u32.to_be_bytes());
    // bpl.s +8 (not taken)
    rom[0x110..0x112].copy_from_slice(&0x6A08u16.to_be_bytes());
    // move.w #$2222, $00FF0012
    rom[0x112..0x114].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x114..0x116].copy_from_slice(&0x2222u16.to_be_bytes());
    rom[0x116..0x11A].copy_from_slice(&0x00FF_0012u32.to_be_bytes());
    // moveq #1, d0
    rom[0x11A..0x11C].copy_from_slice(&0x7001u16.to_be_bytes());
    // tst.w d0 (N=0)
    rom[0x11C..0x11E].copy_from_slice(&0x4A40u16.to_be_bytes());
    // bpl.s +8 (taken)
    rom[0x11E..0x120].copy_from_slice(&0x6A08u16.to_be_bytes());
    // move.w #$3333, $00FF0014 (skipped)
    rom[0x120..0x122].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x122..0x124].copy_from_slice(&0x3333u16.to_be_bytes());
    rom[0x124..0x128].copy_from_slice(&0x00FF_0014u32.to_be_bytes());
    // bmi.s +8 (not taken)
    rom[0x128..0x12A].copy_from_slice(&0x6B08u16.to_be_bytes());
    // move.w #$4444, $00FF0016
    rom[0x12A..0x12C].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x12C..0x12E].copy_from_slice(&0x4444u16.to_be_bytes());
    rom[0x12E..0x132].copy_from_slice(&0x00FF_0016u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..10 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.read_u16(0x00FF_0010), 0x0000);
    assert_eq!(memory.read_u16(0x00FF_0012), 0x2222);
    assert_eq!(memory.read_u16(0x00FF_0014), 0x0000);
    assert_eq!(memory.read_u16(0x00FF_0016), 0x4444);
}

#[test]
fn scc_writes_condition_result_without_modifying_ccr() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #0, d0 (Z=1)
    rom[0x100..0x102].copy_from_slice(&0x7000u16.to_be_bytes());
    // seq d1
    rom[0x102..0x104].copy_from_slice(&0x57C1u16.to_be_bytes());
    // movea.l #$00FF0040, a0
    rom[0x104..0x106].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x106..0x10A].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // sne (a0) ; Z=1 so writes 0
    rom[0x10A..0x10C].copy_from_slice(&0x56D0u16.to_be_bytes());
    // moveq #1, d2 (Z=0)
    rom[0x10C..0x10E].copy_from_slice(&0x7401u16.to_be_bytes());
    // sne (1,a0) ; Z=0 so writes 0xFF
    rom[0x10E..0x110].copy_from_slice(&0x56E8u16.to_be_bytes());
    rom[0x110..0x112].copy_from_slice(&0x0001u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory); // moveq #0, d0
    assert_ne!(cpu.sr() & CCR_Z, 0);

    cpu.step(&mut memory); // seq d1
    assert_eq!(cpu.d_regs[1] & 0xFF, 0xFF);
    assert_ne!(cpu.sr() & CCR_Z, 0, "Scc must not change CCR");

    cpu.step(&mut memory); // movea.l
    cpu.step(&mut memory); // sne (a0)
    assert_eq!(memory.read_u8(0x00FF_0040), 0x00);
    assert_ne!(cpu.sr() & CCR_Z, 0, "Scc must not change CCR");

    cpu.step(&mut memory); // moveq #1, d2
    assert_eq!(cpu.sr() & CCR_Z, 0);

    cpu.step(&mut memory); // sne (1,a0)
    assert_eq!(memory.read_u8(0x00FF_0041), 0xFF);
    assert_eq!(cpu.sr() & CCR_Z, 0, "Scc must not change CCR");
}

#[test]
fn dbcc_loops_until_counter_expires_and_skips_decrement_when_condition_true() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #2, d0
    rom[0x100..0x102].copy_from_slice(&0x7002u16.to_be_bytes());
    // moveq #0, d1
    rom[0x102..0x104].copy_from_slice(&0x7200u16.to_be_bytes());
    // addq.b #1, d1
    rom[0x104..0x106].copy_from_slice(&0x5201u16.to_be_bytes());
    // dbf d0, -4 (to addq.b)
    rom[0x106..0x108].copy_from_slice(&0x51C8u16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0xFFFCu16.to_be_bytes());
    // moveq #1, d2 (Z=0)
    rom[0x10A..0x10C].copy_from_slice(&0x7401u16.to_be_bytes());
    // dbne d2, +0 (condition true, must not decrement d2)
    rom[0x10C..0x10E].copy_from_slice(&0x56CAu16.to_be_bytes());
    rom[0x10E..0x110].copy_from_slice(&0x0000u16.to_be_bytes());
    // move.w d1, $00FF0000
    rom[0x110..0x112].copy_from_slice(&0x33C1u16.to_be_bytes());
    rom[0x112..0x116].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
    // move.w d2, $00FF0002
    rom[0x116..0x118].copy_from_slice(&0x33C2u16.to_be_bytes());
    rom[0x118..0x11C].copy_from_slice(&0x00FF_0002u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..12 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[1] & 0xFF, 0x03);
    assert_eq!(cpu.d_regs[0] & 0xFFFF, 0xFFFF);
    assert_eq!(cpu.d_regs[2] & 0xFFFF, 0x0001);
    assert_eq!(memory.read_u16(0x00FF_0000), 0x0003);
    assert_eq!(memory.read_u16(0x00FF_0002), 0x0001);
}

#[test]
fn abcd_memory_mode_updates_xc_and_preserves_zero_until_nonzero_result() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // abcd -(a1),-(a0)
    rom[0x100..0x102].copy_from_slice(&0xC109u16.to_be_bytes());
    // abcd -(a1),-(a0)
    rom[0x102..0x104].copy_from_slice(&0xC109u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.a_regs[0] = 0x00FF_0007;
    cpu.a_regs[1] = 0x00FF_0005;
    memory.write_u8(0x00FF_0006, 0x45);
    memory.write_u8(0x00FF_0004, 0x55);
    memory.write_u8(0x00FF_0005, 0x00);
    memory.write_u8(0x00FF_0003, 0x55);
    cpu.sr |= CCR_Z;
    cpu.sr &= !CCR_X;

    let cycles1 = cpu.step(&mut memory);
    assert_eq!(cycles1, 18);
    assert_eq!(memory.read_u8(0x00FF_0006), 0x00);
    assert_ne!(cpu.sr() & CCR_C, 0);
    assert_ne!(cpu.sr() & CCR_X, 0);
    assert_ne!(cpu.sr() & CCR_Z, 0);

    let cycles2 = cpu.step(&mut memory);
    assert_eq!(cycles2, 18);
    assert_eq!(memory.read_u8(0x00FF_0005), 0x56);
    assert_eq!(cpu.sr() & CCR_C, 0);
    assert_eq!(cpu.sr() & CCR_X, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
}

#[test]
fn sbcd_memory_mode_predecrements_address_registers_and_writes_result() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // sbcd -(a1),-(a0)
    rom[0x100..0x102].copy_from_slice(&0x8109u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.a_regs[0] = 0x00FF_0005;
    cpu.a_regs[1] = 0x00FF_0003;
    memory.write_u8(0x00FF_0004, 0x00);
    memory.write_u8(0x00FF_0002, 0x01);
    cpu.sr |= CCR_X | CCR_Z;

    let cycles = cpu.step(&mut memory);
    assert_eq!(cycles, 18);
    assert_eq!(cpu.a_regs[0], 0x00FF_0004);
    assert_eq!(cpu.a_regs[1], 0x00FF_0002);
    assert_eq!(memory.read_u8(0x00FF_0004), 0x98);
    assert_ne!(cpu.sr() & CCR_C, 0);
    assert_ne!(cpu.sr() & CCR_X, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
}

#[test]
fn exg_swaps_data_and_address_register_variants() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #1, d0
    rom[0x100..0x102].copy_from_slice(&0x7001u16.to_be_bytes());
    // moveq #2, d1
    rom[0x102..0x104].copy_from_slice(&0x7202u16.to_be_bytes());
    // exg d0,d1
    rom[0x104..0x106].copy_from_slice(&0xC141u16.to_be_bytes());
    // movea.l #$11223344, a0
    rom[0x106..0x108].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x1122_3344u32.to_be_bytes());
    // movea.l #$55667788, a1
    rom[0x10C..0x10E].copy_from_slice(&0x227Cu16.to_be_bytes());
    rom[0x10E..0x112].copy_from_slice(&0x5566_7788u32.to_be_bytes());
    // exg a0,a1
    rom[0x112..0x114].copy_from_slice(&0xC149u16.to_be_bytes());
    // exg d0,a0
    rom[0x114..0x116].copy_from_slice(&0xC188u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..7 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0], 0x5566_7788);
    assert_eq!(cpu.d_regs[1], 0x0000_0001);
    assert_eq!(cpu.a_regs[0], 0x0000_0002);
    assert_eq!(cpu.a_regs[1], 0x1122_3344);
}

#[test]
fn addi_and_subi_byte_update_flags() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #1, d0
    rom[0x100..0x102].copy_from_slice(&0x7001u16.to_be_bytes());
    // addi.b #$7F, d0
    rom[0x102..0x104].copy_from_slice(&0x0600u16.to_be_bytes());
    rom[0x104..0x106].copy_from_slice(&0x007Fu16.to_be_bytes());
    // subi.b #$80, d0
    rom[0x106..0x108].copy_from_slice(&0x0400u16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x0080u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory); // moveq
    cpu.step(&mut memory); // addi.b
    assert_eq!(cpu.d_regs[0] & 0xFF, 0x80);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_ne!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);

    cpu.step(&mut memory); // subi.b
    assert_eq!(cpu.d_regs[0] & 0xFF, 0x00);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_ne!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn addi_and_subi_long_support_absolute_long_memory_destination() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.l #$00000010, $00FF0020
    rom[0x100..0x102].copy_from_slice(&0x23FCu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x0000_0010u32.to_be_bytes());
    rom[0x106..0x10A].copy_from_slice(&0x00FF_0020u32.to_be_bytes());
    // addi.l #$00000005, $00FF0020
    rom[0x10A..0x10C].copy_from_slice(&0x06B9u16.to_be_bytes());
    rom[0x10C..0x110].copy_from_slice(&0x0000_0005u32.to_be_bytes());
    rom[0x110..0x114].copy_from_slice(&0x00FF_0020u32.to_be_bytes());
    // subi.l #$00000015, $00FF0020
    rom[0x114..0x116].copy_from_slice(&0x04B9u16.to_be_bytes());
    rom[0x116..0x11A].copy_from_slice(&0x0000_0015u32.to_be_bytes());
    rom[0x11A..0x11E].copy_from_slice(&0x00FF_0020u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory); // move.l
    cpu.step(&mut memory); // addi.l
    assert_eq!(memory.read_u32(0x00FF_0020), 0x0000_0015);

    cpu.step(&mut memory); // subi.l
    assert_eq!(memory.read_u32(0x00FF_0020), 0x0000_0000);
    assert_ne!(cpu.sr() & CCR_Z, 0);
}

#[test]
fn add_and_sub_ea_to_dn_with_register_source() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #5, d0
    rom[0x100..0x102].copy_from_slice(&0x7005u16.to_be_bytes());
    // moveq #3, d1
    rom[0x102..0x104].copy_from_slice(&0x7203u16.to_be_bytes());
    // add.w d1, d0
    rom[0x104..0x106].copy_from_slice(&0xD041u16.to_be_bytes());
    // sub.b d1, d0
    rom[0x106..0x108].copy_from_slice(&0x9001u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0] & 0xFF, 0x05);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn add_and_sub_ea_to_dn_with_displacement_memory_source() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0040, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // move.w #$0010, $00FF0042
    rom[0x106..0x108].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x0010u16.to_be_bytes());
    rom[0x10A..0x10E].copy_from_slice(&0x00FF_0042u32.to_be_bytes());
    // move.l #$00000020, $00FF0044
    rom[0x10E..0x110].copy_from_slice(&0x23FCu16.to_be_bytes());
    rom[0x110..0x114].copy_from_slice(&0x0000_0020u32.to_be_bytes());
    rom[0x114..0x118].copy_from_slice(&0x00FF_0044u32.to_be_bytes());
    // moveq #1, d0
    rom[0x118..0x11A].copy_from_slice(&0x7001u16.to_be_bytes());
    // add.w (2,a0), d0
    rom[0x11A..0x11C].copy_from_slice(&0xD068u16.to_be_bytes());
    rom[0x11C..0x11E].copy_from_slice(&0x0002u16.to_be_bytes());
    // sub.l (4,a0), d0
    rom[0x11E..0x120].copy_from_slice(&0x90A8u16.to_be_bytes());
    rom[0x120..0x122].copy_from_slice(&0x0004u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..6 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0], 0xFFFF_FFF1);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_ne!(cpu.sr() & CCR_C, 0);
}

#[test]
fn and_and_or_ea_to_dn_with_register_source() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #$0F, d0
    rom[0x100..0x102].copy_from_slice(&0x700Fu16.to_be_bytes());
    // moveq #$33, d1
    rom[0x102..0x104].copy_from_slice(&0x7233u16.to_be_bytes());
    // or.b d1, d0
    rom[0x104..0x106].copy_from_slice(&0x8001u16.to_be_bytes());
    // and.b d1, d0
    rom[0x106..0x108].copy_from_slice(&0xC001u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0] & 0xFF, 0x33);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn and_and_or_ea_to_dn_with_displacement_memory_source() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0040, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // move.l #$0F0F00FF, $00FF0044
    rom[0x106..0x108].copy_from_slice(&0x23FCu16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x0F0F_00FFu32.to_be_bytes());
    rom[0x10C..0x110].copy_from_slice(&0x00FF_0044u32.to_be_bytes());
    // move.l #$F0F0FFFF, d0
    rom[0x110..0x112].copy_from_slice(&0x203Cu16.to_be_bytes());
    rom[0x112..0x116].copy_from_slice(&0xF0F0_FFFFu32.to_be_bytes());
    // and.l (4,a0), d0
    rom[0x116..0x118].copy_from_slice(&0xC0A8u16.to_be_bytes());
    rom[0x118..0x11A].copy_from_slice(&0x0004u16.to_be_bytes());
    // or.l (4,a0), d0
    rom[0x11A..0x11C].copy_from_slice(&0x80A8u16.to_be_bytes());
    rom[0x11C..0x11E].copy_from_slice(&0x0004u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..5 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0], 0x0F0F_00FF);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn and_and_or_dn_to_ea_with_register_and_memory_destinations() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #$0F, d0
    rom[0x100..0x102].copy_from_slice(&0x700Fu16.to_be_bytes());
    // moveq #$30, d1
    rom[0x102..0x104].copy_from_slice(&0x7230u16.to_be_bytes());
    // or.b d0, d1
    rom[0x104..0x106].copy_from_slice(&0x8101u16.to_be_bytes());
    // and.b d0, d1
    rom[0x106..0x108].copy_from_slice(&0xC101u16.to_be_bytes());
    // movea.l #$00FF0040, a0
    rom[0x108..0x10A].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x10A..0x10E].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // move.w #$00F0, $00FF0042
    rom[0x10E..0x110].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x110..0x112].copy_from_slice(&0x00F0u16.to_be_bytes());
    rom[0x112..0x116].copy_from_slice(&0x00FF_0042u32.to_be_bytes());
    // or.w d1, (2,a0)
    rom[0x116..0x118].copy_from_slice(&0x8368u16.to_be_bytes());
    rom[0x118..0x11A].copy_from_slice(&0x0002u16.to_be_bytes());
    // and.w d0, (2,a0)
    rom[0x11A..0x11C].copy_from_slice(&0xC168u16.to_be_bytes());
    rom[0x11C..0x11E].copy_from_slice(&0x0002u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..8 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[1] & 0xFF, 0x0F);
    assert_eq!(memory.read_u16(0x00FF_0042), 0x000F);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn mulu_word_with_register_source_updates_result_and_flags() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #6, d0
    rom[0x100..0x102].copy_from_slice(&0x7006u16.to_be_bytes());
    // moveq #7, d1
    rom[0x102..0x104].copy_from_slice(&0x7207u16.to_be_bytes());
    // mulu.w d1, d0
    rom[0x104..0x106].copy_from_slice(&0xC0C1u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);
    cpu.step(&mut memory);

    assert_eq!(cpu.d_regs[0], 42);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn mulu_word_with_displacement_memory_source_sets_zero() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0040, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // move.w #0, $00FF0042
    rom[0x106..0x108].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x0000u16.to_be_bytes());
    rom[0x10A..0x10E].copy_from_slice(&0x00FF_0042u32.to_be_bytes());
    // moveq #3, d0
    rom[0x10E..0x110].copy_from_slice(&0x7003u16.to_be_bytes());
    // mulu.w (2,a0), d0
    rom[0x110..0x112].copy_from_slice(&0xC0E8u16.to_be_bytes());
    rom[0x112..0x114].copy_from_slice(&0x0002u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..5 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0], 0);
    assert_ne!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn muls_word_with_register_source_handles_negative_result() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #-2, d0
    rom[0x100..0x102].copy_from_slice(&0x70FEu16.to_be_bytes());
    // moveq #3, d1
    rom[0x102..0x104].copy_from_slice(&0x7203u16.to_be_bytes());
    // muls.w d1, d0
    rom[0x104..0x106].copy_from_slice(&0xC1C1u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);
    cpu.step(&mut memory);

    assert_eq!(cpu.d_regs[0], 0xFFFF_FFFA);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn muls_word_with_memory_source_sets_zero() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0040, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // move.w #-5, $00FF0042
    rom[0x106..0x108].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0xFFFBu16.to_be_bytes());
    rom[0x10A..0x10E].copy_from_slice(&0x00FF_0042u32.to_be_bytes());
    // moveq #0, d0
    rom[0x10E..0x110].copy_from_slice(&0x7000u16.to_be_bytes());
    // muls.w (2,a0), d0
    rom[0x110..0x112].copy_from_slice(&0xC1E8u16.to_be_bytes());
    rom[0x112..0x114].copy_from_slice(&0x0002u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..5 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0], 0);
    assert_ne!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn divu_word_with_register_source_produces_quotient_and_remainder() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.l #$0001000A, d0
    rom[0x100..0x102].copy_from_slice(&0x203Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x0001_000Au32.to_be_bytes());
    // moveq #5, d1
    rom[0x106..0x108].copy_from_slice(&0x7205u16.to_be_bytes());
    // divu.w d1, d0
    rom[0x108..0x10A].copy_from_slice(&0x80C1u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);
    cpu.step(&mut memory);

    assert_eq!(cpu.d_regs[0], 0x0001_3335);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn divu_by_zero_vectors_to_exception_5() {
    let mut rom = vec![0u8; 0x600];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // Divide by zero vector #5
    rom[0x14..0x18].copy_from_slice(&0x0000_0200u32.to_be_bytes());
    rom[0x100..0x102].copy_from_slice(&0x7007u16.to_be_bytes()); // moveq #7, d0
    rom[0x102..0x104].copy_from_slice(&0x7200u16.to_be_bytes()); // moveq #0, d1
    rom[0x104..0x106].copy_from_slice(&0x80C1u16.to_be_bytes()); // divu.w d1, d0

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);
    let cycles = cpu.step(&mut memory);

    assert_eq!(cycles, 38);
    assert_eq!(cpu.pc(), 0x0000_0200);
    assert_eq!(cpu.a_regs[7], 0x00FF_0FFA);
    assert_eq!(cpu.d_regs[0], 7);
    assert_eq!(memory.read_u32(0x00FF_0FFC), 0x0000_0106);
}

#[test]
fn divs_word_with_register_source_handles_negative_result() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.l #$FFFFFFD8 (-40), d0
    rom[0x100..0x102].copy_from_slice(&0x203Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0xFFFF_FFD8u32.to_be_bytes());
    // moveq #6, d1
    rom[0x106..0x108].copy_from_slice(&0x7206u16.to_be_bytes());
    // divs.w d1, d0
    rom[0x108..0x10A].copy_from_slice(&0x81C1u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);
    cpu.step(&mut memory);

    assert_eq!(cpu.d_regs[0], 0xFFFC_FFFA);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn divs_word_overflow_sets_v_and_keeps_destination() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.l #$00010000, d0
    rom[0x100..0x102].copy_from_slice(&0x203Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    // moveq #1, d1
    rom[0x106..0x108].copy_from_slice(&0x7201u16.to_be_bytes());
    // divs.w d1, d0 (overflow: quotient 65536)
    rom[0x108..0x10A].copy_from_slice(&0x81C1u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);
    cpu.step(&mut memory);

    assert_eq!(cpu.d_regs[0], 0x0001_0000);
    assert_ne!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn cmp_ea_to_dn_supports_register_and_displacement_memory_sources() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #$10, d0
    rom[0x100..0x102].copy_from_slice(&0x7010u16.to_be_bytes());
    // moveq #$10, d1
    rom[0x102..0x104].copy_from_slice(&0x7210u16.to_be_bytes());
    // cmp.w d1, d0
    rom[0x104..0x106].copy_from_slice(&0xB041u16.to_be_bytes());
    // movea.l #$00FF0040, a0
    rom[0x106..0x108].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // move.w #$0011, $00FF0042
    rom[0x10C..0x10E].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x10E..0x110].copy_from_slice(&0x0011u16.to_be_bytes());
    rom[0x110..0x114].copy_from_slice(&0x00FF_0042u32.to_be_bytes());
    // cmp.w (2,a0), d0
    rom[0x114..0x116].copy_from_slice(&0xB068u16.to_be_bytes());
    rom[0x116..0x118].copy_from_slice(&0x0002u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);
    cpu.step(&mut memory); // cmp.w d1, d0
    assert_ne!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);

    cpu.step(&mut memory);
    cpu.step(&mut memory);
    cpu.step(&mut memory); // cmp.w (2,a0), d0
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_ne!(cpu.sr() & CCR_C, 0);
}

#[test]
fn addq_and_subq_support_register_and_displacement_memory_destinations() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #1, d0
    rom[0x100..0x102].copy_from_slice(&0x7001u16.to_be_bytes());
    // addq.b #8, d0
    rom[0x102..0x104].copy_from_slice(&0x5000u16.to_be_bytes());
    // subq.b #1, d0
    rom[0x104..0x106].copy_from_slice(&0x5300u16.to_be_bytes());
    // movea.l #$00FF0040, a0
    rom[0x106..0x108].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // move.w #$0001, $00FF0042
    rom[0x10C..0x10E].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x10E..0x110].copy_from_slice(&0x0001u16.to_be_bytes());
    rom[0x110..0x114].copy_from_slice(&0x00FF_0042u32.to_be_bytes());
    // addq.w #7, (2,a0)
    rom[0x114..0x116].copy_from_slice(&0x5E68u16.to_be_bytes());
    rom[0x116..0x118].copy_from_slice(&0x0002u16.to_be_bytes());
    // subq.w #2, (2,a0)
    rom[0x118..0x11A].copy_from_slice(&0x5568u16.to_be_bytes());
    rom[0x11A..0x11C].copy_from_slice(&0x0002u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..3 {
        cpu.step(&mut memory);
    }
    assert_eq!(cpu.d_regs[0] & 0xFF, 0x08);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }
    assert_eq!(memory.read_u16(0x00FF_0042), 0x0006);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn clr_word_on_data_register_clears_low_word_and_sets_zero_flag() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.l #$12345678, d0
    rom[0x100..0x102].copy_from_slice(&0x203Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x1234_5678u32.to_be_bytes());
    // clr.w d0
    rom[0x106..0x108].copy_from_slice(&0x4240u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);

    assert_eq!(cpu.d_regs[0], 0x1234_0000);
    assert_ne!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn clr_word_supports_postincrement_destination() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0020, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0020u32.to_be_bytes());
    // move.w #$BEEF, $00FF0020
    rom[0x106..0x108].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0xBEEFu16.to_be_bytes());
    rom[0x10A..0x10E].copy_from_slice(&0x00FF_0020u32.to_be_bytes());
    // clr.w (a0)+
    rom[0x10E..0x110].copy_from_slice(&0x4258u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);
    cpu.step(&mut memory);

    assert_eq!(memory.read_u16(0x00FF_0020), 0x0000);
    assert_eq!(cpu.a_regs[0], 0x00FF_0022);
}

#[test]
fn can_write_to_vdp_ports_via_move_sequence() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.w #$4000, $00C00004  ; VDP command high word (VRAM write @0)
    rom[0x100..0x102].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0x4000u16.to_be_bytes());
    rom[0x104..0x108].copy_from_slice(&0x00C0_0004u32.to_be_bytes());
    // moveq #0, d0
    rom[0x108..0x10A].copy_from_slice(&0x7000u16.to_be_bytes());
    // move.w d0, $00C00004      ; VDP command low word
    rom[0x10A..0x10C].copy_from_slice(&0x33C0u16.to_be_bytes());
    rom[0x10C..0x110].copy_from_slice(&0x00C0_0004u32.to_be_bytes());
    // move.l #$0000ABCD, d0
    rom[0x110..0x112].copy_from_slice(&0x203Cu16.to_be_bytes());
    rom[0x112..0x116].copy_from_slice(&0x0000_ABCDu32.to_be_bytes());
    // move.w d0, $00C00000
    rom[0x116..0x118].copy_from_slice(&0x33C0u16.to_be_bytes());
    rom[0x118..0x11C].copy_from_slice(&0x00C0_0000u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..5 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.vdp().read_vram_u8(0), 0xAB);
    assert_eq!(memory.vdp().read_vram_u8(1), 0xCD);
}

#[test]
fn cmppi_and_tst_support_memory_effective_addresses() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.w #$1234, $00FF0010
    rom[0x100..0x102].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0x1234u16.to_be_bytes());
    rom[0x104..0x108].copy_from_slice(&0x00FF_0010u32.to_be_bytes());
    // movea.l #$00FF0010, a0
    rom[0x108..0x10A].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x10A..0x10E].copy_from_slice(&0x00FF_0010u32.to_be_bytes());
    // cmpi.w #$1234, (a0)
    rom[0x10E..0x110].copy_from_slice(&0x0C50u16.to_be_bytes());
    rom[0x110..0x112].copy_from_slice(&0x1234u16.to_be_bytes());
    // tst.w (a0)+
    rom[0x112..0x114].copy_from_slice(&0x4A58u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);
    cpu.step(&mut memory);
    assert_ne!(cpu.sr() & CCR_Z, 0, "CMPI equal should set Z");

    cpu.step(&mut memory);
    assert_eq!(cpu.sr() & CCR_Z, 0, "TST non-zero should clear Z");
    assert_eq!(cpu.a_regs[0], 0x00FF_0012);
}

#[test]
fn movea_and_adda_support_absolute_and_postincrement_sources() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.l #$00000010, $00FF0020
    rom[0x100..0x102].copy_from_slice(&0x23FCu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x0000_0010u32.to_be_bytes());
    rom[0x106..0x10A].copy_from_slice(&0x00FF_0020u32.to_be_bytes());
    // movea.l $00FF0020, a1
    rom[0x10A..0x10C].copy_from_slice(&0x2279u16.to_be_bytes());
    rom[0x10C..0x110].copy_from_slice(&0x00FF_0020u32.to_be_bytes());
    // movea.l #$00FF0030, a0
    rom[0x110..0x112].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x112..0x116].copy_from_slice(&0x00FF_0030u32.to_be_bytes());
    // move.w #$0003, $00FF0030
    rom[0x116..0x118].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x118..0x11A].copy_from_slice(&0x0003u16.to_be_bytes());
    rom[0x11A..0x11E].copy_from_slice(&0x00FF_0030u32.to_be_bytes());
    // move.w #$0004, $00FF0032
    rom[0x11E..0x120].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x120..0x122].copy_from_slice(&0x0004u16.to_be_bytes());
    rom[0x122..0x126].copy_from_slice(&0x00FF_0032u32.to_be_bytes());
    // adda.w (a0)+, a1
    rom[0x126..0x128].copy_from_slice(&0xD2D8u16.to_be_bytes());
    // adda.w (a0)+, a1
    rom[0x128..0x12A].copy_from_slice(&0xD2D8u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..8 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.a_regs[1], 0x0000_0017);
    assert_eq!(cpu.a_regs[0], 0x00FF_0034);
}

#[test]
fn suba_word_and_long_immediate_are_decoded() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00000100, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // suba.w #$0004, a0
    rom[0x106..0x108].copy_from_slice(&0x90FCu16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x0004u16.to_be_bytes());
    // suba.l #$00000010, a0
    rom[0x10A..0x10C].copy_from_slice(&0x91FCu16.to_be_bytes());
    rom[0x10C..0x110].copy_from_slice(&0x0000_0010u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);
    cpu.step(&mut memory);

    assert_eq!(cpu.a_regs[0], 0x0000_00EC);
    assert_eq!(cpu.unknown_opcode_total(), 0);
}

#[test]
fn cmpi_sets_negative_and_carry_on_underflow() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #0, d0
    rom[0x100..0x102].copy_from_slice(&0x7000u16.to_be_bytes());
    // cmpi.w #1, d0
    rom[0x102..0x104].copy_from_slice(&0x0C40u16.to_be_bytes());
    rom[0x104..0x106].copy_from_slice(&0x0001u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);

    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_ne!(cpu.sr() & CCR_C, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
}

#[test]
fn cmpa_long_with_immediate_updates_flags_without_modifying_address_register() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$000001F4, a1
    rom[0x100..0x102].copy_from_slice(&0x227Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x0000_01F4u32.to_be_bytes());
    // cmpa.l #$000001F0, a1
    rom[0x106..0x108].copy_from_slice(&0xB3FCu16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x0000_01F0u32.to_be_bytes());
    // cmpa.l #$000001F4, a1
    rom[0x10C..0x10E].copy_from_slice(&0xB3FCu16.to_be_bytes());
    rom[0x10E..0x112].copy_from_slice(&0x0000_01F4u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory); // movea.l
    cpu.step(&mut memory); // cmpa.l a1 - 0x1F0
    assert_eq!(cpu.a_regs[1], 0x0000_01F4);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);

    cpu.step(&mut memory); // cmpa.l a1 - 0x1F4
    assert_eq!(cpu.a_regs[1], 0x0000_01F4);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_ne!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn cmpi_byte_supports_memory_destination_and_updates_flags() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0040, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // cmpi.b #$20, (a0)
    rom[0x106..0x108].copy_from_slice(&0x0C10u16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x0020u16.to_be_bytes());
    // cmpi.b #$7F, (a0)
    rom[0x10A..0x10C].copy_from_slice(&0x0C10u16.to_be_bytes());
    rom[0x10C..0x10E].copy_from_slice(&0x007Fu16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    memory.write_u8(0x00FF_0040, 0x20);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory); // movea.l
    cpu.step(&mut memory); // cmpi.b equal
    assert_ne!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);

    cpu.step(&mut memory); // cmpi.b 0x20 - 0x7F
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_ne!(cpu.sr() & CCR_C, 0);
}

#[test]
fn cmp_word_and_long_support_an_source() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00000003, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x0000_0003u32.to_be_bytes());
    // moveq #5, d0
    rom[0x106..0x108].copy_from_slice(&0x7005u16.to_be_bytes());
    // cmp.w a0, d0
    rom[0x108..0x10A].copy_from_slice(&0xB048u16.to_be_bytes());
    // cmp.l a0, d0
    rom[0x10A..0x10C].copy_from_slice(&0xB088u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.unknown_opcode_total(), 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn services_vdp_level6_interrupt_when_unmasked() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // Autovector level 6
    rom[0x78..0x7C].copy_from_slice(&0x0000_0200u32.to_be_bytes());
    rom[0x100..0x102].copy_from_slice(&0x4E71u16.to_be_bytes());
    rom[0x200..0x202].copy_from_slice(&0x4E71u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);
    cpu.sr = SR_SUPERVISOR; // Interrupt mask = 0

    // Register 1 = 0x60 (display + V-INT enable)
    memory.write_u16(0xC00004, 0x8160);
    assert!(memory.step_vdp(127_800));

    let cycles = cpu.step(&mut memory);
    assert_eq!(cycles, 44);
    assert_eq!(cpu.pc(), 0x0000_0200);
    assert_eq!((cpu.sr & SR_INT_MASK) >> 8, 6);
    assert_eq!(cpu.a_regs[7], 0x00FF_0FFA);
}

#[test]
fn trap_and_rte_round_trip_to_handler_and_back() {
    let mut rom = vec![0u8; 0x1200];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // TRAP #1 vector (32 + 1 = 33)
    rom[0x84..0x88].copy_from_slice(&0x0000_0200u32.to_be_bytes());

    // trap #1
    rom[0x100..0x102].copy_from_slice(&0x4E41u16.to_be_bytes());
    // move.w #$1111, $00FF0000
    rom[0x102..0x104].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x104..0x106].copy_from_slice(&0x1111u16.to_be_bytes());
    rom[0x106..0x10A].copy_from_slice(&0x00FF_0000u32.to_be_bytes());

    // handler: move.w #$2222, $00FF0002 ; rte
    rom[0x200..0x202].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x202..0x204].copy_from_slice(&0x2222u16.to_be_bytes());
    rom[0x204..0x208].copy_from_slice(&0x00FF_0002u32.to_be_bytes());
    rom[0x208..0x20A].copy_from_slice(&0x4E73u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    let trap_cycles = cpu.step(&mut memory);
    assert_eq!(trap_cycles, 34);
    assert_eq!(cpu.pc(), 0x0000_0200);

    cpu.step(&mut memory); // handler move.w
    assert_eq!(memory.read_u16(0x00FF_0002), 0x2222);

    let rte_cycles = cpu.step(&mut memory);
    assert_eq!(rte_cycles, 20);
    assert_eq!(cpu.pc(), 0x0000_0102);
    assert_eq!(cpu.sr(), 0x2700);

    cpu.step(&mut memory); // post-trap mainline move.w
    assert_eq!(memory.read_u16(0x00FF_0000), 0x1111);
}

#[test]
fn link_and_unlk_manage_stack_frame() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0100, a7
    rom[0x100..0x102].copy_from_slice(&0x2E7Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0100u32.to_be_bytes());
    // movea.l #$00FF0200, a6
    rom[0x106..0x108].copy_from_slice(&0x2C7Cu16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x00FF_0200u32.to_be_bytes());
    // link a6, #-8
    rom[0x10C..0x10E].copy_from_slice(&0x4E56u16.to_be_bytes());
    rom[0x10E..0x110].copy_from_slice(&0xFFF8u16.to_be_bytes());
    // unlk a6
    rom[0x110..0x112].copy_from_slice(&0x4E5Eu16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.read_u32(0x00FF_00FC), 0x00FF_0200);
    assert_eq!(cpu.a_regs[6], 0x00FF_0200);
    assert_eq!(cpu.a_regs[7], 0x00FF_0100);
}

#[test]
fn move_to_and_from_sr_supports_immediate_register_and_memory() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.w #$A71F, sr
    rom[0x100..0x102].copy_from_slice(&0x46FCu16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0xA71Fu16.to_be_bytes());
    // move.w sr, d0
    rom[0x104..0x106].copy_from_slice(&0x40C0u16.to_be_bytes());
    // move.w sr, $00FF0000
    rom[0x106..0x108].copy_from_slice(&0x40F9u16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x00FF_0000u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..3 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.sr(), 0xA71F);
    assert_eq!(cpu.d_regs[0] & 0xFFFF, 0xA71F);
    assert_eq!(memory.read_u16(0x00FF_0000), 0xA71F);
}

#[test]
fn move_usp_transfers_stack_pointer_with_privileged_opcode() {
    let mut rom = vec![0u8; 0x600];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0200, a1
    rom[0x100..0x102].copy_from_slice(&0x227Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0200u32.to_be_bytes());
    // move a1, usp
    rom[0x106..0x108].copy_from_slice(&0x4E61u16.to_be_bytes());
    // movea.l #0, a1
    rom[0x108..0x10A].copy_from_slice(&0x227Cu16.to_be_bytes());
    rom[0x10A..0x10E].copy_from_slice(&0x0000_0000u32.to_be_bytes());
    // move usp, a1
    rom[0x10E..0x110].copy_from_slice(&0x4E69u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..5 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.unknown_opcode_total(), 0);
    assert_eq!(cpu.usp, 0x00FF_0200);
    assert_eq!(cpu.a_regs[1], 0x00FF_0200);
}

#[test]
fn immediate_sr_operations_are_privileged_and_ccr_operations_are_not() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // Privilege violation vector
    rom[0x20..0x24].copy_from_slice(&0x0000_0200u32.to_be_bytes());

    // ori #$0011, ccr
    rom[0x100..0x102].copy_from_slice(&0x003Cu16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0x0011u16.to_be_bytes());
    // andi #$0015, ccr
    rom[0x104..0x106].copy_from_slice(&0x023Cu16.to_be_bytes());
    rom[0x106..0x108].copy_from_slice(&0x0015u16.to_be_bytes());
    // eori #$0004, ccr
    rom[0x108..0x10A].copy_from_slice(&0x0A3Cu16.to_be_bytes());
    rom[0x10A..0x10C].copy_from_slice(&0x0004u16.to_be_bytes());
    // ori #$2000, sr (must trap in user mode)
    rom[0x10C..0x10E].copy_from_slice(&0x007Cu16.to_be_bytes());
    rom[0x10E..0x110].copy_from_slice(&0x2000u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);
    cpu.sr &= !SR_SUPERVISOR;

    cpu.step(&mut memory); // ori to ccr
    assert_eq!(cpu.sr() & 0x001F, 0x0011);

    cpu.step(&mut memory); // andi to ccr
    assert_eq!(cpu.sr() & 0x001F, 0x0011);

    cpu.step(&mut memory); // eori to ccr
    assert_eq!(cpu.sr() & 0x001F, 0x0015);

    let cycles = cpu.step(&mut memory); // ori to sr => privilege violation
    assert_eq!(cycles, 34);
    assert_eq!(cpu.pc(), 0x0000_0200);
    assert_eq!(cpu.exception_histogram.get(&8).copied(), Some(1));
}

#[test]
fn swap_and_ext_transform_register_values_and_flags() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.l #$1234ABCD, d0
    rom[0x100..0x102].copy_from_slice(&0x203Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x1234_ABCDu32.to_be_bytes());
    // swap d0
    rom[0x106..0x108].copy_from_slice(&0x4840u16.to_be_bytes());
    // moveq #-128, d1
    rom[0x108..0x10A].copy_from_slice(&0x7280u16.to_be_bytes());
    // ext.w d1
    rom[0x10A..0x10C].copy_from_slice(&0x4881u16.to_be_bytes());
    // ext.l d1
    rom[0x10C..0x10E].copy_from_slice(&0x48C1u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..5 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0], 0xABCD_1234);
    assert_eq!(cpu.d_regs[1], 0xFFFF_FF80);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & CCR_V, 0);
    assert_eq!(cpu.sr() & CCR_C, 0);
}

#[test]
fn movem_long_predecrement_and_postincrement_round_trip() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0100, a7
    rom[0x100..0x102].copy_from_slice(&0x2E7Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0100u32.to_be_bytes());
    // move.l #$11223344, d0
    rom[0x106..0x108].copy_from_slice(&0x203Cu16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x1122_3344u32.to_be_bytes());
    // movea.l #$55667788, a0
    rom[0x10C..0x10E].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x10E..0x112].copy_from_slice(&0x5566_7788u32.to_be_bytes());
    // movem.l d0/a0, -(a7) ; mask uses predecrement bit ordering
    rom[0x112..0x114].copy_from_slice(&0x48E7u16.to_be_bytes());
    rom[0x114..0x116].copy_from_slice(&0x8080u16.to_be_bytes());
    // moveq #0, d0
    rom[0x116..0x118].copy_from_slice(&0x7000u16.to_be_bytes());
    // movea.l #0, a0
    rom[0x118..0x11A].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x11A..0x11E].copy_from_slice(&0x0000_0000u32.to_be_bytes());
    // movem.l (a7)+, d0/a0
    rom[0x11E..0x120].copy_from_slice(&0x4CDFu16.to_be_bytes());
    rom[0x120..0x122].copy_from_slice(&0x0101u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..7 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0], 0x1122_3344);
    assert_eq!(cpu.a_regs[0], 0x5566_7788);
    assert_eq!(cpu.a_regs[7], 0x00FF_0100);
}

#[test]
fn movem_word_from_memory_sign_extends_registers() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.w #$FF80, $00FF0040
    rom[0x100..0x102].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0xFF80u16.to_be_bytes());
    rom[0x104..0x108].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // move.w #$007F, $00FF0042
    rom[0x108..0x10A].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x10A..0x10C].copy_from_slice(&0x007Fu16.to_be_bytes());
    rom[0x10C..0x110].copy_from_slice(&0x00FF_0042u32.to_be_bytes());
    // movea.l #$00FF0040, a0
    rom[0x110..0x112].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x112..0x116].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // movem.w (a0), d0-d1
    rom[0x116..0x118].copy_from_slice(&0x4C90u16.to_be_bytes());
    rom[0x118..0x11A].copy_from_slice(&0x0003u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0], 0xFFFF_FF80);
    assert_eq!(cpu.d_regs[1], 0x0000_007F);
}

#[test]
fn pea_pushes_effective_addresses_onto_stack() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0100, a7
    rom[0x100..0x102].copy_from_slice(&0x2E7Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0100u32.to_be_bytes());
    // movea.l #$00FF0200, a0
    rom[0x106..0x108].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x108..0x10C].copy_from_slice(&0x00FF_0200u32.to_be_bytes());
    // pea (4,a0)
    rom[0x10C..0x10E].copy_from_slice(&0x4868u16.to_be_bytes());
    rom[0x10E..0x110].copy_from_slice(&0x0004u16.to_be_bytes());
    // pea $00FF0300.l
    rom[0x110..0x112].copy_from_slice(&0x4879u16.to_be_bytes());
    rom[0x112..0x116].copy_from_slice(&0x00FF_0300u32.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.a_regs[7], 0x00FF_00F8);
    assert_eq!(memory.read_u32(0x00FF_00F8), 0x00FF_0300);
    assert_eq!(memory.read_u32(0x00FF_00FC), 0x00FF_0204);
}

#[test]
fn bit_ops_immediate_and_dynamic_support_register_and_memory_targets() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #0, d0
    rom[0x100..0x102].copy_from_slice(&0x7000u16.to_be_bytes());
    // bset #1, d0
    rom[0x102..0x104].copy_from_slice(&0x08C0u16.to_be_bytes());
    rom[0x104..0x106].copy_from_slice(&0x0001u16.to_be_bytes());
    // bchg #1, d0
    rom[0x106..0x108].copy_from_slice(&0x0840u16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x0001u16.to_be_bytes());
    // bclr #2, d0
    rom[0x10A..0x10C].copy_from_slice(&0x0880u16.to_be_bytes());
    rom[0x10C..0x10E].copy_from_slice(&0x0002u16.to_be_bytes());
    // moveq #3, d1
    rom[0x10E..0x110].copy_from_slice(&0x7203u16.to_be_bytes());
    // bset d1, d0
    rom[0x110..0x112].copy_from_slice(&0x03C0u16.to_be_bytes());
    // btst d1, d0
    rom[0x112..0x114].copy_from_slice(&0x0300u16.to_be_bytes());
    // movea.l #$00FF0040, a0
    rom[0x114..0x116].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x116..0x11A].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // bset #2, (a0)
    rom[0x11A..0x11C].copy_from_slice(&0x08D0u16.to_be_bytes());
    rom[0x11C..0x11E].copy_from_slice(&0x0002u16.to_be_bytes());
    // bchg d1, (a0)
    rom[0x11E..0x120].copy_from_slice(&0x0350u16.to_be_bytes());
    // btst #2, (a0)
    rom[0x120..0x122].copy_from_slice(&0x0810u16.to_be_bytes());
    rom[0x122..0x124].copy_from_slice(&0x0002u16.to_be_bytes());
    // bclr #3, (a0)
    rom[0x124..0x126].copy_from_slice(&0x0890u16.to_be_bytes());
    rom[0x126..0x128].copy_from_slice(&0x0003u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    memory.write_u8(0x00FF_0040, 0x00);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..12 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0], 0x0000_0008);
    assert_eq!(memory.read_u8(0x00FF_0040), 0x04);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & (CCR_N | CCR_V | CCR_C), 0);
}

#[test]
fn tst_byte_supports_register_and_memory_effective_addresses() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #-1, d0
    rom[0x100..0x102].copy_from_slice(&0x70FFu16.to_be_bytes());
    // tst.b d0
    rom[0x102..0x104].copy_from_slice(&0x4A00u16.to_be_bytes());
    // moveq #0, d0
    rom[0x104..0x106].copy_from_slice(&0x7000u16.to_be_bytes());
    // tst.b d0
    rom[0x106..0x108].copy_from_slice(&0x4A00u16.to_be_bytes());
    // movea.l #$00FF0050, a0
    rom[0x108..0x10A].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x10A..0x10E].copy_from_slice(&0x00FF_0050u32.to_be_bytes());
    // tst.b (a0)
    rom[0x10E..0x110].copy_from_slice(&0x4A10u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    memory.write_u8(0x00FF_0050, 0x80);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory); // moveq #-1
    cpu.step(&mut memory); // tst.b d0
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);

    cpu.step(&mut memory); // moveq #0
    cpu.step(&mut memory); // tst.b d0
    assert_eq!(cpu.sr() & CCR_N, 0);
    assert_ne!(cpu.sr() & CCR_Z, 0);

    cpu.step(&mut memory); // movea.l
    cpu.step(&mut memory); // tst.b (a0)
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
    assert_eq!(memory.read_u8(0x00FF_0050), 0x80);
}

#[test]
fn clr_byte_clears_register_and_postincrement_memory_destination() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.l #$12345678, d0
    rom[0x100..0x102].copy_from_slice(&0x203Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x1234_5678u32.to_be_bytes());
    // clr.b d0
    rom[0x106..0x108].copy_from_slice(&0x4200u16.to_be_bytes());
    // movea.l #$00FF0060, a0
    rom[0x108..0x10A].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x10A..0x10E].copy_from_slice(&0x00FF_0060u32.to_be_bytes());
    // clr.b (a0)+
    rom[0x10E..0x110].copy_from_slice(&0x4218u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    memory.write_u8(0x00FF_0060, 0xAA);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0], 0x1234_5600);
    assert_eq!(memory.read_u8(0x00FF_0060), 0x00);
    assert_eq!(cpu.a_regs[0], 0x00FF_0061);
    assert_ne!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.sr() & (CCR_N | CCR_V | CCR_C), 0);
}

#[test]
fn move_word_supports_an_indexed_source_and_destination() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0040, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // moveq #2, d1
    rom[0x106..0x108].copy_from_slice(&0x7202u16.to_be_bytes());
    // move.w (6,a0,d1.w), d0
    rom[0x108..0x10A].copy_from_slice(&0x3030u16.to_be_bytes());
    rom[0x10A..0x10C].copy_from_slice(&0x1006u16.to_be_bytes());
    // clr.b (4,a0,d1.w)
    rom[0x10C..0x10E].copy_from_slice(&0x4230u16.to_be_bytes());
    rom[0x10E..0x110].copy_from_slice(&0x1004u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    memory.write_u16(0x00FF_0048, 0xCAFE);
    memory.write_u8(0x00FF_0046, 0xAA);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0] & 0xFFFF, 0xCAFE);
    assert_eq!(memory.read_u8(0x00FF_0046), 0x00);
}

#[test]
fn move_word_supports_pc_relative_and_pc_indexed_sources() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.w (12,pc), d0
    rom[0x100..0x102].copy_from_slice(&0x303Au16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0x000Cu16.to_be_bytes());
    // moveq #2, d1
    rom[0x104..0x106].copy_from_slice(&0x7202u16.to_be_bytes());
    // move.w (8,pc,d1.w), d2
    rom[0x106..0x108].copy_from_slice(&0x343Bu16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x1008u16.to_be_bytes());
    // nop
    rom[0x10A..0x10C].copy_from_slice(&0x4E71u16.to_be_bytes());
    // data words read by PC-relative modes
    rom[0x10E..0x110].copy_from_slice(&0xBEEFu16.to_be_bytes());
    rom[0x112..0x114].copy_from_slice(&0x1234u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..3 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0] & 0xFFFF, 0xBEEF);
    assert_eq!(cpu.d_regs[2] & 0xFFFF, 0x1234);
}

#[test]
fn lea_supports_indexed_an_and_pc_relative_modes() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0100, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0100u32.to_be_bytes());
    // moveq #3, d1
    rom[0x106..0x108].copy_from_slice(&0x7203u16.to_be_bytes());
    // lea (4,a0,d1.w), a2
    rom[0x108..0x10A].copy_from_slice(&0x45F0u16.to_be_bytes());
    rom[0x10A..0x10C].copy_from_slice(&0x1004u16.to_be_bytes());
    // lea (6,pc), a3
    rom[0x10C..0x10E].copy_from_slice(&0x47FAu16.to_be_bytes());
    rom[0x10E..0x110].copy_from_slice(&0x0006u16.to_be_bytes());
    // nop
    rom[0x110..0x112].copy_from_slice(&0x4E71u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..5 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.a_regs[2], 0x00FF_0107);
    assert_eq!(cpu.a_regs[3], 0x0000_0114);
}

#[test]
fn executes_shift_and_rotate_register_forms_used_by_roms() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #1, d1
    rom[0x100..0x102].copy_from_slice(&0x7201u16.to_be_bytes());
    // ror.b #1, d1  (E219)
    rom[0x102..0x104].copy_from_slice(&0xE219u16.to_be_bytes());
    // moveq #1, d2
    rom[0x104..0x106].copy_from_slice(&0x7401u16.to_be_bytes());
    // rol.l #4, d2  (E99A)
    rom[0x106..0x108].copy_from_slice(&0xE99Au16.to_be_bytes());
    // move.b #$C0, d0
    rom[0x108..0x10A].copy_from_slice(&0x103Cu16.to_be_bytes());
    rom[0x10A..0x10C].copy_from_slice(&0x00C0u16.to_be_bytes());
    // lsr.b #6, d0  (EC08)
    rom[0x10C..0x10E].copy_from_slice(&0xEC08u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..6 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[1] & 0xFF, 0x80);
    assert_eq!(cpu.d_regs[2], 0x0000_0010);
    assert_eq!(cpu.d_regs[0] & 0xFF, 0x03);
    assert_eq!(cpu.unknown_opcode_total(), 0);
}

#[test]
fn executes_roxl_and_roxr_register_forms() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.w #$0010, d7 (set X via move to CCR)
    rom[0x100..0x102].copy_from_slice(&0x3E3Cu16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0x0010u16.to_be_bytes());
    // move.w d7, ccr
    rom[0x104..0x106].copy_from_slice(&0x44C7u16.to_be_bytes());
    // moveq #-128, d0
    rom[0x106..0x108].copy_from_slice(&0x7080u16.to_be_bytes());
    // roxl.b #1, d0
    rom[0x108..0x10A].copy_from_slice(&0xE310u16.to_be_bytes());
    // roxr.b #1, d0
    rom[0x10A..0x10C].copy_from_slice(&0xE210u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..5 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[0] & 0xFF, 0x80);
    assert_ne!(cpu.sr() & CCR_X, 0);
    assert_ne!(cpu.sr() & CCR_C, 0);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
}

#[test]
fn executes_memory_shift_form_with_displacement_extension_word() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0040, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // asr.w (16,a0)  (E0E8 0010)
    rom[0x106..0x108].copy_from_slice(&0xE0E8u16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x0010u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);
    memory.write_u16(0x00FF_0050, 0x8001);

    cpu.step(&mut memory); // movea.l
    cpu.step(&mut memory); // asr.w (16,a0)

    assert_eq!(memory.read_u16(0x00FF_0050), 0xC000);
    assert_eq!(cpu.pc(), 0x0000_010A);
    assert_eq!(cpu.unknown_opcode_total(), 0);
    assert_ne!(cpu.sr() & CCR_X, 0);
    assert_ne!(cpu.sr() & CCR_C, 0);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
}

#[test]
fn move_to_ccr_updates_condition_code_bits() {
    let mut rom = vec![0u8; 0x600];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // move.w #$0011, d0
    rom[0x100..0x102].copy_from_slice(&0x303Cu16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0x0011u16.to_be_bytes());
    // move.w d0, ccr
    rom[0x104..0x106].copy_from_slice(&0x44C0u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    cpu.step(&mut memory);
    cpu.step(&mut memory);

    assert_eq!(cpu.sr() & 0x001F, 0x0011);
}

#[test]
fn neg_and_not_are_decoded_and_update_results() {
    let mut rom = vec![0u8; 0x700];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // moveq #1, d6
    rom[0x100..0x102].copy_from_slice(&0x7C01u16.to_be_bytes());
    // neg.w d6 (4446)
    rom[0x102..0x104].copy_from_slice(&0x4446u16.to_be_bytes());
    // moveq #0, d0
    rom[0x104..0x106].copy_from_slice(&0x7000u16.to_be_bytes());
    // not.b d0 (4600)
    rom[0x106..0x108].copy_from_slice(&0x4600u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[6] & 0xFFFF, 0xFFFF);
    assert_eq!(cpu.d_regs[0] & 0xFF, 0xFF);
    assert_eq!(cpu.unknown_opcode_total(), 0);
}

#[test]
fn neg_and_not_memory_modes_consume_displacement_once() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0040, a1
    rom[0x100..0x102].copy_from_slice(&0x227Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0040u32.to_be_bytes());
    // move.w #$0001, $00FF0042
    rom[0x106..0x108].copy_from_slice(&0x33FCu16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x0001u16.to_be_bytes());
    rom[0x10A..0x10E].copy_from_slice(&0x00FF_0042u32.to_be_bytes());
    // neg.w (2,a1)
    rom[0x10E..0x110].copy_from_slice(&0x4469u16.to_be_bytes());
    rom[0x110..0x112].copy_from_slice(&0x0002u16.to_be_bytes());
    // not.w (2,a1)
    rom[0x112..0x114].copy_from_slice(&0x4669u16.to_be_bytes());
    rom[0x114..0x116].copy_from_slice(&0x0002u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..4 {
        cpu.step(&mut memory);
    }

    assert_eq!(memory.read_u16(0x00FF_0042), 0x0000);
    assert_eq!(cpu.pc(), 0x0000_0116);
    assert_eq!(cpu.unknown_opcode_total(), 0);
}

#[test]
fn movep_word_load_and_store_use_interleaved_bytes() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0000, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
    // movep.w (0,a0), d3
    rom[0x106..0x108].copy_from_slice(&0x0708u16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x0000u16.to_be_bytes());
    // movep.w d3, (4,a0)
    rom[0x10A..0x10C].copy_from_slice(&0x0788u16.to_be_bytes());
    rom[0x10C..0x10E].copy_from_slice(&0x0004u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    memory.write_u8(0x00FF_0000, 0x12);
    memory.write_u8(0x00FF_0002, 0x34);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..3 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[3] & 0xFFFF, 0x1234);
    assert_eq!(memory.read_u8(0x00FF_0004), 0x12);
    assert_eq!(memory.read_u8(0x00FF_0006), 0x34);
}

#[test]
fn movep_long_load_and_store_use_interleaved_bytes() {
    let mut rom = vec![0u8; 0x800];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());

    // movea.l #$00FF0000, a0
    rom[0x100..0x102].copy_from_slice(&0x207Cu16.to_be_bytes());
    rom[0x102..0x106].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
    // movep.l (0,a0), d3
    rom[0x106..0x108].copy_from_slice(&0x0748u16.to_be_bytes());
    rom[0x108..0x10A].copy_from_slice(&0x0000u16.to_be_bytes());
    // movep.l d3, (8,a0)
    rom[0x10A..0x10C].copy_from_slice(&0x07C8u16.to_be_bytes());
    rom[0x10C..0x10E].copy_from_slice(&0x0008u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    memory.write_u8(0x00FF_0000, 0x11);
    memory.write_u8(0x00FF_0002, 0x22);
    memory.write_u8(0x00FF_0004, 0x33);
    memory.write_u8(0x00FF_0006, 0x44);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    for _ in 0..3 {
        cpu.step(&mut memory);
    }

    assert_eq!(cpu.d_regs[3], 0x1122_3344);
    assert_eq!(memory.read_u8(0x00FF_0008), 0x11);
    assert_eq!(memory.read_u8(0x00FF_000A), 0x22);
    assert_eq!(memory.read_u8(0x00FF_000C), 0x33);
    assert_eq!(memory.read_u8(0x00FF_000E), 0x44);
}

#[test]
fn illegal_opcode_vectors_to_exception_4() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // Illegal instruction vector #4
    rom[0x10..0x14].copy_from_slice(&0x0000_0180u32.to_be_bytes());
    rom[0x100..0x102].copy_from_slice(&0x4AFCu16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    let cycles = cpu.step(&mut memory);
    assert_eq!(cycles, 34);
    assert_eq!(cpu.pc(), 0x0000_0180);
    assert_eq!(cpu.a_regs[7], 0x00FF_0FFA);
    assert_eq!(memory.read_u16(0x00FF_0FFA), 0x2700);
    assert_eq!(memory.read_u32(0x00FF_0FFC), 0x0000_0102);
}

#[test]
fn trapv_vectors_only_when_overflow_is_set() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // Trap #7 vector.
    rom[0x1C..0x20].copy_from_slice(&0x0000_0180u32.to_be_bytes());
    rom[0x100..0x102].copy_from_slice(&0x4E76u16.to_be_bytes()); // trapv

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    // V clear: no trap.
    let cycles_no_trap = cpu.step(&mut memory);
    assert_eq!(cycles_no_trap, 4);
    assert_eq!(cpu.pc(), 0x0000_0102);

    cpu.pc = 0x0000_0100;
    cpu.sr |= CCR_V;
    let cycles_trap = cpu.step(&mut memory);
    assert_eq!(cycles_trap, 34);
    assert_eq!(cpu.pc(), 0x0000_0180);
}

#[test]
fn rtr_restores_ccr_and_pc_from_stack() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    rom[0x100..0x102].copy_from_slice(&0x4E77u16.to_be_bytes()); // rtr

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    memory.write_u16(cpu.a_regs[7], 0x0015);
    memory.write_u32(cpu.a_regs[7] + 2, 0x0000_0120);
    let cycles = cpu.step(&mut memory);
    assert_eq!(cycles, 20);
    assert_eq!(cpu.pc(), 0x0000_0120);
    assert_eq!(cpu.sr() & 0x001F, 0x0015);
}

#[test]
fn negx_byte_uses_extend_and_updates_flags() {
    let mut rom = vec![0u8; 0x400];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    rom[0x100..0x102].copy_from_slice(&0x4000u16.to_be_bytes()); // negx.b d0

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);
    cpu.d_regs[0] = 0x0000_0000;
    cpu.sr |= CCR_X | CCR_Z;

    cpu.step(&mut memory);
    assert_eq!(cpu.d_regs[0] & 0xFF, 0xFF);
    assert_ne!(cpu.sr() & CCR_X, 0);
    assert_ne!(cpu.sr() & CCR_C, 0);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);
}

#[test]
fn nbcd_and_tas_are_decoded() {
    let mut rom = vec![0u8; 0x500];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    rom[0x100..0x102].copy_from_slice(&0x4800u16.to_be_bytes()); // nbcd d0
    rom[0x102..0x104].copy_from_slice(&0x4AC1u16.to_be_bytes()); // tas d1

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);
    cpu.d_regs[0] = 0x0000_0001;
    cpu.d_regs[1] = 0x0000_0001;
    cpu.sr |= CCR_Z;

    cpu.step(&mut memory);
    assert_eq!(cpu.d_regs[0] & 0xFF, 0x99);
    assert_ne!(cpu.sr() & CCR_C, 0);
    assert_ne!(cpu.sr() & CCR_X, 0);
    assert_eq!(cpu.sr() & CCR_Z, 0);

    cpu.step(&mut memory);
    assert_eq!(cpu.d_regs[1] & 0xFF, 0x81);
    assert_eq!(cpu.unknown_opcode_total(), 0);
}

#[test]
fn chk_w_raises_vector_6_for_negative_or_out_of_range() {
    let mut rom = vec![0u8; 0x500];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // CHK vector #6
    rom[0x18..0x1C].copy_from_slice(&0x0000_0180u32.to_be_bytes());
    // chk.w d1,d0
    rom[0x100..0x102].copy_from_slice(&0x4181u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);
    cpu.d_regs[1] = 10;

    cpu.d_regs[0] = 5;
    let ok_cycles = cpu.step(&mut memory);
    assert_eq!(ok_cycles, 10);
    assert_eq!(cpu.pc(), 0x0000_0102);

    cpu.pc = 0x0000_0100;
    cpu.d_regs[0] = 11;
    let trap_cycles = cpu.step(&mut memory);
    assert_eq!(trap_cycles, 40);
    assert_eq!(cpu.pc(), 0x0000_0180);

    cpu.pc = 0x0000_0100;
    cpu.a_regs[7] = cpu.ssp;
    cpu.d_regs[0] = 0xFFFF_FFFF;
    let trap_neg_cycles = cpu.step(&mut memory);
    assert_eq!(trap_neg_cycles, 40);
    assert_eq!(cpu.pc(), 0x0000_0180);
}

#[test]
fn reset_requires_supervisor_mode() {
    let mut rom = vec![0u8; 0x500];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // Privilege violation vector #8
    rom[0x20..0x24].copy_from_slice(&0x0000_0180u32.to_be_bytes());
    rom[0x100..0x102].copy_from_slice(&0x4E70u16.to_be_bytes()); // reset

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    let sup_cycles = cpu.step(&mut memory);
    assert_eq!(sup_cycles, 132);
    assert_eq!(cpu.pc(), 0x0000_0102);

    cpu.pc = 0x0000_0100;
    cpu.sr &= !SR_SUPERVISOR;
    let user_cycles = cpu.step(&mut memory);
    assert_eq!(user_cycles, 34);
    assert_eq!(cpu.pc(), 0x0000_0180);
}

#[test]
fn reset_instruction_pulses_external_reset_line() {
    let mut rom = vec![0u8; 0x500];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    rom[0x100..0x102].copy_from_slice(&0x4E70u16.to_be_bytes()); // reset

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    // Run Z80 first so we can verify RESET drives it back to initial state.
    memory.write_u16(0xA11200, 0x0100); // release reset
    memory.write_u16(0xA11100, 0x0000); // bus owned by Z80
    memory.step_subsystems(64);
    assert!(memory.z80().pc() > 0);

    let cycles = cpu.step(&mut memory);
    assert_eq!(cycles, 132);
    assert_eq!(memory.z80().read_reset_byte(), 0x01);
    assert_eq!(memory.z80().pc(), 0);
}

#[test]
fn addx_and_subx_data_register_mode_are_decoded() {
    let mut rom = vec![0u8; 0x500];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // addx.b d1,d0
    rom[0x100..0x102].copy_from_slice(&0xD101u16.to_be_bytes());
    // subx.b d1,d0
    rom[0x102..0x104].copy_from_slice(&0x9101u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);
    cpu.d_regs[0] = 0x0000_0010;
    cpu.d_regs[1] = 0x0000_0001;
    cpu.sr |= CCR_X | CCR_Z;

    cpu.step(&mut memory);
    assert_eq!(cpu.d_regs[0] & 0xFF, 0x12);
    assert_eq!(cpu.sr() & CCR_Z, 0);

    cpu.step(&mut memory);
    assert_eq!(cpu.d_regs[0] & 0xFF, 0x11);
    assert_eq!(cpu.unknown_opcode_total(), 0);
}

#[test]
fn addx_subx_memory_predecrement_mode_updates_memory() {
    let mut rom = vec![0u8; 0x600];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // addx.b -(a0),-(a1)
    rom[0x100..0x102].copy_from_slice(&0xD308u16.to_be_bytes());
    // subx.b -(a0),-(a1)
    rom[0x102..0x104].copy_from_slice(&0x9308u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);
    cpu.a_regs[0] = 0x00FF_0012;
    cpu.a_regs[1] = 0x00FF_0022;
    memory.write_u8(0x00FF_0011, 0x01);
    memory.write_u8(0x00FF_0021, 0x10);
    memory.write_u8(0x00FF_0010, 0x01);
    memory.write_u8(0x00FF_0020, 0x12);
    cpu.sr &= !CCR_X;
    cpu.sr |= CCR_Z;

    cpu.step(&mut memory);
    assert_eq!(memory.read_u8(0x00FF_0021), 0x11);
    assert_eq!(cpu.a_regs[0], 0x00FF_0011);
    assert_eq!(cpu.a_regs[1], 0x00FF_0021);

    cpu.step(&mut memory);
    assert_eq!(memory.read_u8(0x00FF_0020), 0x11);
    assert_eq!(cpu.a_regs[0], 0x00FF_0010);
    assert_eq!(cpu.a_regs[1], 0x00FF_0020);
    assert_eq!(cpu.unknown_opcode_total(), 0);
}

#[test]
fn cmpm_byte_word_long_are_decoded() {
    let mut rom = vec![0u8; 0x700];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // cmpm.b (a1)+,(a0)+
    rom[0x100..0x102].copy_from_slice(&0xB109u16.to_be_bytes());
    // cmpm.w (a1)+,(a0)+
    rom[0x102..0x104].copy_from_slice(&0xB149u16.to_be_bytes());
    // cmpm.l (a1)+,(a0)+
    rom[0x104..0x106].copy_from_slice(&0xB189u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);
    cpu.a_regs[0] = 0x00FF_0100;
    cpu.a_regs[1] = 0x00FF_0200;
    // byte compare: 0x10 - 0x20 => negative
    memory.write_u8(0x00FF_0100, 0x10);
    memory.write_u8(0x00FF_0200, 0x20);
    // word compare: 0x1234 - 0x1234 => zero
    memory.write_u16(0x00FF_0101, 0x1234);
    memory.write_u16(0x00FF_0201, 0x1234);
    // long compare: 0x00000005 - 0x00000007 => negative
    memory.write_u32(0x00FF_0103, 0x0000_0005);
    memory.write_u32(0x00FF_0203, 0x0000_0007);

    let c1 = cpu.step(&mut memory);
    assert_eq!(c1, 12);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.a_regs[0], 0x00FF_0101);
    assert_eq!(cpu.a_regs[1], 0x00FF_0201);

    let c2 = cpu.step(&mut memory);
    assert_eq!(c2, 12);
    assert_ne!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.a_regs[0], 0x00FF_0103);
    assert_eq!(cpu.a_regs[1], 0x00FF_0203);

    let c3 = cpu.step(&mut memory);
    assert_eq!(c3, 20);
    assert_ne!(cpu.sr() & CCR_N, 0);
    assert_eq!(cpu.a_regs[0], 0x00FF_0107);
    assert_eq!(cpu.a_regs[1], 0x00FF_0207);
    assert_eq!(cpu.unknown_opcode_total(), 0);
}

#[test]
fn cmpm_byte_on_a7_uses_byte_addr_step() {
    let mut rom = vec![0u8; 0x700];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // cmpm.b (a7)+,(a7)+
    rom[0x100..0x102].copy_from_slice(&0xBF0F_u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);
    cpu.a_regs[7] = 0x00FF_0300;
    memory.write_u8(0x00FF_0300, 0x11);
    memory.write_u8(0x00FF_0302, 0x11);

    cpu.step(&mut memory);
    assert_eq!(cpu.a_regs[7], 0x00FF_0304);
    assert_ne!(cpu.sr() & CCR_Z, 0);
    assert_eq!(cpu.unknown_opcode_total(), 0);
}

#[test]
fn line_a_and_line_f_vector_to_10_and_11() {
    let mut rom = vec![0u8; 0x600];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // vector 10 @ 0x28
    rom[0x28..0x2C].copy_from_slice(&0x0000_0180u32.to_be_bytes());
    // vector 11 @ 0x2C
    rom[0x2C..0x30].copy_from_slice(&0x0000_01A0u32.to_be_bytes());
    rom[0x100..0x102].copy_from_slice(&0xA000u16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0xF000u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    let c1 = cpu.step(&mut memory);
    assert_eq!(c1, 34);
    assert_eq!(cpu.pc(), 0x0000_0180);

    cpu.pc = 0x0000_0102;
    cpu.a_regs[7] = cpu.ssp;
    let c2 = cpu.step(&mut memory);
    assert_eq!(c2, 34);
    assert_eq!(cpu.pc(), 0x0000_01A0);
}

#[test]
fn bkpt_on_68000_behaves_like_illegal() {
    let mut rom = vec![0u8; 0x500];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // illegal vector #4
    rom[0x10..0x14].copy_from_slice(&0x0000_0180u32.to_be_bytes());
    // bkpt #0
    rom[0x100..0x102].copy_from_slice(&0x4848u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    let cycles = cpu.step(&mut memory);
    assert_eq!(cycles, 34);
    assert_eq!(cpu.pc(), 0x0000_0180);
    assert_eq!(cpu.unknown_opcode_total(), 0);
}

#[test]
fn stop_halts_fetch_until_interrupt() {
    let mut rom = vec![0u8; 0x600];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // Level-6 autovector
    rom[0x78..0x7C].copy_from_slice(&0x0000_0180u32.to_be_bytes());
    // stop #$2000 ; moveq #1,d0
    rom[0x100..0x102].copy_from_slice(&0x4E72u16.to_be_bytes());
    rom[0x102..0x104].copy_from_slice(&0x2000u16.to_be_bytes());
    rom[0x104..0x106].copy_from_slice(&0x7001u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);

    let stop_cycles = cpu.step(&mut memory);
    assert_eq!(stop_cycles, 4);
    assert_eq!(cpu.pc(), 0x0000_0104);

    // Still stopped: PC does not advance.
    let idle_cycles = cpu.step(&mut memory);
    assert_eq!(idle_cycles, 4);
    assert_eq!(cpu.pc(), 0x0000_0104);
    assert_eq!(cpu.d_regs[0], 0);

    // Raise VINT level 6 and ensure STOP is released by interrupt service.
    memory.write_u16(0xC00004, 0x8160); // display+vint enable
    memory.step_vdp(127_800);
    let int_cycles = cpu.step(&mut memory);
    assert_eq!(int_cycles, 44);
    assert_eq!(cpu.pc(), 0x0000_0180);
}

#[test]
fn move_from_ccr_writes_low_five_flags() {
    let mut rom = vec![0u8; 0x500];
    rom[0x0..0x4].copy_from_slice(&0x00FF_1000u32.to_be_bytes());
    rom[0x4..0x8].copy_from_slice(&0x0000_0100u32.to_be_bytes());
    // move from ccr to d0
    rom[0x100..0x102].copy_from_slice(&0x42C0u16.to_be_bytes());

    let cart = Cartridge::from_bytes(rom).expect("valid rom");
    let mut memory = MemoryMap::new(cart);
    let mut cpu = M68k::new();
    cpu.reset(&mut memory);
    cpu.sr = 0x27_1B;

    let cycles = cpu.step(&mut memory);
    assert_eq!(cycles, 6);
    assert_eq!(cpu.d_regs[0] & 0xFFFF, 0x001B);
    assert_eq!(cpu.unknown_opcode_total(), 0);
}
