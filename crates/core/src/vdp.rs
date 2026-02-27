pub const FRAME_WIDTH: usize = 320;
pub const FRAME_HEIGHT: usize = 224;

const VRAM_SIZE: usize = 0x10000;
const CRAM_COLORS: usize = 64;
const VSRAM_WORDS: usize = 40;
const TILE_SIZE_BYTES: usize = 32;
const REG_COUNT: usize = 0x20;
const REG_MODE_SET_2: usize = 1;
const REG_PLANE_A_NAMETABLE: usize = 2;
const REG_WINDOW_NAMETABLE: usize = 3;
const REG_PLANE_B_NAMETABLE: usize = 4;
const REG_SPRITE_TABLE: usize = 5;
const REG_BACKGROUND_COLOR: usize = 7;
const REG_HSCROLL_TABLE: usize = 13;
const REG_WINDOW_HPOS: usize = 17;
const REG_WINDOW_VPOS: usize = 18;
const REG_PLANE_SIZE: usize = 16;
const REG_AUTO_INCREMENT: usize = 15;
const REG_DMA_LENGTH_LOW: usize = 19;
const REG_DMA_LENGTH_HIGH: usize = 20;
const REG_DMA_SOURCE_LOW: usize = 21;
const REG_DMA_SOURCE_MID: usize = 22;
const REG_DMA_SOURCE_HIGH: usize = 23;
const STATUS_BASE: u16 = 0x3400;
const STATUS_VBLANK: u16 = 0x0008;
const STATUS_SPRITE_COLLISION: u16 = 0x0020;
const STATUS_SPRITE_OVERFLOW: u16 = 0x0040;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DmaFillState {
    remaining_words: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DmaTarget {
    Vram,
    Cram,
    Vsram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BusDmaRequest {
    pub source_addr: u32,
    pub words: usize,
    pub target: DmaTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AccessMode {
    VramRead,
    #[default]
    VramWrite,
    CramRead,
    CramWrite,
    VsramRead,
    VsramWrite,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlaneSample {
    color_index: usize,
    opaque: bool,
    priority_high: bool,
}

#[derive(Debug, Clone)]
pub struct Vdp {
    frame_cycles: u64,
    frame_count: u64,
    vblank: bool,
    sprite_collision: bool,
    sprite_overflow: bool,
    v_interrupt_pending: bool,
    vram: [u8; VRAM_SIZE],
    cram: [u16; CRAM_COLORS],
    vsram: [u16; VSRAM_WORDS],
    frame_buffer: Vec<u8>,
    registers: [u8; REG_COUNT],
    control_latch: Option<u16>,
    access_addr: u16,
    access_mode: AccessMode,
    dma_fill_pending: Option<DmaFillState>,
    dma_bus_pending: Option<BusDmaRequest>,
}

impl Default for Vdp {
    fn default() -> Self {
        let mut registers = [0u8; REG_COUNT];
        registers[REG_MODE_SET_2] = 0x40; // Display enabled
        registers[REG_PLANE_A_NAMETABLE] = 0x30; // Plane A name table base: 0xC000
        registers[REG_SPRITE_TABLE] = 0x70; // Sprite attribute table base: 0xE000
        registers[REG_HSCROLL_TABLE] = 0x3C; // Horizontal scroll table base: 0xF000
        // Window off by default (left/up split at 0 => empty region).
        registers[REG_WINDOW_HPOS] = 0x00;
        registers[REG_WINDOW_VPOS] = 0x00;
        registers[REG_AUTO_INCREMENT] = 2; // Word access by default

        let mut vdp = Self {
            frame_cycles: 0,
            frame_count: 0,
            vblank: false,
            sprite_collision: false,
            sprite_overflow: false,
            v_interrupt_pending: false,
            vram: [0; VRAM_SIZE],
            cram: [0; CRAM_COLORS],
            vsram: [0; VSRAM_WORDS],
            frame_buffer: vec![0; FRAME_WIDTH * FRAME_HEIGHT * 3],
            registers,
            control_latch: None,
            access_addr: 0,
            access_mode: AccessMode::default(),
            dma_fill_pending: None,
            dma_bus_pending: None,
        };
        vdp.seed_demo_scene();
        vdp.render_frame();
        vdp
    }
}

impl Vdp {
    // Very rough placeholder for NTSC timing (~7.67 MHz / 60 Hz).
    const CYCLES_PER_FRAME: u64 = 127_800;
    const TOTAL_LINES: u64 = 262;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&mut self, cpu_cycles: u32) -> bool {
        self.frame_cycles += cpu_cycles as u64;
        if self.frame_cycles >= Self::CYCLES_PER_FRAME {
            self.frame_cycles -= Self::CYCLES_PER_FRAME;
            self.frame_count += 1;
            self.vblank = true;
            if self.v_interrupt_enabled() {
                self.v_interrupt_pending = true;
            }
            self.render_frame();
            return true;
        }
        false
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn frame_buffer(&self) -> &[u8] {
        &self.frame_buffer
    }

    pub fn register(&self, index: usize) -> u8 {
        self.registers.get(index).copied().unwrap_or(0)
    }

    pub fn pending_interrupt_level(&self) -> Option<u8> {
        if self.v_interrupt_pending {
            Some(6)
        } else {
            None
        }
    }

    pub fn acknowledge_interrupt(&mut self, level: u8) {
        if level == 6 {
            self.v_interrupt_pending = false;
        }
    }

    pub fn read_control_port(&mut self) -> u16 {
        // Reading status clears command latch.
        self.control_latch = None;
        let mut status = STATUS_BASE;
        if self.vblank {
            status |= STATUS_VBLANK;
        }
        if self.sprite_collision {
            status |= STATUS_SPRITE_COLLISION;
        }
        if self.sprite_overflow {
            status |= STATUS_SPRITE_OVERFLOW;
        }
        self.vblank = false;
        self.sprite_collision = false;
        self.sprite_overflow = false;
        status
    }

    pub fn read_hv_counter(&self) -> u16 {
        let cycles_per_line = (Self::CYCLES_PER_FRAME / Self::TOTAL_LINES).max(1);
        let v = ((self.frame_cycles / cycles_per_line) % Self::TOTAL_LINES) as u8;
        let h = ((self.frame_cycles % cycles_per_line) * 256 / cycles_per_line) as u8;
        u16::from_be_bytes([v, h])
    }

    pub fn write_control_port(&mut self, value: u16) {
        // Register set command: 10rrrddd dddddddd
        if self.control_latch.is_none() && (value & 0xC000) == 0x8000 {
            let reg = ((value >> 8) & 0x1F) as usize;
            let data = (value & 0x00FF) as u8;
            self.write_register(reg, data);
            return;
        }

        if let Some(first) = self.control_latch.take() {
            let command = ((first as u32) << 16) | value as u32;
            self.set_access_command(command);
        } else {
            self.control_latch = Some(value);
        }
    }

    pub fn read_data_port(&mut self) -> u16 {
        let value = match self.access_mode {
            AccessMode::VramRead | AccessMode::VramWrite => {
                let hi = self.vram[self.access_addr as usize];
                let lo = self.vram[self.access_addr.wrapping_add(1) as usize];
                u16::from_be_bytes([hi, lo])
            }
            AccessMode::CramRead | AccessMode::CramWrite => {
                self.read_cram_u16((self.access_addr >> 1) as u8)
            }
            AccessMode::VsramRead | AccessMode::VsramWrite => {
                self.read_vsram_u16((self.access_addr >> 1) as u8)
            }
            AccessMode::Unsupported => 0,
        };
        self.advance_access_addr();
        value
    }

    pub fn write_data_port(&mut self, value: u16) {
        if let Some(fill) = self.dma_fill_pending.take() {
            self.execute_dma_fill(fill.remaining_words, value);
            return;
        }

        self.write_data_value(value);
        self.advance_access_addr();
    }

    fn write_data_value(&mut self, value: u16) {
        match self.access_mode {
            AccessMode::VramWrite => {
                let addr = self.access_addr as usize;
                let [hi, lo] = value.to_be_bytes();
                self.vram[addr % VRAM_SIZE] = hi;
                self.vram[(addr + 1) % VRAM_SIZE] = lo;
            }
            AccessMode::CramWrite => {
                let index = ((self.access_addr >> 1) as usize) % CRAM_COLORS;
                self.cram[index] = value & 0x0EEE;
            }
            AccessMode::VsramWrite => {
                let index = ((self.access_addr >> 1) as usize) % VSRAM_WORDS;
                self.vsram[index] = value & 0x07FF;
            }
            AccessMode::VramRead
            | AccessMode::CramRead
            | AccessMode::VsramRead
            | AccessMode::Unsupported => {}
        }
    }

    pub fn read_vram_u8(&self, addr: u16) -> u8 {
        self.vram[addr as usize]
    }

    pub fn write_vram_u8(&mut self, addr: u16, value: u8) {
        self.vram[addr as usize] = value;
    }

    pub fn read_cram_u16(&self, index: u8) -> u16 {
        let i = (index as usize) % CRAM_COLORS;
        self.cram[i]
    }

    pub fn write_cram_u16(&mut self, index: u8, value: u16) {
        let i = (index as usize) % CRAM_COLORS;
        self.cram[i] = value & 0x0EEE;
    }

    pub fn read_vsram_u16(&self, index: u8) -> u16 {
        let i = (index as usize) % VSRAM_WORDS;
        self.vsram[i]
    }

    pub fn write_vsram_u16(&mut self, index: u8, value: u16) {
        let i = (index as usize) % VSRAM_WORDS;
        self.vsram[i] = value & 0x07FF;
    }

    pub(crate) fn take_bus_dma_request(&mut self) -> Option<BusDmaRequest> {
        self.dma_bus_pending.take()
    }

    pub(crate) fn complete_bus_dma(&mut self, next_source_addr: u32) {
        self.set_dma_bus_source_addr(next_source_addr);
        self.clear_dma_length();
    }

    fn advance_access_addr(&mut self) {
        let increment = self.auto_increment();
        self.access_addr = self.access_addr.wrapping_add(increment);
    }

    fn auto_increment(&self) -> u16 {
        let increment = self.registers[REG_AUTO_INCREMENT] as u16;
        increment.max(1)
    }

    fn write_register(&mut self, reg: usize, value: u8) {
        if reg < REG_COUNT {
            let masked = match reg {
                REG_MODE_SET_2 => value & 0x7F,
                REG_PLANE_A_NAMETABLE => value & 0x38,
                REG_WINDOW_NAMETABLE => value & 0x3E,
                REG_PLANE_B_NAMETABLE => value & 0x07,
                REG_SPRITE_TABLE => value & 0x7F,
                REG_BACKGROUND_COLOR => value & 0x3F,
                REG_HSCROLL_TABLE => value & 0x3F,
                REG_WINDOW_HPOS | REG_WINDOW_VPOS => value & 0x9F,
                REG_PLANE_SIZE => value & 0x33,
                REG_AUTO_INCREMENT => value,
                REG_DMA_LENGTH_LOW | REG_DMA_LENGTH_HIGH | REG_DMA_SOURCE_LOW
                | REG_DMA_SOURCE_MID | REG_DMA_SOURCE_HIGH => value,
                _ => value,
            };
            self.registers[reg] = masked;
        }
    }

    fn set_access_command(&mut self, command: u32) {
        let code = ((command >> 30) as u8 & 0x3) | (((command >> 2) as u8) & 0x3C);
        let base_code = code & 0x1F;
        let dma_request = (code & 0x20) != 0;
        let address = (((command >> 16) & 0x3FFF) as u16) | (((command & 0x3) as u16) << 14);

        self.dma_fill_pending = None;
        self.dma_bus_pending = None;
        self.access_addr = address;
        self.access_mode = match base_code {
            0x00 => AccessMode::VramRead,
            0x01 => AccessMode::VramWrite,
            0x02 => AccessMode::CramRead,
            0x03 => AccessMode::CramWrite,
            0x04 => AccessMode::VsramRead,
            0x05 => AccessMode::VsramWrite,
            _ => AccessMode::Unsupported,
        };

        if dma_request && self.dma_enabled() {
            self.start_dma(base_code);
        }
    }

    fn dma_enabled(&self) -> bool {
        (self.registers[REG_MODE_SET_2] & 0x10) != 0
    }

    fn dma_mode(&self) -> u8 {
        let high = self.registers[REG_DMA_SOURCE_HIGH];
        if (high & 0x80) == 0 {
            // 68k bus transfer. In this mode, bit6 contributes to source address.
            0
        } else if (high & 0x40) == 0 {
            0b10
        } else {
            0b11
        }
    }

    fn dma_length(&self) -> usize {
        let len = ((self.registers[REG_DMA_LENGTH_HIGH] as usize) << 8)
            | self.registers[REG_DMA_LENGTH_LOW] as usize;
        if len == 0 { 0x10000 } else { len }
    }

    fn clear_dma_length(&mut self) {
        self.registers[REG_DMA_LENGTH_LOW] = 0;
        self.registers[REG_DMA_LENGTH_HIGH] = 0;
    }

    fn dma_source_addr(&self) -> u16 {
        ((self.registers[REG_DMA_SOURCE_MID] as u16) << 8)
            | self.registers[REG_DMA_SOURCE_LOW] as u16
    }

    fn set_dma_source_addr(&mut self, addr: u16) {
        self.registers[REG_DMA_SOURCE_LOW] = (addr & 0x00FF) as u8;
        self.registers[REG_DMA_SOURCE_MID] = (addr >> 8) as u8;
    }

    fn dma_bus_source_addr(&self) -> u32 {
        let encoded = ((self.registers[REG_DMA_SOURCE_HIGH] as u32 & 0x7F) << 16)
            | ((self.registers[REG_DMA_SOURCE_MID] as u32) << 8)
            | self.registers[REG_DMA_SOURCE_LOW] as u32;
        (encoded << 1) & 0x00FF_FFFE
    }

    fn set_dma_bus_source_addr(&mut self, addr: u32) {
        let encoded = (addr >> 1) & 0x007F_FFFF;
        self.registers[REG_DMA_SOURCE_LOW] = (encoded & 0xFF) as u8;
        self.registers[REG_DMA_SOURCE_MID] = ((encoded >> 8) & 0xFF) as u8;
        let mode = self.registers[REG_DMA_SOURCE_HIGH] & 0x80;
        self.registers[REG_DMA_SOURCE_HIGH] = mode | ((encoded >> 16) as u8 & 0x7F);
    }

    fn start_dma(&mut self, base_code: u8) {
        // DMA writes are valid for VRAM/CRAM/VSRAM write targets.
        if !matches!(
            self.access_mode,
            AccessMode::VramWrite | AccessMode::CramWrite | AccessMode::VsramWrite
        ) {
            return;
        }

        match self.dma_mode() {
            // 68k bus -> VDP transfer.
            0b00 | 0b01 => {
                let target = match self.access_mode {
                    AccessMode::VramWrite => DmaTarget::Vram,
                    AccessMode::CramWrite => DmaTarget::Cram,
                    AccessMode::VsramWrite => DmaTarget::Vsram,
                    _ => return,
                };
                self.dma_bus_pending = Some(BusDmaRequest {
                    source_addr: self.dma_bus_source_addr(),
                    words: self.dma_length(),
                    target,
                });
            }
            // DMA fill: executes when the next data-port write provides fill value.
            0b10 => {
                if self.access_mode == AccessMode::VramWrite {
                    self.dma_fill_pending = Some(DmaFillState {
                        remaining_words: self.dma_length(),
                    });
                }
            }
            // DMA copy: immediate VRAM-to-VRAM byte copy.
            0b11 => {
                if base_code == 0x01 && self.access_mode == AccessMode::VramWrite {
                    self.execute_dma_copy();
                }
            }
            // Other modes (68k->VDP) are not modeled yet.
            _ => {}
        }
    }

    fn execute_dma_fill(&mut self, words: usize, value: u16) {
        let increment = self.auto_increment();
        for _ in 0..words {
            self.write_data_value(value);
            self.access_addr = self.access_addr.wrapping_add(increment);
        }
        self.clear_dma_length();
    }

    fn execute_dma_copy(&mut self) {
        let length = self.dma_length();
        let increment = self.auto_increment();
        let mut src = self.dma_source_addr();

        for _ in 0..length {
            let byte = self.vram[src as usize % VRAM_SIZE];
            self.vram[self.access_addr as usize % VRAM_SIZE] = byte;
            src = src.wrapping_add(1);
            self.access_addr = self.access_addr.wrapping_add(increment);
        }

        self.set_dma_source_addr(src);
        self.clear_dma_length();
    }

    fn seed_demo_scene(&mut self) {
        self.seed_palette();
        self.seed_tile_data();
        self.seed_name_table();
    }

    fn seed_palette(&mut self) {
        for palette in 0..4u16 {
            for color in 0..16u16 {
                let idx = (palette * 16 + color) as usize;
                let ramp = color & 0x7;
                let (r, g, b) = match palette {
                    0 => (ramp, 0, 0),
                    1 => (0, ramp, 0),
                    2 => (0, 0, ramp),
                    _ => (ramp, ramp / 2, ramp),
                };
                self.cram[idx] = encode_md_color(r as u8, g as u8, b as u8);
            }
        }
    }

    fn seed_tile_data(&mut self) {
        const TILE_COUNT: usize = 64;
        // Keep tile 0 transparent so zero-filled name table entries render as background color.
        for tile_index in 1..TILE_COUNT {
            let tile_base = tile_index * TILE_SIZE_BYTES;
            for y in 0..8 {
                for x_pair in 0..4 {
                    let x0 = x_pair * 2;
                    let color_a = ((tile_index + y + x0) % 15 + 1) as u8;
                    let color_b = ((tile_index + y + x0 + 1) % 15 + 1) as u8;
                    let packed = (color_a << 4) | color_b;
                    self.vram[tile_base + y * 4 + x_pair] = packed;
                }
            }
        }
    }

    fn seed_name_table(&mut self) {
        const TILE_COUNT: usize = 64;
        let base = self.nametable_base();
        let (plane_width_tiles, plane_height_tiles) = self.plane_tile_dimensions();
        for tile_y in 0..plane_height_tiles {
            for tile_x in 0..plane_width_tiles {
                let tile_number = ((tile_y * plane_width_tiles + tile_x) % TILE_COUNT) as u16;
                let palette_line = ((tile_y / 7) % 4) as u16;
                let entry = tile_number | (palette_line << 13);
                let name_addr = base + (tile_y * plane_width_tiles + tile_x) * 2;
                self.vram[name_addr % VRAM_SIZE] = (entry >> 8) as u8;
                self.vram[(name_addr + 1) % VRAM_SIZE] = entry as u8;
            }
        }
    }

    fn nametable_base(&self) -> usize {
        ((self.registers[REG_PLANE_A_NAMETABLE] as usize & 0x38) << 10) % VRAM_SIZE
    }

    fn plane_b_nametable_base(&self) -> usize {
        ((self.registers[REG_PLANE_B_NAMETABLE] as usize & 0x07) << 13) % VRAM_SIZE
    }

    fn hscroll_table_base(&self) -> usize {
        ((self.registers[REG_HSCROLL_TABLE] as usize & 0x3F) << 10) % VRAM_SIZE
    }

    fn window_nametable_base(&self) -> usize {
        ((self.registers[REG_WINDOW_NAMETABLE] as usize & 0x3E) << 10) % VRAM_SIZE
    }

    fn sprite_table_base(&self) -> usize {
        ((self.registers[REG_SPRITE_TABLE] as usize & 0x7F) << 9) % VRAM_SIZE
    }

    fn plane_tile_dimensions(&self) -> (usize, usize) {
        let width_code = self.registers[REG_PLANE_SIZE] & 0x03;
        let height_code = (self.registers[REG_PLANE_SIZE] >> 4) & 0x03;
        (
            plane_size_code_to_tiles(width_code),
            plane_size_code_to_tiles(height_code),
        )
    }

    fn plane_scroll(
        &self,
        hscroll_word_index: usize,
        vsram_index: usize,
        plane_width_px: usize,
        plane_height_px: usize,
    ) -> (usize, usize) {
        let hscroll_addr = self.hscroll_table_base() + hscroll_word_index * 2;
        let hscroll = read_u16_be_wrapped(&self.vram, hscroll_addr) as i16;
        let vscroll = self.read_vsram_u16(vsram_index as u8) as i16;
        (
            normalize_scroll(hscroll, plane_width_px),
            normalize_scroll(vscroll, plane_height_px),
        )
    }

    fn vscroll_index_for_x(&self, plane: usize, x: usize) -> usize {
        if (self.registers[11] & 0x04) == 0 {
            return plane;
        }
        ((x / 16) * 2 + plane) % VSRAM_WORDS
    }

    fn plane_vscroll(&self, vsram_index: usize, plane_height_px: usize) -> usize {
        normalize_scroll(
            self.read_vsram_u16(vsram_index as u8) as i16,
            plane_height_px,
        )
    }

    fn hscroll_word_index_for_line(&self, plane: usize, y: usize) -> usize {
        match self.registers[11] & 0x03 {
            // Full-screen scroll (and reserved mode treated as full-screen).
            0x00 | 0x01 => plane,
            // 8-line strips.
            0x02 => (y / 8) * 2 + plane,
            // Per-line scroll.
            0x03 => y * 2 + plane,
            _ => plane,
        }
    }

    fn sample_plane_pixel(
        &self,
        base: usize,
        sample_x: usize,
        sample_y: usize,
        plane_width_tiles: usize,
        plane_height_tiles: usize,
    ) -> Option<PlaneSample> {
        let tile_x = (sample_x / 8) % plane_width_tiles.max(1);
        let tile_y = (sample_y / 8) % plane_height_tiles.max(1);
        let mut in_tile_x = sample_x & 7;
        let mut in_tile_y = sample_y & 7;

        let name_addr = base + (tile_y * plane_width_tiles + tile_x) * 2;
        let entry = read_u16_be_wrapped(&self.vram, name_addr);
        let tile_index = (entry & 0x07FF) as usize;
        let palette_line = ((entry >> 13) & 0x3) as usize;
        let priority_high = (entry & 0x8000) != 0;
        let hflip = (entry & 0x0800) != 0;
        let vflip = (entry & 0x1000) != 0;
        if hflip {
            in_tile_x = 7 - in_tile_x;
        }
        if vflip {
            in_tile_y = 7 - in_tile_y;
        }

        let tile_addr = tile_index * TILE_SIZE_BYTES + in_tile_y * 4 + in_tile_x / 2;
        let tile_byte = self.vram[tile_addr % VRAM_SIZE];
        let pixel = if in_tile_x & 1 == 0 {
            tile_byte >> 4
        } else {
            tile_byte & 0x0F
        };
        if pixel == 0 {
            return None;
        }

        Some(PlaneSample {
            color_index: palette_line * 16 + pixel as usize,
            opaque: true,
            priority_high,
        })
    }

    fn compose_plane_samples(
        &self,
        front: Option<PlaneSample>,
        back: Option<PlaneSample>,
    ) -> Option<PlaneSample> {
        match (front, back) {
            (Some(front), Some(back)) => {
                if front.priority_high != back.priority_high {
                    if front.priority_high {
                        Some(front)
                    } else {
                        Some(back)
                    }
                } else {
                    Some(front)
                }
            }
            (Some(front), None) => Some(front),
            (None, Some(back)) => Some(back),
            (None, None) => None,
        }
    }

    fn window_active_at(&self, x: usize, y: usize) -> bool {
        let hreg = self.registers[REG_WINDOW_HPOS];
        let vreg = self.registers[REG_WINDOW_VPOS];
        let hsplit = (((hreg & 0x1F) as usize) * 16).min(FRAME_WIDTH);
        let vsplit = (((vreg & 0x1F) as usize) * 8).min(FRAME_HEIGHT);
        let hactive = if (hreg & 0x80) != 0 {
            x >= hsplit
        } else {
            x < hsplit
        };
        let vactive = if (vreg & 0x80) != 0 {
            y >= vsplit
        } else {
            y < vsplit
        };
        hactive && vactive
    }

    fn render_frame(&mut self) {
        self.sprite_collision = false;
        self.sprite_overflow = false;
        if !self.display_enabled() {
            self.frame_buffer.fill(0);
            return;
        }

        let plane_a_base = self.nametable_base();
        let plane_b_base = self.plane_b_nametable_base();
        let window_base = self.window_nametable_base();
        let (plane_width_tiles, plane_height_tiles) = self.plane_tile_dimensions();
        let plane_width_px = plane_width_tiles * 8;
        let plane_height_px = plane_height_tiles * 8;
        let mut plane_meta = vec![0u8; FRAME_WIDTH * FRAME_HEIGHT];
        let bg_color_index = self.background_color_index();
        for y in 0..FRAME_HEIGHT {
            let a_hscroll_word = self.hscroll_word_index_for_line(0, y);
            let b_hscroll_word = self.hscroll_word_index_for_line(1, y);
            let a_hscroll = self
                .plane_scroll(a_hscroll_word, 0, plane_width_px, plane_height_px)
                .0;
            let b_hscroll = self
                .plane_scroll(b_hscroll_word, 1, plane_width_px, plane_height_px)
                .0;
            for x in 0..FRAME_WIDTH {
                let a_vscroll = self.plane_vscroll(self.vscroll_index_for_x(0, x), plane_height_px);
                let b_vscroll = self.plane_vscroll(self.vscroll_index_for_x(1, x), plane_height_px);
                let plane_b = self.sample_plane_pixel(
                    plane_b_base,
                    (x + b_hscroll) % plane_width_px,
                    (y + b_vscroll) % plane_height_px,
                    plane_width_tiles,
                    plane_height_tiles,
                );

                let front_plane = if self.window_active_at(x, y) {
                    self.sample_plane_pixel(
                        window_base,
                        x % plane_width_px,
                        y % plane_height_px,
                        plane_width_tiles,
                        plane_height_tiles,
                    )
                } else {
                    self.sample_plane_pixel(
                        plane_a_base,
                        (x + a_hscroll) % plane_width_px,
                        (y + a_vscroll) % plane_height_px,
                        plane_width_tiles,
                        plane_height_tiles,
                    )
                };

                let composed = self.compose_plane_samples(front_plane, plane_b);
                let color_index = composed
                    .map(|sample| sample.color_index)
                    .unwrap_or(bg_color_index);
                let color = self.cram[color_index % CRAM_COLORS];
                let (r, g, b) = md_color_to_rgb888(color);

                let out = (y * FRAME_WIDTH + x) * 3;
                self.frame_buffer[out] = r;
                self.frame_buffer[out + 1] = g;
                self.frame_buffer[out + 2] = b;

                let meta_index = y * FRAME_WIDTH + x;
                if let Some(sample) = composed {
                    plane_meta[meta_index] =
                        (sample.opaque as u8) | ((sample.priority_high as u8) << 1);
                } else {
                    plane_meta[meta_index] = 0;
                }
            }
        }
        self.render_sprites(&plane_meta);
    }

    fn render_sprites(&mut self, plane_meta: &[u8]) {
        const MAX_SPRITES: usize = 80;
        let (max_sprites_per_line, max_pixels_per_line) = if self.h40_mode() {
            (20usize, FRAME_WIDTH)
        } else {
            (16usize, 256usize)
        };
        let mut sprites_on_line = [0u8; FRAME_HEIGHT];
        let mut sprite_pixels_on_line = [0u16; FRAME_HEIGHT];
        let mut masked_line = [false; FRAME_HEIGHT];
        let mut sprite_filled = vec![false; FRAME_WIDTH * FRAME_HEIGHT];
        let mut index = 0usize;

        for _ in 0..MAX_SPRITES {
            let entry_addr = self.sprite_table_base() + index * 8;
            let y_word = read_u16_be_wrapped(&self.vram, entry_addr);
            let size_link = read_u16_be_wrapped(&self.vram, entry_addr + 2);
            let attr = read_u16_be_wrapped(&self.vram, entry_addr + 4);
            let x_word = read_u16_be_wrapped(&self.vram, entry_addr + 6);

            self.draw_sprite(
                y_word,
                size_link,
                attr,
                x_word,
                plane_meta,
                &mut sprite_filled,
                &mut masked_line,
                &mut sprites_on_line,
                &mut sprite_pixels_on_line,
                max_sprites_per_line,
                max_pixels_per_line,
            );

            let link = (size_link & 0x007F) as usize;
            if link == 0 || link == index {
                break;
            }
            index = link.min(MAX_SPRITES - 1);
        }
    }

    fn draw_sprite(
        &mut self,
        y_word: u16,
        size_link: u16,
        attr: u16,
        x_word: u16,
        plane_meta: &[u8],
        sprite_filled: &mut [bool],
        masked_line: &mut [bool; FRAME_HEIGHT],
        sprites_on_line: &mut [u8; FRAME_HEIGHT],
        sprite_pixels_on_line: &mut [u16; FRAME_HEIGHT],
        max_sprites_per_line: usize,
        max_pixels_per_line: usize,
    ) {
        let x = (x_word & 0x03FF) as i32 - 128;
        let y = (y_word & 0x03FF) as i32 - 128;
        let width_tiles = ((size_link >> 10) & 0x3) as usize + 1;
        let height_tiles = ((size_link >> 8) & 0x3) as usize + 1;
        let sprite_priority_high = (attr & 0x8000) != 0;
        let tile_base = (attr & 0x07FF) as usize;
        let palette_line = ((attr >> 13) & 0x3) as usize;
        let hflip = (attr & 0x0800) != 0;
        let vflip = (attr & 0x1000) != 0;
        let width_px = width_tiles * 8;
        let height_px = height_tiles * 8;
        let is_mask_sprite = (x_word & 0x03FF) == 0;

        for sy in 0..height_px {
            let src_y = if vflip { height_px - 1 - sy } else { sy };
            let dy = y + sy as i32;
            if !(0..FRAME_HEIGHT as i32).contains(&dy) {
                continue;
            }
            let dy_index = dy as usize;
            if is_mask_sprite {
                masked_line[dy_index] = true;
                continue;
            }
            if masked_line[dy_index] {
                continue;
            }
            if sprites_on_line[dy_index] as usize >= max_sprites_per_line {
                self.sprite_overflow = true;
                continue;
            }
            sprites_on_line[dy_index] = sprites_on_line[dy_index].saturating_add(1);

            let tile_row = src_y / 8;
            let in_tile_y = src_y & 7;
            for sx in 0..width_px {
                let src_x = if hflip { width_px - 1 - sx } else { sx };
                let dx = x + sx as i32;
                if !(0..FRAME_WIDTH as i32).contains(&dx) {
                    continue;
                }
                if sprite_pixels_on_line[dy_index] as usize >= max_pixels_per_line {
                    self.sprite_overflow = true;
                    break;
                }
                // VDP line sprite budget is consumed by visible sprite dots,
                // including transparent pixels.
                sprite_pixels_on_line[dy_index] = sprite_pixels_on_line[dy_index].saturating_add(1);

                let tile_col = src_x / 8;
                let in_tile_x = src_x & 7;
                let tile_index = tile_base + tile_col * height_tiles + tile_row;
                let tile_addr = tile_index * TILE_SIZE_BYTES + in_tile_y * 4 + in_tile_x / 2;
                let tile_byte = self.vram[tile_addr % VRAM_SIZE];
                let pixel = if in_tile_x & 1 == 0 {
                    tile_byte >> 4
                } else {
                    tile_byte & 0x0F
                };
                if pixel == 0 {
                    continue;
                }

                let meta_index = dy as usize * FRAME_WIDTH + dx as usize;
                let meta = plane_meta[meta_index];
                let plane_opaque = (meta & 0x01) != 0;
                let plane_priority_high = (meta & 0x02) != 0;
                if !sprite_priority_high && plane_opaque && plane_priority_high {
                    continue;
                }

                let color_index = palette_line * 16 + pixel as usize;
                let color = self.cram[color_index % CRAM_COLORS];
                let (r, g, b) = md_color_to_rgb888(color);
                let out = meta_index * 3;
                if sprite_filled[meta_index] {
                    self.sprite_collision = true;
                    continue;
                }
                self.frame_buffer[out] = r;
                self.frame_buffer[out + 1] = g;
                self.frame_buffer[out + 2] = b;
                sprite_filled[meta_index] = true;
            }
        }
    }

    fn display_enabled(&self) -> bool {
        (self.registers[REG_MODE_SET_2] & 0x40) != 0
    }

    fn v_interrupt_enabled(&self) -> bool {
        (self.registers[REG_MODE_SET_2] & 0x20) != 0
    }

    fn h40_mode(&self) -> bool {
        (self.registers[12] & 0x01) != 0
    }

    fn background_color_index(&self) -> usize {
        let bg = self.registers[REG_BACKGROUND_COLOR];
        let palette = ((bg >> 4) & 0x3) as usize;
        let color = (bg & 0x0F) as usize;
        palette * 16 + color
    }
}

fn read_u16_be_wrapped(vram: &[u8; VRAM_SIZE], addr: usize) -> u16 {
    let hi = vram[addr % VRAM_SIZE];
    let lo = vram[(addr + 1) % VRAM_SIZE];
    u16::from_be_bytes([hi, lo])
}

fn encode_md_color(r: u8, g: u8, b: u8) -> u16 {
    let r = (r & 0x7) as u16;
    let g = (g & 0x7) as u16;
    let b = (b & 0x7) as u16;
    (b << 9) | (g << 5) | (r << 1)
}

fn md_color_to_rgb888(color: u16) -> (u8, u8, u8) {
    let r = ((color >> 1) & 0x7) as u8;
    let g = ((color >> 5) & 0x7) as u8;
    let b = ((color >> 9) & 0x7) as u8;
    (r * 36, g * 36, b * 36)
}

fn plane_size_code_to_tiles(code: u8) -> usize {
    match code & 0x3 {
        0x0 => 32,
        0x1 => 64,
        0x3 => 128,
        _ => 32,
    }
}

fn normalize_scroll(value: i16, size: usize) -> usize {
    let size = size as i32;
    let mut wrapped = value as i32 % size;
    if wrapped < 0 {
        wrapped += size;
    }
    wrapped as usize
}

#[cfg(test)]
mod tests {
    use super::{FRAME_HEIGHT, FRAME_WIDTH, Vdp, encode_md_color};

    #[test]
    fn supports_vram_read_write() {
        let mut vdp = Vdp::new();
        vdp.write_vram_u8(0x1234, 0xAB);
        assert_eq!(vdp.read_vram_u8(0x1234), 0xAB);
    }

    #[test]
    fn supports_cram_read_write() {
        let mut vdp = Vdp::new();
        vdp.write_cram_u16(3, encode_md_color(7, 0, 0));
        assert_eq!(vdp.read_cram_u16(3), encode_md_color(7, 0, 0));
    }

    #[test]
    fn supports_vsram_read_write() {
        let mut vdp = Vdp::new();
        vdp.write_vsram_u16(5, 0x1ABC);
        assert_eq!(vdp.read_vsram_u16(5), 0x02BC);
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
    fn respects_auto_increment_register_for_data_port_writes() {
        let mut vdp = Vdp::new();
        // Set register 15 (auto increment) to 4.
        vdp.write_control_port(0x8F04);
        // VRAM write command @ 0x0000.
        vdp.write_control_port(0x4000);
        vdp.write_control_port(0x0000);
        vdp.write_data_port(0xABCD);
        vdp.write_data_port(0x1234);

        assert_eq!(vdp.read_vram_u8(0x0000), 0xAB);
        assert_eq!(vdp.read_vram_u8(0x0001), 0xCD);
        assert_eq!(vdp.read_vram_u8(0x0004), 0x12);
        assert_eq!(vdp.read_vram_u8(0x0005), 0x34);
    }

    #[test]
    fn increments_address_on_data_port_read() {
        let mut vdp = Vdp::new();
        vdp.write_vram_u8(0x0000, 0x11);
        vdp.write_vram_u8(0x0001, 0x22);
        vdp.write_vram_u8(0x0002, 0x33);
        vdp.write_vram_u8(0x0003, 0x44);
        // VRAM read command @ 0x0000.
        vdp.write_control_port(0x0000);
        vdp.write_control_port(0x0000);

        assert_eq!(vdp.read_data_port(), 0x1122);
        assert_eq!(vdp.read_data_port(), 0x3344);
    }

    #[test]
    fn supports_control_and_data_ports_for_cram_write() {
        let mut vdp = Vdp::new();
        vdp.write_control_port(0xC000);
        vdp.write_control_port(0x0000);
        vdp.write_data_port(0x0E0E);
        assert_eq!(vdp.read_cram_u16(0), 0x0E0E);
    }

    #[test]
    fn supports_control_and_data_ports_for_vsram_write_and_read() {
        let mut vdp = Vdp::new();
        // VSRAM write command @ 0x0000.
        vdp.write_control_port(0x4000);
        vdp.write_control_port(0x0010);
        vdp.write_data_port(0x17AB);
        assert_eq!(vdp.read_vsram_u16(0), 0x07AB);

        // VSRAM read command @ 0x0000.
        vdp.write_control_port(0x0000);
        vdp.write_control_port(0x0010);
        assert_eq!(vdp.read_data_port(), 0x07AB);
    }

    #[test]
    fn register_write_updates_name_table_base() {
        let mut vdp = Vdp::new();
        assert_eq!(vdp.nametable_base(), 0xC000);

        // Register 2 = 0x20 -> base 0x8000
        vdp.write_control_port(0x8220);
        assert_eq!(vdp.nametable_base(), 0x8000);
    }

    #[test]
    fn renders_non_uniform_frame() {
        let vdp = Vdp::new();
        assert_eq!(vdp.frame_buffer().len(), FRAME_WIDTH * FRAME_HEIGHT * 3);

        let first = &vdp.frame_buffer()[0..3];
        let mut saw_different = false;
        for chunk in vdp.frame_buffer().chunks_exact(3).take(512) {
            if chunk != first {
                saw_different = true;
                break;
            }
        }
        assert!(saw_different, "frame buffer should not be a flat color");
    }

    #[test]
    fn frame_buffer_updates_after_vram_change() {
        let mut vdp = Vdp::new();
        let before = vdp.frame_buffer()[0..3].to_vec();

        // Top-left pixel uses tile 0, row 0, high nibble.
        vdp.write_cram_u16(2, encode_md_color(7, 7, 7));
        vdp.write_vram_u8(0, 0x20);
        let frame_ready = vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert!(frame_ready);

        let after = vdp.frame_buffer()[0..3].to_vec();
        assert_ne!(before, after);
    }

    #[test]
    fn display_disable_register_blacks_out_frame() {
        let mut vdp = Vdp::new();
        // Register 1 = 0x00 (display disable)
        vdp.write_control_port(0x8100);
        let frame_ready = vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert!(frame_ready);
        assert!(vdp.frame_buffer().iter().all(|&b| b == 0));
    }

    #[test]
    fn control_port_read_reports_and_clears_vblank() {
        let mut vdp = Vdp::new();
        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        let status = vdp.read_control_port();
        assert_ne!(status & super::STATUS_VBLANK, 0);

        let status_after = vdp.read_control_port();
        assert_eq!(status_after & super::STATUS_VBLANK, 0);
    }

    #[test]
    fn vblank_interrupt_becomes_pending_when_enabled() {
        let mut vdp = Vdp::new();
        // Register 1 = 0x60 (display enable + v-interrupt enable)
        vdp.write_control_port(0x8160);
        vdp.step(Vdp::CYCLES_PER_FRAME as u32);

        assert_eq!(vdp.pending_interrupt_level(), Some(6));
        vdp.acknowledge_interrupt(6);
        assert_eq!(vdp.pending_interrupt_level(), None);
    }

    #[test]
    fn hv_counter_changes_as_cycles_advance() {
        let mut vdp = Vdp::new();
        let before = vdp.read_hv_counter();
        vdp.step(1_000);
        let after = vdp.read_hv_counter();
        assert_ne!(before, after);
    }

    #[test]
    fn uses_background_color_register_for_zero_pixels() {
        let mut vdp = Vdp::new();
        vdp.write_cram_u16(0x25, encode_md_color(0, 7, 0));
        // Register 7 = palette 2, color 5
        vdp.write_control_port(0x8725);
        // Force first pixel of tile 0 to color 0 (high nibble of first byte).
        vdp.write_vram_u8(0, 0x00);

        let frame_ready = vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert!(frame_ready);
        let pixel = &vdp.frame_buffer()[0..3];
        assert_eq!(pixel, &[0, 252, 0]);
    }

    #[test]
    fn applies_horizontal_scroll_from_table() {
        let mut vdp = Vdp::new();
        let base = 0xC000usize;

        // Register 13 = 0x3C -> hscroll table @ 0xF000.
        vdp.write_control_port(0x8D3C);
        vdp.write_vram_u8(0xF000, 0x00);
        vdp.write_vram_u8(0xF001, 0x00);

        // Place tile 0 at (0,0), tile 1 at (1,0).
        vdp.write_vram_u8(base as u16, 0x00);
        vdp.write_vram_u8((base + 1) as u16, 0x00);
        vdp.write_vram_u8((base + 2) as u16, 0x00);
        vdp.write_vram_u8((base + 3) as u16, 0x01);

        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));
        for i in 0..4 {
            vdp.write_vram_u8(i, 0x11);
            vdp.write_vram_u8((32 + i) as u16, 0x22);
        }

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[252, 0, 0]);

        // Apply +8 pixel scroll so x=0 samples tile 1.
        vdp.write_vram_u8(0xF000, 0x00);
        vdp.write_vram_u8(0xF001, 0x08);
        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[0, 252, 0]);
    }

    #[test]
    fn applies_per_line_horizontal_scroll_mode() {
        let mut vdp = Vdp::new();
        let base = 0xC000usize;
        vdp.vram.fill(0);
        vdp.cram.fill(0);
        vdp.vsram.fill(0);

        // Register 13 = 0x3C -> hscroll table @ 0xF000.
        vdp.write_control_port(0x8D3C);
        // Register 11 = 0x03 -> per-line hscroll mode.
        vdp.write_control_port(0x8B03);

        // Line 0: no scroll (plane A word at 0xF000).
        vdp.write_vram_u8(0xF000, 0x00);
        vdp.write_vram_u8(0xF001, 0x00);
        // Line 1: +8 scroll (plane A word at 0xF004).
        vdp.write_vram_u8(0xF004, 0x00);
        vdp.write_vram_u8(0xF005, 0x08);

        // Place tile 0 at (0,0), tile 1 at (1,0).
        vdp.write_vram_u8(base as u16, 0x00);
        vdp.write_vram_u8((base + 1) as u16, 0x00);
        vdp.write_vram_u8((base + 2) as u16, 0x00);
        vdp.write_vram_u8((base + 3) as u16, 0x01);

        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));
        for row in 0..8u16 {
            for i in 0..4u16 {
                vdp.write_vram_u8(row * 4 + i, 0x11);
                vdp.write_vram_u8(32 + row * 4 + i, 0x22);
            }
        }

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        // y=0 uses line 0 scroll (tile 0).
        assert_eq!(&vdp.frame_buffer()[0..3], &[252, 0, 0]);
        // y=1 uses line 1 scroll (tile 1).
        let y1 = FRAME_WIDTH * 3;
        assert_eq!(&vdp.frame_buffer()[y1..y1 + 3], &[0, 252, 0]);
    }

    #[test]
    fn applies_vertical_scroll_from_vsram() {
        let mut vdp = Vdp::new();
        let base = 0xC000usize;
        let default_plane_width_tiles = 32usize;

        // Register 13 = 0x3C -> hscroll table @ 0xF000, keep hscroll = 0.
        vdp.write_control_port(0x8D3C);
        vdp.write_vram_u8(0xF000, 0x00);
        vdp.write_vram_u8(0xF001, 0x00);

        // Place tile 0 at (0,0), tile 2 at (0,1).
        vdp.write_vram_u8(base as u16, 0x00);
        vdp.write_vram_u8((base + 1) as u16, 0x00);
        vdp.write_vram_u8((base + default_plane_width_tiles * 2) as u16, 0x00);
        vdp.write_vram_u8((base + default_plane_width_tiles * 2 + 1) as u16, 0x02);

        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));
        for i in 0..4 {
            vdp.write_vram_u8(i, 0x11);
            vdp.write_vram_u8((64 + i) as u16, 0x22);
        }

        vdp.write_vsram_u16(0, 0);
        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[252, 0, 0]);

        // Scroll down by one tile row so y=0 samples tile row 1.
        vdp.write_vsram_u16(0, 8);
        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[0, 252, 0]);
    }

    #[test]
    fn applies_two_cell_column_vertical_scroll_mode() {
        let mut vdp = Vdp::new();
        vdp.vram.fill(0);
        vdp.cram.fill(0);
        vdp.vsram.fill(0);
        let base = 0xC000usize;
        let default_plane_width_tiles = 32usize;

        // Reg11 bit2 enables 2-cell column vertical scroll mode.
        vdp.write_control_port(0x8B04);

        // Name table:
        // Columns 0-1 row0 use tile 1 (red).
        vdp.write_vram_u8(base as u16, 0x00);
        vdp.write_vram_u8((base + 1) as u16, 0x01);
        vdp.write_vram_u8((base + 2) as u16, 0x00);
        vdp.write_vram_u8((base + 3) as u16, 0x01);
        // Columns 2-3 row0 use tile 1 (red) by default.
        vdp.write_vram_u8((base + 4) as u16, 0x00);
        vdp.write_vram_u8((base + 5) as u16, 0x01);
        vdp.write_vram_u8((base + 6) as u16, 0x00);
        vdp.write_vram_u8((base + 7) as u16, 0x01);
        // Columns 2-3 row1 use tile 2 (green).
        let row1 = base + default_plane_width_tiles * 2;
        vdp.write_vram_u8((row1 + 4) as u16, 0x00);
        vdp.write_vram_u8((row1 + 5) as u16, 0x02);
        vdp.write_vram_u8((row1 + 6) as u16, 0x00);
        vdp.write_vram_u8((row1 + 7) as u16, 0x02);

        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));
        for i in 0..32u16 {
            vdp.write_vram_u8(32 + i, 0x11);
            vdp.write_vram_u8(64 + i, 0x22);
        }

        // Plane A VSRAM entries are even indices:
        // col group 0 => index 0 (no scroll)
        // col group 1 => index 2 (+8px)
        vdp.write_vsram_u16(0, 0);
        vdp.write_vsram_u16(2, 8);

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[252, 0, 0]);
        assert_eq!(&vdp.frame_buffer()[16 * 3..16 * 3 + 3], &[0, 252, 0]);
    }

    #[test]
    fn applies_plane_tile_flip_bits() {
        let mut vdp = Vdp::new();
        vdp.vram.fill(0);
        vdp.cram.fill(0);
        vdp.vsram.fill(0);

        // Plane A entry (0,0): tile 1, hflip+vflip.
        let entry = 0x1801u16;
        vdp.write_vram_u8(0xC000, (entry >> 8) as u8);
        vdp.write_vram_u8(0xC001, entry as u8);

        // Tile 1, source pixel at (7,7) uses color index 2.
        let tile_base = 32usize;
        vdp.write_vram_u8((tile_base + 7 * 4 + 3) as u16, 0x02);
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[0, 252, 0]);
    }

    #[test]
    fn renders_sprite_pixels_over_plane() {
        let mut vdp = Vdp::new();
        // Register 5 = 0x70 -> sprite table @ 0xE000.
        vdp.write_control_port(0x8570);
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));

        // Tile 3, first pixel = color index 2.
        vdp.write_vram_u8(3 * 32, 0x20);

        let sat = 0xE000u16;
        // Y position = 128 (screen y = 0)
        vdp.write_vram_u8(sat, 0x00);
        vdp.write_vram_u8(sat + 1, 0x80);
        // Size/link: 1x1 tile, end of list.
        vdp.write_vram_u8(sat + 2, 0x00);
        vdp.write_vram_u8(sat + 3, 0x00);
        // Attr: tile index = 3.
        vdp.write_vram_u8(sat + 4, 0x00);
        vdp.write_vram_u8(sat + 5, 0x03);
        // X position = 128 (screen x = 0)
        vdp.write_vram_u8(sat + 6, 0x00);
        vdp.write_vram_u8(sat + 7, 0x80);

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[0, 252, 0]);
    }

    #[test]
    fn renders_window_plane_over_plane_a() {
        let mut vdp = Vdp::new();
        let plane_a_base = 0xC000u16;
        let window_base = 0xD000u16;

        // Keep hscroll = 0 to make plane A baseline deterministic.
        vdp.write_control_port(0x8D3C);
        vdp.write_vram_u8(0xF000, 0x00);
        vdp.write_vram_u8(0xF001, 0x00);

        // Plane A base (reg2) is default 0x30; entry (0,0) uses tile 0.
        vdp.write_vram_u8(plane_a_base, 0x00);
        vdp.write_vram_u8(plane_a_base + 1, 0x00);

        // Window base (reg3) = 0x34 -> 0xD000, entry (0,0) uses tile 2.
        vdp.write_control_port(0x8334);
        vdp.write_vram_u8(window_base, 0x00);
        vdp.write_vram_u8(window_base + 1, 0x02);

        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));
        for i in 0..4 {
            vdp.write_vram_u8(i, 0x11);
            vdp.write_vram_u8(64 + i as u16, 0x22);
        }

        // Enable window over the full screen.
        vdp.write_control_port(0x9180);
        vdp.write_control_port(0x9280);

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[0, 252, 0]);
    }

    #[test]
    fn window_horizontal_split_selects_region() {
        let mut vdp = Vdp::new();
        let plane_a_base = 0xC000u16;
        let window_base = 0xD000u16;

        vdp.write_control_port(0x8D3C);
        vdp.write_vram_u8(0xF000, 0x00);
        vdp.write_vram_u8(0xF001, 0x00);

        // Plane A entries (0,0) and (1,0) use tile 0.
        vdp.write_vram_u8(plane_a_base, 0x00);
        vdp.write_vram_u8(plane_a_base + 1, 0x00);
        vdp.write_vram_u8(plane_a_base + 2, 0x00);
        vdp.write_vram_u8(plane_a_base + 3, 0x00);

        // Window entries (0..3,0) use tile 2.
        vdp.write_control_port(0x8334);
        vdp.write_vram_u8(window_base, 0x00);
        vdp.write_vram_u8(window_base + 1, 0x02);
        vdp.write_vram_u8(window_base + 2, 0x00);
        vdp.write_vram_u8(window_base + 3, 0x02);
        vdp.write_vram_u8(window_base + 4, 0x00);
        vdp.write_vram_u8(window_base + 5, 0x02);
        vdp.write_vram_u8(window_base + 6, 0x00);
        vdp.write_vram_u8(window_base + 7, 0x02);

        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));
        for i in 0..4 {
            vdp.write_vram_u8(i, 0x11);
            vdp.write_vram_u8(64 + i as u16, 0x22);
        }

        // x<16: Plane A, x>=16: Window.
        vdp.write_control_port(0x9181);
        vdp.write_control_port(0x9280);

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[252, 0, 0]);
        assert_eq!(&vdp.frame_buffer()[16 * 3..16 * 3 + 3], &[0, 252, 0]);
    }

    #[test]
    fn low_priority_sprite_is_behind_high_priority_plane() {
        let mut vdp = Vdp::new();
        let plane_a_base = 0xC000u16;
        let sat = 0xE000u16;

        // Plane pixel: tile 0 with high priority.
        vdp.write_vram_u8(plane_a_base, 0x80);
        vdp.write_vram_u8(plane_a_base + 1, 0x00);
        vdp.write_vram_u8(0, 0x11);
        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));

        // Sprite pixel at same position: tile 3, low priority.
        vdp.write_vram_u8(3 * 32, 0x20);
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));
        vdp.write_vram_u8(sat, 0x00);
        vdp.write_vram_u8(sat + 1, 0x80);
        vdp.write_vram_u8(sat + 2, 0x00);
        vdp.write_vram_u8(sat + 3, 0x00);
        vdp.write_vram_u8(sat + 4, 0x00);
        vdp.write_vram_u8(sat + 5, 0x03);
        vdp.write_vram_u8(sat + 6, 0x00);
        vdp.write_vram_u8(sat + 7, 0x80);

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[252, 0, 0]);
    }

    #[test]
    fn limits_sprites_per_line_in_h40_mode() {
        let mut vdp = Vdp::new();
        vdp.vram.fill(0);
        vdp.cram.fill(0);
        vdp.vsram.fill(0);

        // H40 mode and SAT at 0xE000.
        vdp.write_control_port(0x8C81);
        vdp.write_control_port(0x8570);
        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        // Tile 1: fully opaque color index 1.
        for i in 0..32u16 {
            vdp.write_vram_u8(32 + i, 0x11);
        }

        let sat = 0xE000usize;
        for i in 0..21usize {
            let entry = sat + i * 8;
            let x_pos = 128 + (i as u16) * 8;
            let link = if i == 20 { 0 } else { (i + 1) as u16 };

            // Y = 128 (screen y=0)
            vdp.write_vram_u8(entry as u16, 0x00);
            vdp.write_vram_u8((entry + 1) as u16, 0x80);
            // 1x1 sprite + link
            vdp.write_vram_u8((entry + 2) as u16, 0x00);
            vdp.write_vram_u8((entry + 3) as u16, (link & 0x7F) as u8);
            // Tile index 1
            vdp.write_vram_u8((entry + 4) as u16, 0x00);
            vdp.write_vram_u8((entry + 5) as u16, 0x01);
            // X position
            vdp.write_vram_u8((entry + 6) as u16, (x_pos >> 8) as u8);
            vdp.write_vram_u8((entry + 7) as u16, x_pos as u8);
        }

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);

        // First 20 sprites are visible.
        for i in 0..20usize {
            let x = i * 8;
            let p = x * 3;
            assert_eq!(&vdp.frame_buffer()[p..p + 3], &[252, 0, 0]);
        }
        // 21st sprite is dropped by per-line limit.
        let p = 20 * 8 * 3;
        assert_eq!(&vdp.frame_buffer()[p..p + 3], &[0, 0, 0]);
        let status = vdp.read_control_port();
        assert_ne!(status & super::STATUS_SPRITE_OVERFLOW, 0);
    }

    #[test]
    fn sprites_use_column_major_tile_layout() {
        let mut vdp = Vdp::new();
        vdp.vram.fill(0);
        vdp.cram.fill(0);
        vdp.vsram.fill(0);

        // SAT at 0xE000.
        vdp.write_control_port(0x8570);
        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));
        vdp.write_cram_u16(3, encode_md_color(0, 0, 7));
        vdp.write_cram_u16(4, encode_md_color(7, 7, 0));

        // Tiles 1..4 as solid colors 1..4.
        for i in 0..32u16 {
            vdp.write_vram_u8(32 + i, 0x11);
            vdp.write_vram_u8(64 + i, 0x22);
            vdp.write_vram_u8(96 + i, 0x33);
            vdp.write_vram_u8(128 + i, 0x44);
        }

        let sat = 0xE000u16;
        // Y = 128 (screen y = 0)
        vdp.write_vram_u8(sat, 0x00);
        vdp.write_vram_u8(sat + 1, 0x80);
        // Size: 2x2 tiles, link end.
        vdp.write_vram_u8(sat + 2, 0x05);
        vdp.write_vram_u8(sat + 3, 0x00);
        // Attr: tile index 1.
        vdp.write_vram_u8(sat + 4, 0x00);
        vdp.write_vram_u8(sat + 5, 0x01);
        // X = 128 (screen x = 0)
        vdp.write_vram_u8(sat + 6, 0x00);
        vdp.write_vram_u8(sat + 7, 0x80);

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);

        let top_left = &vdp.frame_buffer()[0..3];
        let top_right = &vdp.frame_buffer()[8 * 3..8 * 3 + 3];
        let bottom_left = &vdp.frame_buffer()[FRAME_WIDTH * 8 * 3..FRAME_WIDTH * 8 * 3 + 3];
        let bottom_right =
            &vdp.frame_buffer()[FRAME_WIDTH * 8 * 3 + 8 * 3..FRAME_WIDTH * 8 * 3 + 8 * 3 + 3];

        assert_eq!(top_left, &[252, 0, 0]);
        assert_eq!(top_right, &[0, 0, 252]);
        assert_eq!(bottom_left, &[0, 252, 0]);
        assert_eq!(bottom_right, &[252, 252, 0]);
    }

    #[test]
    fn lower_index_sprite_has_priority_when_overlapping() {
        let mut vdp = Vdp::new();
        vdp.vram.fill(0);
        vdp.cram.fill(0);
        vdp.vsram.fill(0);

        vdp.write_control_port(0x8570);
        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));

        // Tile 1 = red, tile 2 = green
        for i in 0..32u16 {
            vdp.write_vram_u8(32 + i, 0x11);
            vdp.write_vram_u8(64 + i, 0x22);
        }

        let sat = 0xE000usize;
        // Sprite 0: tile 1 at (0,0), link -> sprite 1
        vdp.write_vram_u8(sat as u16, 0x00);
        vdp.write_vram_u8((sat + 1) as u16, 0x80);
        vdp.write_vram_u8((sat + 2) as u16, 0x00);
        vdp.write_vram_u8((sat + 3) as u16, 0x01);
        vdp.write_vram_u8((sat + 4) as u16, 0x00);
        vdp.write_vram_u8((sat + 5) as u16, 0x01);
        vdp.write_vram_u8((sat + 6) as u16, 0x00);
        vdp.write_vram_u8((sat + 7) as u16, 0x80);

        // Sprite 1: tile 2 at same (0,0), end
        let sat1 = sat + 8;
        vdp.write_vram_u8(sat1 as u16, 0x00);
        vdp.write_vram_u8((sat1 + 1) as u16, 0x80);
        vdp.write_vram_u8((sat1 + 2) as u16, 0x00);
        vdp.write_vram_u8((sat1 + 3) as u16, 0x00);
        vdp.write_vram_u8((sat1 + 4) as u16, 0x00);
        vdp.write_vram_u8((sat1 + 5) as u16, 0x02);
        vdp.write_vram_u8((sat1 + 6) as u16, 0x00);
        vdp.write_vram_u8((sat1 + 7) as u16, 0x80);

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[252, 0, 0]);
        let status = vdp.read_control_port();
        assert_ne!(status & super::STATUS_SPRITE_COLLISION, 0);
        let status_after = vdp.read_control_port();
        assert_eq!(status_after & super::STATUS_SPRITE_COLLISION, 0);
    }

    #[test]
    fn x_zero_sprite_masks_following_sprites_on_same_line() {
        let mut vdp = Vdp::new();
        vdp.vram.fill(0);
        vdp.cram.fill(0);
        vdp.vsram.fill(0);

        vdp.write_control_port(0x8570);
        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        for i in 0..32u16 {
            vdp.write_vram_u8(32 + i, 0x11);
        }

        let sat = 0xE000usize;
        // Sprite 0: mask sprite (X=0 internal), covers y=0 line.
        vdp.write_vram_u8(sat as u16, 0x00);
        vdp.write_vram_u8((sat + 1) as u16, 0x80);
        vdp.write_vram_u8((sat + 2) as u16, 0x00);
        vdp.write_vram_u8((sat + 3) as u16, 0x01);
        vdp.write_vram_u8((sat + 4) as u16, 0x00);
        vdp.write_vram_u8((sat + 5) as u16, 0x00);
        vdp.write_vram_u8((sat + 6) as u16, 0x00);
        vdp.write_vram_u8((sat + 7) as u16, 0x00);

        // Sprite 1: red sprite at (0,0), should be masked.
        let sat1 = sat + 8;
        vdp.write_vram_u8(sat1 as u16, 0x00);
        vdp.write_vram_u8((sat1 + 1) as u16, 0x80);
        vdp.write_vram_u8((sat1 + 2) as u16, 0x00);
        vdp.write_vram_u8((sat1 + 3) as u16, 0x00);
        vdp.write_vram_u8((sat1 + 4) as u16, 0x00);
        vdp.write_vram_u8((sat1 + 5) as u16, 0x01);
        vdp.write_vram_u8((sat1 + 6) as u16, 0x00);
        vdp.write_vram_u8((sat1 + 7) as u16, 0x80);

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[0, 0, 0]);
    }

    #[test]
    fn transparent_sprite_dots_consume_line_dot_budget() {
        let mut vdp = Vdp::new();
        vdp.vram.fill(0);
        vdp.cram.fill(0);
        vdp.vsram.fill(0);

        // H40 mode (320-dot sprite line budget), SAT at 0xE000.
        vdp.write_control_port(0x8C81);
        vdp.write_control_port(0x8570);
        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        // Tile 1: opaque red.
        for i in 0..32u16 {
            vdp.write_vram_u8(32 + i, 0x11);
        }

        let sat = 0xE000usize;
        // 10 transparent 32px sprites (4x1 tiles) at y=0 consume 320 dots.
        for i in 0..10usize {
            let entry = sat + i * 8;
            let link = (i + 1) as u16;
            vdp.write_vram_u8(entry as u16, 0x00);
            vdp.write_vram_u8((entry + 1) as u16, 0x80);
            vdp.write_vram_u8((entry + 2) as u16, 0x0C); // 4x1
            vdp.write_vram_u8((entry + 3) as u16, (link & 0x7F) as u8);
            vdp.write_vram_u8((entry + 4) as u16, 0x00);
            vdp.write_vram_u8((entry + 5) as u16, 0x00); // tile 0 transparent
            vdp.write_vram_u8((entry + 6) as u16, 0x00);
            vdp.write_vram_u8((entry + 7) as u16, 0x80); // x=0
        }

        // 11th sprite is opaque red at same line, should be dropped by dot budget.
        let entry = sat + 10 * 8;
        vdp.write_vram_u8(entry as u16, 0x00);
        vdp.write_vram_u8((entry + 1) as u16, 0x80);
        vdp.write_vram_u8((entry + 2) as u16, 0x00); // 1x1
        vdp.write_vram_u8((entry + 3) as u16, 0x00); // end
        vdp.write_vram_u8((entry + 4) as u16, 0x00);
        vdp.write_vram_u8((entry + 5) as u16, 0x01); // tile 1 opaque
        vdp.write_vram_u8((entry + 6) as u16, 0x00);
        vdp.write_vram_u8((entry + 7) as u16, 0x80); // x=0

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[0, 0, 0]);
    }

    #[test]
    fn high_priority_sprite_overrides_high_priority_plane() {
        let mut vdp = Vdp::new();
        let plane_a_base = 0xC000u16;
        let sat = 0xE000u16;

        // Plane pixel: tile 0 with high priority.
        vdp.write_vram_u8(plane_a_base, 0x80);
        vdp.write_vram_u8(plane_a_base + 1, 0x00);
        vdp.write_vram_u8(0, 0x11);
        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));

        // Sprite pixel at same position: tile 3, high priority.
        vdp.write_vram_u8(3 * 32, 0x20);
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));
        vdp.write_vram_u8(sat, 0x00);
        vdp.write_vram_u8(sat + 1, 0x80);
        vdp.write_vram_u8(sat + 2, 0x00);
        vdp.write_vram_u8(sat + 3, 0x00);
        vdp.write_vram_u8(sat + 4, 0x80);
        vdp.write_vram_u8(sat + 5, 0x03);
        vdp.write_vram_u8(sat + 6, 0x00);
        vdp.write_vram_u8(sat + 7, 0x80);

        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[0, 252, 0]);
    }

    #[test]
    fn defaults_plane_size_to_32x32_cells() {
        let mut vdp = Vdp::new();
        vdp.vram.fill(0);
        vdp.cram.fill(0);
        vdp.vsram.fill(0);

        // Plane size register defaults to 0x00 => 32x32.
        // Place tile 1 at (0,0) and tile 2 at (0,32) to verify vertical wrap.
        let plane_a = 0xC000usize;
        vdp.write_vram_u8(plane_a as u16, 0x00);
        vdp.write_vram_u8((plane_a + 1) as u16, 0x01);
        let row32 = plane_a + 32 * 32 * 2;
        vdp.write_vram_u8(row32 as u16, 0x00);
        vdp.write_vram_u8((row32 + 1) as u16, 0x02);

        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));
        for i in 0..4u16 {
            vdp.write_vram_u8(32 + i, 0x11);
            vdp.write_vram_u8(64 + i, 0x22);
        }

        // Scroll down by 32 tiles (256px). With 32-cell height this wraps to row 0.
        vdp.write_vsram_u16(0, 256);
        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[252, 0, 0]);
    }

    #[test]
    fn supports_64x64_plane_size_from_reg16() {
        let mut vdp = Vdp::new();
        vdp.vram.fill(0);
        vdp.cram.fill(0);
        vdp.vsram.fill(0);

        // reg16 = 0x11 => 64x64 cells.
        vdp.write_control_port(0x9011);

        let plane_a = 0xC000usize;
        // Tile at (0,0)
        vdp.write_vram_u8(plane_a as u16, 0x00);
        vdp.write_vram_u8((plane_a + 1) as u16, 0x01);
        // Tile at (0,32) within the 64-cell-tall map.
        let row32 = plane_a + 32 * 64 * 2;
        vdp.write_vram_u8(row32 as u16, 0x00);
        vdp.write_vram_u8((row32 + 1) as u16, 0x02);

        vdp.write_cram_u16(1, encode_md_color(7, 0, 0));
        vdp.write_cram_u16(2, encode_md_color(0, 7, 0));
        for i in 0..4u16 {
            vdp.write_vram_u8(32 + i, 0x11);
            vdp.write_vram_u8(64 + i, 0x22);
        }

        // Scroll 32 tiles down. On 64-cell height this should land on row 32 (green), not wrap.
        vdp.write_vsram_u16(0, 256);
        vdp.step(Vdp::CYCLES_PER_FRAME as u32);
        assert_eq!(&vdp.frame_buffer()[0..3], &[0, 252, 0]);
    }

    #[test]
    fn dma_fill_writes_repeated_words_to_vram() {
        let mut vdp = Vdp::new();
        // Register 1: display + DMA enable.
        vdp.write_control_port(0x8150);
        // Auto-increment = 2 bytes.
        vdp.write_control_port(0x8F02);
        // DMA length = 3 words.
        vdp.write_control_port(0x9303);
        vdp.write_control_port(0x9400);
        // DMA mode = fill.
        vdp.write_control_port(0x9780);

        // VRAM write DMA command @ 0x0000 (code with DMA bit set).
        vdp.write_control_port(0x4000);
        vdp.write_control_port(0x0080);
        // Fill value provided via data port.
        vdp.write_data_port(0xA1B2);

        assert_eq!(vdp.read_vram_u8(0x0000), 0xA1);
        assert_eq!(vdp.read_vram_u8(0x0001), 0xB2);
        assert_eq!(vdp.read_vram_u8(0x0002), 0xA1);
        assert_eq!(vdp.read_vram_u8(0x0003), 0xB2);
        assert_eq!(vdp.read_vram_u8(0x0004), 0xA1);
        assert_eq!(vdp.read_vram_u8(0x0005), 0xB2);
    }

    #[test]
    fn dma_copy_copies_vram_bytes() {
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

        vdp.write_vram_u8(0x0100, 0x11);
        vdp.write_vram_u8(0x0101, 0x22);
        vdp.write_vram_u8(0x0102, 0x33);
        vdp.write_vram_u8(0x0103, 0x44);

        // VRAM write DMA command @ 0x0200 (code with DMA bit set).
        vdp.write_control_port(0x4200);
        vdp.write_control_port(0x0080);

        assert_eq!(vdp.read_vram_u8(0x0200), 0x11);
        assert_eq!(vdp.read_vram_u8(0x0201), 0x22);
        assert_eq!(vdp.read_vram_u8(0x0202), 0x33);
        assert_eq!(vdp.read_vram_u8(0x0203), 0x44);
    }
}
