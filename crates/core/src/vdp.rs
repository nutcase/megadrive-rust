pub const FRAME_WIDTH: usize = 320;
pub const FRAME_HEIGHT: usize = 240;
const FRAME_WIDTH_32_CELL: usize = 256;
const FRAME_HEIGHT_28_CELL: usize = 224;

const VRAM_SIZE: usize = 0x10000;
const CRAM_COLORS: usize = 64;
const VSRAM_WORDS: usize = 40;
const TILE_SIZE_BYTES: usize = 32;
const REG_COUNT: usize = 0x20;
const REG_MODE_SET_1: usize = 0;
const REG_MODE_SET_2: usize = 1;
const REG_PLANE_A_NAMETABLE: usize = 2;
const REG_WINDOW_NAMETABLE: usize = 3;
const REG_PLANE_B_NAMETABLE: usize = 4;
const REG_SPRITE_TABLE: usize = 5;
const REG_BACKGROUND_COLOR: usize = 7;
const REG_H_INTERRUPT_COUNTER: usize = 10;
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
const STATUS_HBLANK: u16 = 0x0004;
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
    pub dest_addr: u16,
    pub auto_increment: u16,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoStandard {
    Ntsc,
    Pal,
}

impl VideoStandard {
    fn total_lines(self) -> u64 {
        match self {
            Self::Ntsc => Vdp::NTSC_TOTAL_LINES,
            Self::Pal => Vdp::PAL_TOTAL_LINES,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Vdp {
    video_standard: VideoStandard,
    frame_cycles: u64,
    frame_count: u64,
    sprite_collision: bool,
    sprite_overflow: bool,
    h_interrupt_pending: bool,
    v_interrupt_pending: bool,
    h_interrupt_counter: u8,
    vram: [u8; VRAM_SIZE],
    cram: [u16; CRAM_COLORS],
    vsram: [u16; VSRAM_WORDS],
    frame_buffer: Vec<u8>,
    registers: [u8; REG_COUNT],
    line_registers: [[u8; REG_COUNT]; FRAME_HEIGHT],
    line_vsram: [[u16; VSRAM_WORDS]; FRAME_HEIGHT],
    line_hscroll: [[u16; 2]; FRAME_HEIGHT],
    line_cram: [[u16; CRAM_COLORS]; FRAME_HEIGHT],
    line_vram: Vec<[u8; VRAM_SIZE]>,
    control_latch: Option<u16>,
    access_addr: u16,
    access_mode: AccessMode,
    dma_fill_pending: Option<DmaFillState>,
    dma_bus_pending: Option<BusDmaRequest>,
    dma_fill_ops: u64,
    dma_copy_ops: u64,
    quirk_bottom_bg_mask: bool,
    quirk_live_plane_vram: bool,
    quirk_live_hscroll: bool,
    quirk_plane_a_64x32_paged: bool,
}

impl Default for Vdp {
    fn default() -> Self {
        Self::with_video_standard(VideoStandard::Ntsc)
    }
}

impl Vdp {
    // Keep legacy NTSC timing to avoid regressions in existing emulation paths.
    const NTSC_CYCLES_PER_FRAME: u64 = 127_800;
    const NTSC_TOTAL_LINES: u64 = 262;
    const PAL_TOTAL_LINES: u64 = 313;
    const TOTAL_DOTS_PER_LINE: u64 = 342;
    #[cfg(test)]
    const CYCLES_PER_FRAME: u64 = Self::NTSC_CYCLES_PER_FRAME;
    #[cfg(test)]
    const TOTAL_LINES: u64 = Self::NTSC_TOTAL_LINES;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_video_standard(video_standard: VideoStandard) -> Self {
        let mut registers = [0u8; REG_COUNT];
        registers[REG_MODE_SET_2] = 0x40; // Display enabled
        registers[REG_PLANE_A_NAMETABLE] = 0x30; // Plane A name table base: 0xC000
        registers[REG_SPRITE_TABLE] = 0x70; // Sprite attribute table base: 0xE000
        registers[REG_HSCROLL_TABLE] = 0x3C; // Horizontal scroll table base: 0xF000
        // Window off by default.
        // Keep window disabled by default.
        registers[REG_WINDOW_HPOS] = 0x00;
        registers[REG_WINDOW_VPOS] = 0x00;
        registers[REG_AUTO_INCREMENT] = 2; // Word access by default

        let mut vdp = Self {
            video_standard,
            frame_cycles: 0,
            frame_count: 0,
            sprite_collision: false,
            sprite_overflow: false,
            h_interrupt_pending: false,
            v_interrupt_pending: false,
            h_interrupt_counter: registers[REG_H_INTERRUPT_COUNTER],
            vram: [0; VRAM_SIZE],
            cram: [0; CRAM_COLORS],
            vsram: [0; VSRAM_WORDS],
            frame_buffer: vec![0; FRAME_WIDTH * FRAME_HEIGHT * 3],
            registers,
            line_registers: [[0; REG_COUNT]; FRAME_HEIGHT],
            line_vsram: [[0; VSRAM_WORDS]; FRAME_HEIGHT],
            line_hscroll: [[0; 2]; FRAME_HEIGHT],
            line_cram: [[0; CRAM_COLORS]; FRAME_HEIGHT],
            line_vram: vec![[0; VRAM_SIZE]; FRAME_HEIGHT],
            control_latch: None,
            access_addr: 0,
            access_mode: AccessMode::default(),
            dma_fill_pending: None,
            dma_bus_pending: None,
            dma_fill_ops: 0,
            dma_copy_ops: 0,
            quirk_bottom_bg_mask: false,
            quirk_live_plane_vram: false,
            quirk_live_hscroll: false,
            quirk_plane_a_64x32_paged: false,
        };
        vdp.reset_line_state();
        vdp.capture_line_state(0);
        vdp.render_frame();
        vdp
    }

    pub fn video_standard(&self) -> VideoStandard {
        self.video_standard
    }

    pub fn total_lines(&self) -> u64 {
        self.video_standard.total_lines()
    }

    pub fn set_quirk_bottom_bg_mask(&mut self, enabled: bool) {
        self.quirk_bottom_bg_mask = enabled;
    }

    pub fn set_quirk_live_plane_vram(&mut self, enabled: bool) {
        self.quirk_live_plane_vram = enabled;
    }

    pub fn set_quirk_live_hscroll(&mut self, enabled: bool) {
        self.quirk_live_hscroll = enabled;
    }

    pub fn set_quirk_plane_a_64x32_paged(&mut self, enabled: bool) {
        self.quirk_plane_a_64x32_paged = enabled;
    }

    fn cycles_per_frame(&self) -> u64 {
        match self.video_standard {
            VideoStandard::Ntsc => Self::NTSC_CYCLES_PER_FRAME,
            VideoStandard::Pal => {
                // Preserve per-line cadence relative to NTSC model.
                (Self::NTSC_CYCLES_PER_FRAME * Self::PAL_TOTAL_LINES + (Self::NTSC_TOTAL_LINES / 2))
                    / Self::NTSC_TOTAL_LINES
            }
        }
    }

    pub fn step(&mut self, cpu_cycles: u32) -> bool {
        let mut remaining = cpu_cycles as u64;
        let mut frame_ready = false;
        let cycles_per_frame = self.cycles_per_frame();

        while remaining > 0 {
            let until_frame_end = cycles_per_frame - self.frame_cycles;
            let advance = remaining.min(until_frame_end);
            let start = self.frame_cycles;
            let end = self.frame_cycles + advance;
            self.process_line_crossings(start, end);
            self.frame_cycles = end;
            remaining -= advance;

            if self.frame_cycles >= cycles_per_frame {
                self.frame_cycles = 0;
                self.frame_count += 1;
                self.render_frame();
                self.h_interrupt_counter = self.registers[REG_H_INTERRUPT_COUNTER];
                self.reset_line_state();
                self.capture_line_state(0);
                self.on_scanline_start(0);
                frame_ready = true;
            }
        }

        frame_ready
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

    pub fn line_register(&self, line: usize, index: usize) -> u8 {
        if line < FRAME_HEIGHT && index < REG_COUNT {
            self.line_registers[line][index]
        } else {
            0
        }
    }

    pub fn line_vsram_u16(&self, line: usize, index: usize) -> u16 {
        if line < FRAME_HEIGHT && index < VSRAM_WORDS {
            self.line_vsram[line][index]
        } else {
            0
        }
    }

    pub fn line_hscroll_words(&self, line: usize) -> [u16; 2] {
        if line < FRAME_HEIGHT {
            self.line_hscroll[line]
        } else {
            [0; 2]
        }
    }

    pub fn line_vram_u8(&self, line: usize, addr: u16) -> u8 {
        if line < FRAME_HEIGHT {
            self.line_vram[line][addr as usize % VRAM_SIZE]
        } else {
            0
        }
    }

    pub fn pending_interrupt_level(&self) -> Option<u8> {
        if self.v_interrupt_pending {
            Some(6)
        } else if self.h_interrupt_pending {
            Some(4)
        } else {
            None
        }
    }

    pub fn dma_fill_ops(&self) -> u64 {
        self.dma_fill_ops
    }

    pub fn dma_copy_ops(&self) -> u64 {
        self.dma_copy_ops
    }

    pub fn acknowledge_interrupt(&mut self, level: u8) {
        if level == 6 {
            self.v_interrupt_pending = false;
        } else if level == 4 {
            self.h_interrupt_pending = false;
        }
    }

    pub fn read_control_port(&mut self) -> u16 {
        // Reading status clears command latch.
        self.control_latch = None;
        let mut status = STATUS_BASE;
        if self.hblank_active() {
            status |= STATUS_HBLANK;
        }
        if self.vblank_active() {
            status |= STATUS_VBLANK;
        }
        if self.sprite_collision {
            status |= STATUS_SPRITE_COLLISION;
        }
        if self.sprite_overflow {
            status |= STATUS_SPRITE_OVERFLOW;
        }
        self.sprite_collision = false;
        self.sprite_overflow = false;
        status
    }

    fn hblank_active(&self) -> bool {
        let cycles_per_line = self.cycles_per_line().max(1);
        let cycle_in_line = self.frame_cycles % cycles_per_line;
        cycle_in_line >= self.hblank_start_cycle(cycles_per_line)
    }

    fn current_line_index(&self) -> usize {
        self.line_index_for_cycle(self.frame_cycles)
            .min(self.total_lines().saturating_sub(1)) as usize
    }

    fn vblank_active(&self) -> bool {
        self.current_line_index() >= self.active_display_height()
    }

    fn cycles_per_line(&self) -> u64 {
        (self.cycles_per_frame() / self.total_lines()).max(1)
    }

    fn hblank_start_cycle(&self, cycles_per_line: u64) -> u64 {
        // Model active display time by dot count. H40: 320 visible dots,
        // H32: 256 visible dots, total line width is 342 dots.
        let active_dots = if self.h40_mode() { 320 } else { 256 };
        (cycles_per_line.saturating_mul(active_dots) / Self::TOTAL_DOTS_PER_LINE)
            .min(cycles_per_line.saturating_sub(1))
    }

    pub fn read_hv_counter(&self) -> u16 {
        let total_lines = self.total_lines();
        let cycles_per_line = self.cycles_per_line();
        let v = ((self.frame_cycles / cycles_per_line) % total_lines) as u8;
        let h = ((self.frame_cycles % cycles_per_line) * 256 / cycles_per_line) as u8;
        u16::from_be_bytes([v, h])
    }

    fn process_line_crossings(&mut self, start: u64, end: u64) {
        if end <= start {
            return;
        }
        let start_line = self.line_index_for_cycle(start);
        let end_line = self.line_index_for_cycle(end);
        let latch_next_line = std::env::var_os("MEGADRIVE_DEBUG_LINE_LATCH_NEXT").is_some();
        for line in (start_line + 1)..=end_line {
            let line = line as usize;
            // Latch line state on scanline boundary; optional line+1 probe helps
            // diagnose transition-frame timing issues.
            if latch_next_line {
                self.capture_line_state(line.saturating_add(1));
            } else {
                self.capture_line_state(line);
            }
            self.on_scanline_start(line);
        }
    }

    fn line_index_for_cycle(&self, cycle: u64) -> u64 {
        let cycles_per_frame = self.cycles_per_frame();
        // Treat cycle==frame_end as the final scanline of the current frame.
        // Frame-start events for the next frame are handled by `step` after wrap.
        let clamped = cycle.min(cycles_per_frame.saturating_sub(1));
        (clamped * self.total_lines()) / cycles_per_frame
    }

    fn reset_line_state(&mut self) {
        for line in 0..FRAME_HEIGHT {
            self.line_registers[line] = self.registers;
            self.line_vsram[line] = self.vsram;
            self.line_hscroll[line] = self.current_line_hscroll_words(line, &self.registers);
            self.line_cram[line] = self.cram;
            self.line_vram[line].copy_from_slice(&self.vram);
        }
    }

    fn capture_line_state(&mut self, line: usize) {
        if line < FRAME_HEIGHT {
            self.line_registers[line] = self.registers;
            self.line_vsram[line] = self.vsram;
            self.line_hscroll[line] = self.current_line_hscroll_words(line, &self.registers);
            self.line_cram[line] = self.cram;
            self.line_vram[line].copy_from_slice(&self.vram);
        }
    }

    fn current_line_hscroll_words(&self, line: usize, regs: &[u8; REG_COUNT]) -> [u16; 2] {
        let hscroll_base = Self::hscroll_table_base_from_regs(regs);
        let a_idx = Self::hscroll_word_index_for_line_from_regs(regs, 0, line);
        let b_idx = Self::hscroll_word_index_for_line_from_regs(regs, 1, line);
        [
            read_u16_be_wrapped(&self.vram, hscroll_base + a_idx * 2),
            read_u16_be_wrapped(&self.vram, hscroll_base + b_idx * 2),
        ]
    }

    fn on_scanline_start(&mut self, line: usize) {
        if line == self.active_display_height() && self.v_interrupt_enabled() {
            self.v_interrupt_pending = true;
        }
        if line >= self.active_display_height() {
            return;
        }

        if self.h_interrupt_counter == 0 {
            if self.h_interrupt_enabled() {
                self.h_interrupt_pending = true;
            }
            self.h_interrupt_counter = self.registers[REG_H_INTERRUPT_COUNTER];
        } else {
            self.h_interrupt_counter = self.h_interrupt_counter.wrapping_sub(1);
        }
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
            // DMA fill is triggered by a regular data-port write: apply the
            // initial write first, then stream fill bytes.
            self.write_data_value(value);
            self.advance_access_addr();
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
                if self.frame_cycles == 0 {
                    self.reset_line_state();
                    self.capture_line_state(0);
                }
            }
            AccessMode::CramWrite => {
                let index = ((self.access_addr >> 1) as usize) % CRAM_COLORS;
                self.cram[index] = value & 0x0EEE;
                if self.frame_cycles == 0 {
                    self.reset_line_state();
                    self.capture_line_state(0);
                }
            }
            AccessMode::VsramWrite => {
                let index = ((self.access_addr >> 1) as usize) % VSRAM_WORDS;
                self.vsram[index] = value & 0x07FF;
                if self.frame_cycles == 0 {
                    self.reset_line_state();
                    self.capture_line_state(0);
                }
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
        if self.frame_cycles == 0 {
            self.reset_line_state();
            self.capture_line_state(0);
        }
    }

    pub fn read_cram_u16(&self, index: u8) -> u16 {
        let i = (index as usize) % CRAM_COLORS;
        self.cram[i]
    }

    pub fn write_cram_u16(&mut self, index: u8, value: u16) {
        let i = (index as usize) % CRAM_COLORS;
        self.cram[i] = value & 0x0EEE;
        if self.frame_cycles == 0 {
            self.reset_line_state();
            self.capture_line_state(0);
        }
    }

    pub fn read_vsram_u16(&self, index: u8) -> u16 {
        let i = (index as usize) % VSRAM_WORDS;
        self.vsram[i]
    }

    pub fn write_vsram_u16(&mut self, index: u8, value: u16) {
        let i = (index as usize) % VSRAM_WORDS;
        self.vsram[i] = value & 0x07FF;
        if self.frame_cycles == 0 {
            self.reset_line_state();
            self.capture_line_state(0);
        }
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
            if self.frame_cycles == 0 {
                self.reset_line_state();
                self.capture_line_state(0);
            }
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
            (high >> 6) & 0x01
        } else {
            0b10 | ((high >> 6) & 0x01)
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
                    dest_addr: self.access_addr,
                    auto_increment: self.auto_increment(),
                    words: self.dma_length(),
                    target,
                });
            }
            // DMA fill: executes when the next data-port write provides fill value.
            0b10 => {
                if self.access_mode == AccessMode::VramWrite {
                    self.dma_fill_ops = self.dma_fill_ops.saturating_add(1);
                    self.dma_fill_pending = Some(DmaFillState {
                        remaining_words: self.dma_length(),
                    });
                }
            }
            // DMA copy: immediate VRAM-to-VRAM byte copy.
            0b11 => {
                if base_code == 0x01 && self.access_mode == AccessMode::VramWrite {
                    self.dma_copy_ops = self.dma_copy_ops.saturating_add(1);
                    self.execute_dma_copy();
                }
            }
            _ => {}
        }
    }

    fn execute_dma_fill(&mut self, words: usize, value: u16) {
        let fill_byte = (value & 0x00FF) as u8;
        let increment = self.auto_increment();
        for _ in 0..words {
            // VRAM fill writes target the byte lane selected by A0, matching
            // hardware behavior used by line-scroll effects.
            let addr = (self.access_addr as usize ^ 0x0001) % VRAM_SIZE;
            self.vram[addr] = fill_byte;
            self.access_addr = self.access_addr.wrapping_add(increment);
        }
        if self.frame_cycles == 0 {
            self.reset_line_state();
            self.capture_line_state(0);
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
        if self.frame_cycles == 0 {
            self.reset_line_state();
            self.capture_line_state(0);
        }
        self.clear_dma_length();
    }

    #[cfg(test)]
    fn nametable_base(&self) -> usize {
        Self::nametable_base_from_regs(&self.registers)
    }

    fn sprite_table_base(&self) -> usize {
        // In H40 mode the SAT base is 1KB aligned (bit0 ignored).
        let mask = if self.h40_mode() { 0x7E } else { 0x7F };
        ((self.registers[REG_SPRITE_TABLE] as usize & mask) << 9) % VRAM_SIZE
    }

    fn nametable_base_from_regs(regs: &[u8; REG_COUNT]) -> usize {
        ((regs[REG_PLANE_A_NAMETABLE] as usize & 0x38) << 10) % VRAM_SIZE
    }

    fn plane_b_nametable_base_from_regs(regs: &[u8; REG_COUNT]) -> usize {
        ((regs[REG_PLANE_B_NAMETABLE] as usize & 0x07) << 13) % VRAM_SIZE
    }

    fn hscroll_table_base_from_regs(regs: &[u8; REG_COUNT]) -> usize {
        ((regs[REG_HSCROLL_TABLE] as usize & 0x3F) << 10) % VRAM_SIZE
    }

    fn window_nametable_base_from_regs(regs: &[u8; REG_COUNT]) -> usize {
        let mask = if Self::h40_mode_from_regs(regs) {
            0x3C
        } else {
            0x3E
        };
        ((regs[REG_WINDOW_NAMETABLE] as usize & mask) << 10) % VRAM_SIZE
    }

    fn plane_tile_dimensions_from_regs(regs: &[u8; REG_COUNT]) -> (usize, usize) {
        let width_code = regs[REG_PLANE_SIZE] & 0x03;
        let height_code = (regs[REG_PLANE_SIZE] >> 4) & 0x03;
        (
            plane_size_code_to_tiles(width_code),
            plane_size_code_to_tiles(height_code),
        )
    }

    fn window_tile_dimensions_from_regs(regs: &[u8; REG_COUNT]) -> (usize, usize) {
        let width_tiles = if Self::h40_mode_from_regs(regs) {
            64
        } else {
            32
        };
        (width_tiles, 32)
    }

    fn sign_extend_11(value: u16) -> i16 {
        let masked = (value & 0x07FF) as i16;
        (masked << 5) >> 5
    }

    fn vscroll_index_for_x_from_regs(regs: &[u8; REG_COUNT], plane: usize, x: usize) -> usize {
        if (regs[11] & 0x04) == 0 {
            return plane;
        }
        ((x / 16) * 2 + plane) % VSRAM_WORDS
    }

    fn hscroll_word_index_for_line_from_regs(
        regs: &[u8; REG_COUNT],
        plane: usize,
        y: usize,
    ) -> usize {
        match regs[11] & 0x03 {
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
        vram: &[u8; VRAM_SIZE],
        base: usize,
        sample_x: usize,
        sample_y: usize,
        plane_width_tiles: usize,
        plane_height_tiles: usize,
        use_64x32_paged_layout: bool,
        scroll_plane_layout: bool,
        plane_paged_layout: bool,
        plane_paged_xmajor: bool,
    ) -> Option<PlaneSample> {
        let tile_x = (sample_x / 8) % plane_width_tiles.max(1);
        let tile_y = (sample_y / 8) % plane_height_tiles.max(1);
        let mut in_tile_x = sample_x & 7;
        let mut in_tile_y = sample_y & 7;

        let name_addr = if scroll_plane_layout {
            self.scroll_plane_name_addr(
                base,
                tile_x,
                tile_y,
                plane_width_tiles,
                plane_height_tiles,
                use_64x32_paged_layout,
                plane_paged_layout,
                plane_paged_xmajor,
            )
        } else {
            base + (tile_y * plane_width_tiles + tile_x) * 2
        };
        let entry = read_u16_be_wrapped(vram, name_addr);
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
        let tile_byte = vram[tile_addr % VRAM_SIZE];
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

    fn scroll_plane_name_addr(
        &self,
        base: usize,
        tile_x: usize,
        tile_y: usize,
        plane_width_tiles: usize,
        plane_height_tiles: usize,
        use_64x32_paged_layout: bool,
        paged_layout: bool,
        paged_xmajor: bool,
    ) -> usize {
        let wrapped_x = tile_x % plane_width_tiles.max(1);
        let wrapped_y = tile_y % plane_height_tiles.max(1);
        // Some 128-cell maps require 64x32-cell paged addressing (2KB pages).
        if use_64x32_paged_layout {
            let page_width = plane_width_tiles.max(1).div_ceil(64);
            let page_height = plane_height_tiles.max(1).div_ceil(32);
            let page_x = wrapped_x / 64;
            let page_y = wrapped_y / 32;
            let in_page_x = wrapped_x & 63;
            let in_page_y = wrapped_y & 31;
            let page_index = if paged_xmajor {
                page_x * page_height + page_y
            } else {
                page_y * page_width + page_x
            };
            return base + page_index * 64 * 32 * 2 + (in_page_y * 64 + in_page_x) * 2;
        }
        // Optional diagnostic mode: force 32x32-cell paged probing.
        if paged_layout {
            let page_width = plane_width_tiles.max(1).div_ceil(32);
            let page_x = wrapped_x / 32;
            let page_y = wrapped_y / 32;
            let in_page_x = wrapped_x & 31;
            let in_page_y = wrapped_y & 31;
            let page_height = plane_height_tiles.max(1).div_ceil(32);
            let page_index = if paged_xmajor {
                page_x * page_height + page_y
            } else {
                page_y * page_width + page_x
            };
            return base + page_index * 32 * 32 * 2 + (in_page_y * 32 + in_page_x) * 2;
        }
        base + (wrapped_y * plane_width_tiles + wrapped_x) * 2
    }

    fn compose_plane_samples(
        &self,
        front: Option<PlaneSample>,
        back: Option<PlaneSample>,
    ) -> Option<PlaneSample> {
        if std::env::var_os("MEGADRIVE_DEBUG_IGNORE_PLANE_PRIORITY").is_some() {
            return front.or(back);
        }
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

    fn window_active_at(&self, regs: &[u8; REG_COUNT], x: usize, y: usize) -> bool {
        let active_height = Self::active_display_height_from_regs(regs);
        let active_width = Self::active_display_width_from_regs(regs);
        let hreg = regs[REG_WINDOW_HPOS];
        let vreg = regs[REG_WINDOW_VPOS];
        let hsplit = (((hreg & 0x1F) as usize) * 16).min(active_width);
        let vsplit = (((vreg & 0x1F) as usize) * 8).min(active_height);
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

        let disable_plane_a = std::env::var_os("MEGADRIVE_DEBUG_DISABLE_PLANE_A").is_some();
        let disable_plane_b = std::env::var_os("MEGADRIVE_DEBUG_DISABLE_PLANE_B").is_some();
        let disable_window = std::env::var_os("MEGADRIVE_DEBUG_DISABLE_WINDOW").is_some()
            || std::env::var_os("FORCE_WINDOW_OFF").is_some();
        let disable_sprites = std::env::var_os("MEGADRIVE_DEBUG_DISABLE_SPRITES").is_some()
            || std::env::var_os("DISABLE_SPRITES").is_some();
        let invert_vscroll_a = std::env::var_os("MEGADRIVE_DEBUG_VSCROLL_INVERT_A").is_some();
        let invert_vscroll_b = std::env::var_os("MEGADRIVE_DEBUG_VSCROLL_INVERT_B").is_some();
        let swap_vscroll_ab = std::env::var_os("MEGADRIVE_DEBUG_VSCROLL_SWAP_AB").is_some();
        let plane_paged_layout = std::env::var_os("MEGADRIVE_DEBUG_PLANE_PAGED").is_some();
        let plane_paged_layout_a =
            plane_paged_layout || std::env::var_os("MEGADRIVE_DEBUG_PLANE_A_PAGED").is_some();
        let plane_paged_layout_b =
            plane_paged_layout || std::env::var_os("MEGADRIVE_DEBUG_PLANE_B_PAGED").is_some();
        let plane_paged_xmajor = std::env::var_os("MEGADRIVE_DEBUG_PLANE_PAGED_XMAJOR").is_some();
        let plane_paged_xmajor_a = plane_paged_xmajor
            || std::env::var_os("MEGADRIVE_DEBUG_PLANE_A_PAGED_XMAJOR").is_some();
        let plane_paged_xmajor_b = plane_paged_xmajor
            || std::env::var_os("MEGADRIVE_DEBUG_PLANE_B_PAGED_XMAJOR").is_some();
        let plane_live_vram = std::env::var_os("MEGADRIVE_DEBUG_PLANE_LIVE_VRAM").is_some();
        let live_cram = std::env::var_os("MEGADRIVE_DEBUG_LIVE_CRAM").is_some();
        let line_offset = std::env::var("MEGADRIVE_DEBUG_LINE_OFFSET")
            .ok()
            .and_then(|v| v.parse::<isize>().ok())
            .unwrap_or(0);
        let bottom_bg_mask = self.quirk_bottom_bg_mask
            || std::env::var_os("MEGADRIVE_DEBUG_BOTTOM_BG_MASK").is_some();
        let mut plane_meta = vec![0u8; FRAME_WIDTH * FRAME_HEIGHT];
        for y in 0..FRAME_HEIGHT {
            let line_idx = y
                .saturating_add_signed(line_offset)
                .min(FRAME_HEIGHT.saturating_sub(1));
            let regs = self
                .line_registers
                .get(line_idx)
                .copied()
                .unwrap_or(self.registers);
            let vsram = self.line_vsram.get(line_idx).copied().unwrap_or(self.vsram);
            let hscroll_words = self
                .line_hscroll
                .get(line_idx)
                .copied()
                .unwrap_or_else(|| self.current_line_hscroll_words(y, &regs));
            let hscroll_words = if self.quirk_live_hscroll
                || std::env::var_os("MEGADRIVE_DEBUG_HSCROLL_LIVE").is_some()
            {
                self.current_line_hscroll_words(line_idx, &regs)
            } else {
                hscroll_words
            };
            let cram = if live_cram {
                self.cram
            } else {
                self.line_cram.get(line_idx).copied().unwrap_or(self.cram)
            };
            let vram = if plane_live_vram || self.quirk_live_plane_vram {
                &self.vram
            } else {
                self.line_vram.get(line_idx).unwrap_or(&self.vram)
            };
            let row = y * FRAME_WIDTH * 3;
            if !Self::display_enabled_from_regs(&regs) {
                self.frame_buffer[row..row + FRAME_WIDTH * 3].fill(0);
                continue;
            }
            let line_active_height = Self::active_display_height_from_regs(&regs);
            if y >= line_active_height {
                self.frame_buffer[row..row + FRAME_WIDTH * 3].fill(0);
                continue;
            }

            let line_active_width = Self::active_display_width_from_regs(&regs);
            let plane_a_base = Self::nametable_base_from_regs(&regs);
            let plane_b_base = Self::plane_b_nametable_base_from_regs(&regs);
            let window_base = Self::window_nametable_base_from_regs(&regs);
            let (plane_width_tiles, plane_height_tiles) =
                Self::plane_tile_dimensions_from_regs(&regs);
            let (window_width_tiles, window_height_tiles) =
                Self::window_tile_dimensions_from_regs(&regs);
            let disable_64x32_paged =
                std::env::var_os("MEGADRIVE_DEBUG_DISABLE_64X32_PAGED").is_some();
            let disable_64x32_paged_a =
                std::env::var_os("MEGADRIVE_DEBUG_DISABLE_64X32_PAGED_A").is_some();
            let disable_64x32_paged_b =
                std::env::var_os("MEGADRIVE_DEBUG_DISABLE_64X32_PAGED_B").is_some();
            let plane_a_uses_64x32_paged = !disable_64x32_paged
                && !disable_64x32_paged_a
                && (std::env::var_os("MEGADRIVE_DEBUG_PLANE_A_64X32_PAGED").is_some()
                    || (self.quirk_plane_a_64x32_paged && plane_width_tiles > 64));
            let plane_b_uses_64x32_paged = !disable_64x32_paged
                && !disable_64x32_paged_b
                && std::env::var_os("MEGADRIVE_DEBUG_PLANE_B_64X32_PAGED").is_some();
            let plane_width_px = plane_width_tiles * 8;
            let plane_height_px = plane_height_tiles * 8;
            let window_width_px = window_width_tiles * 8;
            let window_height_px = window_height_tiles * 8;
            let bg_color_index = Self::background_color_index_from_regs(&regs);

            let a_hscroll =
                normalize_scroll(Self::sign_extend_11(hscroll_words[0]), plane_width_px);
            let b_hscroll =
                normalize_scroll(Self::sign_extend_11(hscroll_words[1]), plane_width_px);

            for x in 0..FRAME_WIDTH {
                if x >= line_active_width {
                    let out = row + x * 3;
                    self.frame_buffer[out] = 0;
                    self.frame_buffer[out + 1] = 0;
                    self.frame_buffer[out + 2] = 0;
                    continue;
                }
                let (a_idx, b_idx) = if swap_vscroll_ab {
                    (1usize, 0usize)
                } else {
                    (0usize, 1usize)
                };
                let a_vscroll = normalize_scroll(
                    Self::sign_extend_11(
                        vsram[Self::vscroll_index_for_x_from_regs(&regs, a_idx, x) % VSRAM_WORDS],
                    ),
                    plane_height_px,
                );
                let b_vscroll = normalize_scroll(
                    Self::sign_extend_11(
                        vsram[Self::vscroll_index_for_x_from_regs(&regs, b_idx, x) % VSRAM_WORDS],
                    ),
                    plane_height_px,
                );
                let plane_b = if disable_plane_b {
                    None
                } else {
                    let sample_y = if invert_vscroll_b {
                        (y + plane_height_px - b_vscroll) % plane_height_px
                    } else {
                        (y + b_vscroll) % plane_height_px
                    };
                    self.sample_plane_pixel(
                        vram,
                        plane_b_base,
                        (x + plane_width_px - b_hscroll) % plane_width_px,
                        sample_y,
                        plane_width_tiles,
                        plane_height_tiles,
                        plane_b_uses_64x32_paged,
                        true,
                        plane_paged_layout_b,
                        plane_paged_xmajor_b,
                    )
                };

                let front_plane = if !disable_window && self.window_active_at(&regs, x, y) {
                    self.sample_plane_pixel(
                        vram,
                        window_base,
                        x % window_width_px,
                        y % window_height_px,
                        window_width_tiles,
                        window_height_tiles,
                        false,
                        false,
                        false,
                        false,
                    )
                } else {
                    let sample_y = if invert_vscroll_a {
                        (y + plane_height_px - a_vscroll) % plane_height_px
                    } else {
                        (y + a_vscroll) % plane_height_px
                    };
                    self.sample_plane_pixel(
                        vram,
                        plane_a_base,
                        (x + plane_width_px - a_hscroll) % plane_width_px,
                        sample_y,
                        plane_width_tiles,
                        plane_height_tiles,
                        plane_a_uses_64x32_paged,
                        true,
                        plane_paged_layout_a,
                        plane_paged_xmajor_a,
                    )
                };
                let front_plane = if disable_plane_a { None } else { front_plane };

                let mut composed = self.compose_plane_samples(front_plane, plane_b);
                if bottom_bg_mask && y >= line_active_height.saturating_sub(32) {
                    composed = None;
                }
                let color_index = composed
                    .map(|sample| sample.color_index)
                    .unwrap_or(bg_color_index);
                let color = cram[color_index % CRAM_COLORS];
                let (r, g, b) = md_color_to_rgb888(color);

                let out = row + x * 3;
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

        if !disable_sprites {
            self.render_sprites(&plane_meta);
        }
    }

    fn render_sprites(&mut self, plane_meta: &[u8]) {
        const MAX_SPRITES: usize = 80;
        let sat_use_live = std::env::var_os("MEGADRIVE_DEBUG_SAT_LIVE").is_some();
        let sat_use_line_latched =
            std::env::var_os("MEGADRIVE_DEBUG_SAT_LINE_LATCH").is_some() || !sat_use_live;
        let sat_per_line = std::env::var_os("MEGADRIVE_DEBUG_SAT_PER_LINE").is_some();
        let sprite_x_offset = std::env::var("MEGADRIVE_DEBUG_SPRITE_X_OFFSET")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        let sprite_y_offset = std::env::var("MEGADRIVE_DEBUG_SPRITE_Y_OFFSET")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        if sat_per_line {
            self.render_sprites_per_line(
                plane_meta,
                sat_use_live,
                sprite_x_offset,
                sprite_y_offset,
            );
            return;
        }
        let mut sprites_on_line = [0u8; FRAME_HEIGHT];
        let mut sprite_pixels_on_line = [0u16; FRAME_HEIGHT];
        let mut masked_line = [false; FRAME_HEIGHT];
        let mut sprite_filled = vec![false; FRAME_WIDTH * FRAME_HEIGHT];
        let mut index = 0usize;

        for _ in 0..MAX_SPRITES {
            let entry_addr = self.sprite_table_base() + index * 8;
            let (mut y_word, mut size_link, mut attr, mut x_word) = {
                // Default to line-0 latched SAT to avoid next-frame SAT leaking
                // into current output. Optional live-SAT mode helps diagnostics.
                let sat_vram = if sat_use_live {
                    &self.vram
                } else {
                    self.line_vram.first().unwrap_or(&self.vram)
                };
                (
                    read_u16_be_wrapped(sat_vram, entry_addr),
                    read_u16_be_wrapped(sat_vram, entry_addr + 2),
                    read_u16_be_wrapped(sat_vram, entry_addr + 4),
                    read_u16_be_wrapped(sat_vram, entry_addr + 6),
                )
            };
            if sat_use_line_latched {
                let y = (y_word & 0x03FF) as i32 - 128;
                let line = y.clamp(0, (FRAME_HEIGHT - 1) as i32) as usize;
                let sat_vram = self.line_vram.get(line).unwrap_or(&self.vram);
                y_word = read_u16_be_wrapped(sat_vram, entry_addr);
                size_link = read_u16_be_wrapped(sat_vram, entry_addr + 2);
                attr = read_u16_be_wrapped(sat_vram, entry_addr + 4);
                x_word = read_u16_be_wrapped(sat_vram, entry_addr + 6);
            }

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
                sprite_x_offset,
                sprite_y_offset,
            );

            let link = (size_link & 0x007F) as usize;
            if link == 0 || link == index || link >= MAX_SPRITES {
                break;
            }
            index = link;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_sprites_per_line(
        &mut self,
        plane_meta: &[u8],
        sat_use_live: bool,
        sprite_x_offset: i32,
        sprite_y_offset: i32,
    ) {
        const MAX_SPRITES: usize = 80;
        let swap_size = std::env::var_os("MEGADRIVE_DEBUG_SPRITE_SWAP_SIZE").is_some();
        let sprite_pattern_line0 = std::env::var_os("MEGADRIVE_DEBUG_SPRITE_PATTERN_LINE0")
            .is_some()
            && std::env::var_os("MEGADRIVE_DEBUG_SPRITE_PATTERN_PER_LINE").is_none();
        let sprite_row_major = std::env::var_os("MEGADRIVE_DEBUG_SPRITE_ROW_MAJOR").is_some();
        let control_no_occupy = std::env::var_os("MEGADRIVE_DEBUG_CONTROL_NO_OCCUPY").is_some();
        let control_behind_hi_plane =
            std::env::var_os("MEGADRIVE_DEBUG_CONTROL_BEHIND_HIPLANE").is_some();
        let control_require_plane_opaque =
            std::env::var_os("MEGADRIVE_DEBUG_CONTROL_REQUIRE_PLANE_OPAQUE").is_some();
        let disable_mask_sprite = std::env::var_os("MEGADRIVE_DEBUG_DISABLE_SPRITE_MASK").is_some();

        let mut sprite_filled = vec![false; FRAME_WIDTH * FRAME_HEIGHT];
        let sat_base = self.sprite_table_base();
        for dy in 0..FRAME_HEIGHT {
            let regs = self
                .line_registers
                .get(dy)
                .copied()
                .unwrap_or(self.registers);
            if !Self::display_enabled_from_regs(&regs) {
                continue;
            }
            let line_active_height = Self::active_display_height_from_regs(&regs);
            if dy >= line_active_height {
                continue;
            }
            let line_active_width = Self::active_display_width_from_regs(&regs);
            let (max_sprites_per_line, max_pixels_per_line) = if Self::h40_mode_from_regs(&regs) {
                (20usize, line_active_width)
            } else {
                (16usize, line_active_width)
            };
            let sat_vram = if sat_use_live {
                &self.vram
            } else {
                self.line_vram.get(dy).unwrap_or(&self.vram)
            };
            let pattern_vram = if sprite_pattern_line0 {
                self.line_vram.first().unwrap_or(&self.vram)
            } else {
                self.line_vram.get(dy).unwrap_or(&self.vram)
            };

            let mut masked = false;
            let mut line_sprites = 0usize;
            let mut line_pixels = 0usize;
            let mut index = 0usize;
            let mut visited = [false; MAX_SPRITES];

            for _ in 0..MAX_SPRITES {
                if index >= MAX_SPRITES || visited[index] {
                    break;
                }
                visited[index] = true;
                let entry_addr = sat_base + index * 8;
                let y_word = read_u16_be_wrapped(sat_vram, entry_addr);
                let size_link = read_u16_be_wrapped(sat_vram, entry_addr + 2);
                let attr = read_u16_be_wrapped(sat_vram, entry_addr + 4);
                let x_word = read_u16_be_wrapped(sat_vram, entry_addr + 6);
                let link = (size_link & 0x007F) as usize;

                let x = (x_word & 0x01FF) as i32 - 128 + sprite_x_offset;
                let y = (y_word & 0x03FF) as i32 - 128 + sprite_y_offset;
                let is_mask_sprite = (x_word & 0x01FF) == 0 && !disable_mask_sprite;
                let (width_tiles, height_tiles) = if swap_size {
                    (
                        ((size_link >> 8) & 0x3) as usize + 1,
                        ((size_link >> 10) & 0x3) as usize + 1,
                    )
                } else {
                    (
                        ((size_link >> 10) & 0x3) as usize + 1,
                        ((size_link >> 8) & 0x3) as usize + 1,
                    )
                };
                let width_px = width_tiles * 8;
                let height_px = height_tiles * 8;
                let dy_i32 = dy as i32;
                let covered = dy_i32 >= y && dy_i32 < y + height_px as i32;
                if covered {
                    if is_mask_sprite {
                        masked = true;
                    } else if !masked {
                        if line_sprites >= max_sprites_per_line {
                            self.sprite_overflow = true;
                        } else {
                            line_sprites += 1;
                            let sprite_priority_high = (attr & 0x8000) != 0;
                            let tile_base = (attr & 0x07FF) as usize;
                            let palette_line = ((attr >> 13) & 0x3) as usize;
                            let hflip = (attr & 0x0800) != 0;
                            let vflip = (attr & 0x1000) != 0;
                            let line_shadow_highlight =
                                Self::shadow_highlight_mode_from_regs(&regs);
                            let sy = (dy_i32 - y) as usize;
                            let src_y = if vflip { height_px - 1 - sy } else { sy };
                            let tile_row = src_y / 8;
                            let in_tile_y = src_y & 7;
                            for sx in 0..width_px {
                                if line_pixels >= max_pixels_per_line {
                                    self.sprite_overflow = true;
                                    break;
                                }
                                // Consume sprite dot budget including transparent/offscreen dots.
                                line_pixels += 1;

                                let src_x = if hflip { width_px - 1 - sx } else { sx };
                                let dx = x + sx as i32;
                                if !(0..line_active_width as i32).contains(&dx) {
                                    continue;
                                }
                                let tile_col = src_x / 8;
                                let in_tile_x = src_x & 7;
                                let tile_index = if sprite_row_major {
                                    tile_base + tile_row * width_tiles + tile_col
                                } else {
                                    tile_base + tile_col * height_tiles + tile_row
                                };
                                let tile_addr =
                                    tile_index * TILE_SIZE_BYTES + in_tile_y * 4 + in_tile_x / 2;
                                let tile_byte = pattern_vram[tile_addr % VRAM_SIZE];
                                let pixel = if in_tile_x & 1 == 0 {
                                    tile_byte >> 4
                                } else {
                                    tile_byte & 0x0F
                                };
                                if pixel == 0 {
                                    continue;
                                }

                                let meta_index = dy * FRAME_WIDTH + dx as usize;
                                let meta = plane_meta[meta_index];
                                let plane_opaque = (meta & 0x01) != 0;
                                let plane_priority_high = (meta & 0x02) != 0;
                                if !sprite_priority_high && plane_opaque && plane_priority_high {
                                    continue;
                                }

                                if line_shadow_highlight
                                    && palette_line == 3
                                    && (pixel == 14 || pixel == 15)
                                {
                                    if control_require_plane_opaque && !plane_opaque {
                                        continue;
                                    }
                                    if control_behind_hi_plane
                                        && plane_opaque
                                        && plane_priority_high
                                    {
                                        continue;
                                    }
                                    if !control_no_occupy && sprite_filled[meta_index] {
                                        self.sprite_collision = true;
                                        continue;
                                    }
                                    let out = meta_index * 3;
                                    if pixel == 15 {
                                        self.frame_buffer[out] =
                                            shadow_channel(self.frame_buffer[out]);
                                        self.frame_buffer[out + 1] =
                                            shadow_channel(self.frame_buffer[out + 1]);
                                        self.frame_buffer[out + 2] =
                                            shadow_channel(self.frame_buffer[out + 2]);
                                    } else {
                                        self.frame_buffer[out] =
                                            highlight_channel(self.frame_buffer[out]);
                                        self.frame_buffer[out + 1] =
                                            highlight_channel(self.frame_buffer[out + 1]);
                                        self.frame_buffer[out + 2] =
                                            highlight_channel(self.frame_buffer[out + 2]);
                                    }
                                    if !control_no_occupy {
                                        sprite_filled[meta_index] = true;
                                    }
                                    continue;
                                }

                                let color_index = palette_line * 16 + pixel as usize;
                                let color = self.line_cram[dy][color_index % CRAM_COLORS];
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
                }

                if link == 0 || link == index || link >= MAX_SPRITES {
                    break;
                }
                index = link;
            }
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
        sprite_x_offset: i32,
        sprite_y_offset: i32,
    ) {
        // Sprite X coordinate is 9-bit (0..511), offset by 128.
        let x = (x_word & 0x01FF) as i32 - 128 + sprite_x_offset;
        let y = (y_word & 0x03FF) as i32 - 128 + sprite_y_offset;
        let swap_size = std::env::var_os("MEGADRIVE_DEBUG_SPRITE_SWAP_SIZE").is_some();
        let (width_tiles, height_tiles) = if swap_size {
            (
                ((size_link >> 8) & 0x3) as usize + 1,
                ((size_link >> 10) & 0x3) as usize + 1,
            )
        } else {
            (
                ((size_link >> 10) & 0x3) as usize + 1,
                ((size_link >> 8) & 0x3) as usize + 1,
            )
        };
        let sprite_priority_high = (attr & 0x8000) != 0;
        let tile_base = (attr & 0x07FF) as usize;
        let palette_line = ((attr >> 13) & 0x3) as usize;
        let hflip = (attr & 0x0800) != 0;
        let vflip = (attr & 0x1000) != 0;
        let width_px = width_tiles * 8;
        let height_px = height_tiles * 8;
        let disable_mask_sprite = std::env::var_os("MEGADRIVE_DEBUG_DISABLE_SPRITE_MASK").is_some();
        let is_mask_sprite = (x_word & 0x01FF) == 0 && !disable_mask_sprite;
        let sprite_pattern_line0 = std::env::var_os("MEGADRIVE_DEBUG_SPRITE_PATTERN_LINE0")
            .is_some()
            && std::env::var_os("MEGADRIVE_DEBUG_SPRITE_PATTERN_PER_LINE").is_none();
        let sprite_row_major = std::env::var_os("MEGADRIVE_DEBUG_SPRITE_ROW_MAJOR").is_some();
        let control_no_occupy = std::env::var_os("MEGADRIVE_DEBUG_CONTROL_NO_OCCUPY").is_some();

        for sy in 0..height_px {
            let src_y = if vflip { height_px - 1 - sy } else { sy };
            let dy = y + sy as i32;
            if !(0..FRAME_HEIGHT as i32).contains(&dy) {
                continue;
            }
            let dy_index = dy as usize;
            let regs = self
                .line_registers
                .get(dy_index)
                .copied()
                .unwrap_or(self.registers);
            if !Self::display_enabled_from_regs(&regs) {
                continue;
            }
            let line_active_height = Self::active_display_height_from_regs(&regs);
            if dy_index >= line_active_height {
                continue;
            }
            let line_active_width = Self::active_display_width_from_regs(&regs);
            let (line_max_sprites_per_line, line_max_pixels_per_line) =
                if Self::h40_mode_from_regs(&regs) {
                    (20usize, line_active_width)
                } else {
                    (16usize, line_active_width)
                };
            let line_shadow_highlight = Self::shadow_highlight_mode_from_regs(&regs);
            if is_mask_sprite {
                masked_line[dy_index] = true;
                continue;
            }
            if masked_line[dy_index] {
                continue;
            }
            if sprites_on_line[dy_index] as usize >= line_max_sprites_per_line {
                self.sprite_overflow = true;
                continue;
            }
            sprites_on_line[dy_index] = sprites_on_line[dy_index].saturating_add(1);

            let tile_row = src_y / 8;
            let in_tile_y = src_y & 7;
            for sx in 0..width_px {
                let src_x = if hflip { width_px - 1 - sx } else { sx };
                let dx = x + sx as i32;
                if sprite_pixels_on_line[dy_index] as usize >= line_max_pixels_per_line {
                    self.sprite_overflow = true;
                    break;
                }
                // VDP line sprite budget is consumed by visible sprite dots,
                // including transparent/offscreen pixels.
                sprite_pixels_on_line[dy_index] = sprite_pixels_on_line[dy_index].saturating_add(1);
                if !(0..line_active_width as i32).contains(&dx) {
                    continue;
                }

                let tile_col = src_x / 8;
                let in_tile_x = src_x & 7;
                let tile_index = if sprite_row_major {
                    // Diagnostic: row-major order.
                    tile_base + tile_row * width_tiles + tile_col
                } else {
                    // Sprite pattern index advances in column-major order on the MD VDP.
                    tile_base + tile_col * height_tiles + tile_row
                };
                let tile_addr = tile_index * TILE_SIZE_BYTES + in_tile_y * 4 + in_tile_x / 2;
                let tile_byte = {
                    let vram = if sprite_pattern_line0 {
                        self.line_vram.first().unwrap_or(&self.vram)
                    } else {
                        self.line_vram.get(dy_index).unwrap_or(&self.vram)
                    };
                    vram[tile_addr % VRAM_SIZE]
                };
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

                if line_shadow_highlight && palette_line == 3 && (pixel == 14 || pixel == 15) {
                    let control_behind_hi_plane =
                        std::env::var_os("MEGADRIVE_DEBUG_CONTROL_BEHIND_HIPLANE").is_some();
                    let control_require_plane_opaque =
                        std::env::var_os("MEGADRIVE_DEBUG_CONTROL_REQUIRE_PLANE_OPAQUE").is_some();
                    if control_require_plane_opaque && !plane_opaque {
                        continue;
                    }
                    if control_behind_hi_plane && plane_opaque && plane_priority_high {
                        continue;
                    }
                    if !control_no_occupy && sprite_filled[meta_index] {
                        self.sprite_collision = true;
                        continue;
                    }
                    let out = meta_index * 3;
                    if pixel == 15 {
                        // Shadow control color.
                        self.frame_buffer[out] = shadow_channel(self.frame_buffer[out]);
                        self.frame_buffer[out + 1] = shadow_channel(self.frame_buffer[out + 1]);
                        self.frame_buffer[out + 2] = shadow_channel(self.frame_buffer[out + 2]);
                    } else {
                        // Highlight control color.
                        self.frame_buffer[out] = highlight_channel(self.frame_buffer[out]);
                        self.frame_buffer[out + 1] = highlight_channel(self.frame_buffer[out + 1]);
                        self.frame_buffer[out + 2] = highlight_channel(self.frame_buffer[out + 2]);
                    }
                    // Diagnostic mode can disable control-pixel occupancy to validate
                    // shadow/highlight ordering against real games.
                    if !control_no_occupy {
                        sprite_filled[meta_index] = true;
                    }
                    continue;
                }

                let color_index = palette_line * 16 + pixel as usize;
                let color = self.line_cram[dy_index][color_index % CRAM_COLORS];
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

    fn h_interrupt_enabled(&self) -> bool {
        (self.registers[REG_MODE_SET_1] & 0x10) != 0
    }

    fn v_interrupt_enabled(&self) -> bool {
        (self.registers[REG_MODE_SET_2] & 0x20) != 0
    }

    fn h40_mode(&self) -> bool {
        Self::h40_mode_from_regs(&self.registers)
    }

    fn shadow_highlight_mode_from_regs(regs: &[u8; REG_COUNT]) -> bool {
        // Mode register 12 bit 3 enables shadow/highlight processing.
        (regs[12] & 0x08) != 0
    }

    fn active_display_height(&self) -> usize {
        Self::active_display_height_from_regs(&self.registers)
    }

    fn display_enabled_from_regs(regs: &[u8; REG_COUNT]) -> bool {
        (regs[REG_MODE_SET_2] & 0x40) != 0
    }

    fn h40_mode_from_regs(regs: &[u8; REG_COUNT]) -> bool {
        (regs[12] & 0x01) != 0
    }

    fn active_display_height_from_regs(regs: &[u8; REG_COUNT]) -> usize {
        if (regs[REG_MODE_SET_2] & 0x08) != 0 {
            FRAME_HEIGHT
        } else {
            FRAME_HEIGHT_28_CELL
        }
    }

    fn active_display_width_from_regs(regs: &[u8; REG_COUNT]) -> usize {
        if Self::h40_mode_from_regs(regs) {
            FRAME_WIDTH
        } else {
            FRAME_WIDTH_32_CELL
        }
    }

    fn background_color_index_from_regs(regs: &[u8; REG_COUNT]) -> usize {
        let bg = regs[REG_BACKGROUND_COLOR];
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

#[cfg(test)]
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

fn rgb888_to_md_level(channel: u8) -> u8 {
    ((channel as u16 + 18) / 36).min(7) as u8
}

fn md_level_to_rgb888(level: u8) -> u8 {
    (level.min(7) as u16 * 36) as u8
}

fn shadow_channel(channel: u8) -> u8 {
    md_level_to_rgb888(rgb888_to_md_level(channel) / 2)
}

fn highlight_channel(channel: u8) -> u8 {
    let level = ((rgb888_to_md_level(channel) as u16 * 3) / 2).min(7) as u8;
    md_level_to_rgb888(level)
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
#[path = "tests/vdp_tests.rs"]
mod tests;
