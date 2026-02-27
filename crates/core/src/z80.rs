use crate::audio::AudioBus;
use crate::cartridge::Cartridge;
use crate::input::IoBus;
use crate::vdp::Vdp;
use std::collections::BTreeMap;

const FLAG_S: u8 = 0x80;
const FLAG_Z: u8 = 0x40;
const FLAG_C: u8 = 0x01;
const IO_VERSION_ADDR: u32 = 0xA10000;
const IO_PORT1_DATA_ADDR: u32 = 0xA10002;
const IO_PORT2_DATA_ADDR: u32 = 0xA10004;
const IO_PORT1_CTRL_ADDR: u32 = 0xA10008;
const IO_PORT2_CTRL_ADDR: u32 = 0xA1000A;

struct Z80Bus<'a> {
    audio: &'a mut AudioBus,
    cartridge: &'a Cartridge,
    work_ram: &'a mut [u8; 0x10000],
    vdp: &'a mut Vdp,
    io: &'a mut IoBus,
}

#[derive(Debug, Clone)]
pub struct Z80 {
    bus_requested: bool,
    bus_granted: bool,
    bus_grant_delay_cycles: u32,
    reset_asserted: bool,
    cycles: u64,
    ram: [u8; 0x2000],
    a: u8,
    f: u8,
    a_alt: u8,
    f_alt: u8,
    b: u8,
    c: u8,
    d: u8,
    e: u8,
    h: u8,
    l: u8,
    b_alt: u8,
    c_alt: u8,
    d_alt: u8,
    e_alt: u8,
    h_alt: u8,
    l_alt: u8,
    ix: u16,
    iy: u16,
    pc: u16,
    sp: u16,
    bank_address: u32,
    vdp_data_write_latch: u16,
    vdp_control_write_latch: u16,
    iff1: bool,
    iff2: bool,
    interrupt_pending: bool,
    halted: bool,
    unknown_opcode_total: u64,
    unknown_opcode_histogram: BTreeMap<u8, u64>,
    unknown_opcode_pc_histogram: BTreeMap<u16, u64>,
}

impl Default for Z80 {
    fn default() -> Self {
        Self {
            bus_requested: false,
            bus_granted: false,
            bus_grant_delay_cycles: 0,
            reset_asserted: true,
            cycles: 0,
            ram: [0; 0x2000],
            a: 0,
            f: 0,
            a_alt: 0,
            f_alt: 0,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            b_alt: 0,
            c_alt: 0,
            d_alt: 0,
            e_alt: 0,
            h_alt: 0,
            l_alt: 0,
            ix: 0,
            iy: 0,
            pc: 0,
            sp: 0x1FFF,
            bank_address: 0,
            vdp_data_write_latch: 0,
            vdp_control_write_latch: 0,
            iff1: false,
            iff2: false,
            interrupt_pending: false,
            halted: false,
            unknown_opcode_total: 0,
            unknown_opcode_histogram: BTreeMap::new(),
            unknown_opcode_pc_histogram: BTreeMap::new(),
        }
    }
}

impl Z80 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_busreq_byte(&self) -> u8 {
        // BUSREQ bit is active-low when read:
        // 0 => 68k bus request has been granted (Z80 halted)
        // 1 => bus still owned by Z80 / grant pending
        if self.bus_granted { 0x00 } else { 0x01 }
    }

    pub fn write_busreq_byte(&mut self, value: u8) {
        let requested = (value & 0x01) != 0;
        if requested {
            if !self.bus_requested {
                self.bus_requested = true;
                self.bus_grant_delay_cycles = 16;
            }
        } else {
            self.bus_requested = false;
            self.bus_granted = false;
            self.bus_grant_delay_cycles = 0;
        }
    }

    pub fn read_reset_byte(&self) -> u8 {
        if self.reset_asserted { 0x00 } else { 0x01 }
    }

    pub fn reset_asserted(&self) -> bool {
        self.reset_asserted
    }

    pub fn bus_requested(&self) -> bool {
        self.bus_requested
    }

    pub fn bus_granted(&self) -> bool {
        self.bus_granted
    }

    pub fn write_reset_byte(&mut self, value: u8) {
        let next_asserted = (value & 0x01) == 0;
        if self.reset_asserted && !next_asserted {
            self.a = 0;
            self.a_alt = 0;
            self.b = 0;
            self.c = 0;
            self.d = 0;
            self.e = 0;
            self.h = 0;
            self.l = 0;
            self.b_alt = 0;
            self.c_alt = 0;
            self.d_alt = 0;
            self.e_alt = 0;
            self.h_alt = 0;
            self.l_alt = 0;
            self.ix = 0;
            self.iy = 0;
            self.pc = 0;
            self.sp = 0x1FFF;
            self.bank_address = 0;
            self.vdp_data_write_latch = 0;
            self.vdp_control_write_latch = 0;
            self.iff1 = false;
            self.iff2 = false;
            self.interrupt_pending = false;
            self.halted = false;
            self.f = 0;
            self.f_alt = 0;
            self.unknown_opcode_total = 0;
            self.unknown_opcode_histogram.clear();
            self.unknown_opcode_pc_histogram.clear();
        }
        self.reset_asserted = next_asserted;
    }

    pub fn m68k_can_access_ram(&self) -> bool {
        self.bus_granted
    }

    pub fn request_interrupt(&mut self) {
        self.interrupt_pending = true;
    }

    pub fn read_ram_u8(&self, addr: u16) -> u8 {
        self.ram[(addr as usize) & 0x1FFF]
    }

    pub fn write_ram_u8(&mut self, addr: u16, value: u8) {
        self.ram[(addr as usize) & 0x1FFF] = value;
    }

    pub fn step(
        &mut self,
        m68k_cycles: u32,
        audio: &mut AudioBus,
        cartridge: &Cartridge,
        work_ram: &mut [u8; 0x10000],
        vdp: &mut Vdp,
        io: &mut IoBus,
    ) {
        let mut bus = Z80Bus {
            audio,
            cartridge,
            work_ram,
            vdp,
            io,
        };
        let z80_can_run = !self.reset_asserted && (!self.bus_requested || !self.bus_granted);
        if self.bus_requested && !self.bus_granted {
            if m68k_cycles >= self.bus_grant_delay_cycles {
                self.bus_granted = true;
                self.bus_grant_delay_cycles = 0;
            } else {
                self.bus_grant_delay_cycles -= m68k_cycles;
            }
        }

        if !z80_can_run {
            return;
        }

        let budget = (m68k_cycles as usize) / 2;
        if budget == 0 {
            return;
        }

        let mut used = 0usize;
        let mut guard = 0usize;
        while used < budget && guard < 2048 {
            guard += 1;
            if self.interrupt_pending && self.iff1 {
                self.interrupt_pending = false;
                self.iff1 = false;
                self.halted = false;
                self.push_u16(self.pc, &mut bus);
                self.pc = 0x0038;
                used += 13;
                continue;
            }
            if self.halted {
                break;
            }
            let opcode_pc = self.pc;
            let opcode = self.fetch_u8(&bus);
            used += self.exec_opcode(opcode_pc, opcode, &mut bus) as usize;
        }

        // Account wall-clock Z80 time even if halted or blocked by unsupported opcodes.
        self.cycles += budget as u64;
    }

    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    pub fn pc(&self) -> u16 {
        self.pc
    }

    pub fn halted(&self) -> bool {
        self.halted
    }

    pub fn unknown_opcode_total(&self) -> u64 {
        self.unknown_opcode_total
    }

    pub fn unknown_opcode_histogram(&self) -> Vec<(u8, u64)> {
        let mut entries: Vec<(u8, u64)> = self
            .unknown_opcode_histogram
            .iter()
            .map(|(opcode, count)| (*opcode, *count))
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        entries
    }

    pub fn unknown_opcode_pc_histogram(&self) -> Vec<(u16, u64)> {
        let mut entries: Vec<(u16, u64)> = self
            .unknown_opcode_pc_histogram
            .iter()
            .map(|(pc, count)| (*pc, *count))
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        entries
    }

    fn exec_opcode(&mut self, opcode_pc: u16, opcode: u8, bus: &mut Z80Bus<'_>) -> u8 {
        match opcode {
            0x00 => 4, // NOP
            0x76 => {
                self.halted = true;
                4
            }
            0xCB => {
                let op2 = self.fetch_u8(bus);
                self.exec_cb(op2, bus)
            }
            0xED => {
                let op2 = self.fetch_u8(bus);
                self.exec_ed(opcode_pc, op2, bus)
            }
            0xDD => self.exec_index_prefix(opcode_pc, true, bus),
            0xFD => self.exec_index_prefix(opcode_pc, false, bus),
            0x3E => {
                self.a = self.fetch_u8(bus);
                7
            }
            0x06 => {
                self.b = self.fetch_u8(bus);
                7
            }
            0x0E => {
                self.c = self.fetch_u8(bus);
                7
            }
            0x16 => {
                self.d = self.fetch_u8(bus);
                7
            }
            0x1E => {
                self.e = self.fetch_u8(bus);
                7
            }
            0x0A => {
                self.a = self.read_byte(self.bc(), bus);
                7
            }
            0x1A => {
                self.a = self.read_byte(self.de(), bus);
                7
            }
            0x12 => {
                self.write_byte(self.de(), self.a, bus);
                7
            }
            0x02 => {
                self.write_byte(self.bc(), self.a, bus);
                7
            }
            0x26 => {
                self.h = self.fetch_u8(bus);
                7
            }
            0x2E => {
                self.l = self.fetch_u8(bus);
                7
            }
            0x01 => {
                let value = self.fetch_u16(bus);
                self.set_bc(value);
                10
            }
            0x11 => {
                let value = self.fetch_u16(bus);
                self.set_de(value);
                10
            }
            0x03 => {
                self.set_bc(self.bc().wrapping_add(1));
                6
            }
            0x13 => {
                self.set_de(self.de().wrapping_add(1));
                6
            }
            0x0B => {
                self.set_bc(self.bc().wrapping_sub(1));
                6
            }
            0x1B => {
                self.set_de(self.de().wrapping_sub(1));
                6
            }
            0x21 => {
                let value = self.fetch_u16(bus);
                self.set_hl(value);
                10
            }
            0x31 => {
                self.sp = self.fetch_u16(bus);
                10
            }
            0x3B => {
                self.sp = self.sp.wrapping_sub(1);
                6
            }
            0x32 => {
                let addr = self.fetch_u16(bus);
                self.write_byte(addr, self.a, bus);
                13
            }
            0x3A => {
                let addr = self.fetch_u16(bus);
                self.a = self.read_byte(addr, bus);
                13
            }
            0x22 => {
                let addr = self.fetch_u16(bus);
                let [lo, hi] = self.hl().to_le_bytes();
                self.write_byte(addr, lo, bus);
                self.write_byte(addr.wrapping_add(1), hi, bus);
                16
            }
            0x2A => {
                let addr = self.fetch_u16(bus);
                let lo = self.read_byte(addr, bus);
                let hi = self.read_byte(addr.wrapping_add(1), bus);
                self.set_hl(u16::from_le_bytes([lo, hi]));
                16
            }
            0x36 => {
                let value = self.fetch_u8(bus);
                let addr = self.hl();
                self.write_byte(addr, value, bus);
                10
            }
            0x77 => {
                let addr = self.hl();
                self.write_byte(addr, self.a, bus);
                7
            }
            0x7E => {
                let addr = self.hl();
                self.a = self.read_byte(addr, bus);
                7
            }
            0x23 => {
                self.set_hl(self.hl().wrapping_add(1));
                6
            }
            0x2B => {
                self.set_hl(self.hl().wrapping_sub(1));
                6
            }
            0x09 => {
                self.add_hl(self.bc());
                11
            }
            0x19 => {
                self.add_hl(self.de());
                11
            }
            0x29 => {
                self.add_hl(self.hl());
                11
            }
            0x39 => {
                self.add_hl(self.sp);
                11
            }
            0xAF => {
                self.a = 0;
                self.f = FLAG_Z;
                4
            }
            0x80..=0x87 => {
                let src = opcode & 0x07;
                let value = self.read_reg_code(src, bus);
                self.add_a(value);
                if src == 0b110 { 7 } else { 4 }
            }
            0x88..=0x8F => {
                let src = opcode & 0x07;
                let value = self.read_reg_code(src, bus);
                self.adc_a(value);
                if src == 0b110 { 7 } else { 4 }
            }
            0x98..=0x9F => {
                let src = opcode & 0x07;
                let value = self.read_reg_code(src, bus);
                self.sbc_a(value);
                if src == 0b110 { 7 } else { 4 }
            }
            0x90..=0x97 => {
                let src = opcode & 0x07;
                let value = self.read_reg_code(src, bus);
                self.sub_a(value);
                if src == 0b110 { 7 } else { 4 }
            }
            0xA0..=0xA7 => {
                let src = opcode & 0x07;
                let value = self.read_reg_code(src, bus);
                self.a &= value;
                self.update_sz_clear_c(self.a);
                if src == 0b110 { 7 } else { 4 }
            }
            0xA8..=0xAF => {
                let src = opcode & 0x07;
                let value = self.read_reg_code(src, bus);
                self.a ^= value;
                self.update_sz_clear_c(self.a);
                if src == 0b110 { 7 } else { 4 }
            }
            0xB0..=0xB7 => {
                let src = opcode & 0x07;
                let value = self.read_reg_code(src, bus);
                self.a |= value;
                self.update_sz_clear_c(self.a);
                if src == 0b110 { 7 } else { 4 }
            }
            0xB8..=0xBF => {
                let src = opcode & 0x07;
                let value = self.read_reg_code(src, bus);
                let result = self.a.wrapping_sub(value);
                self.f = 0;
                if result == 0 {
                    self.f |= FLAG_Z;
                }
                if (result & 0x80) != 0 {
                    self.f |= FLAG_S;
                }
                if value > self.a {
                    self.f |= FLAG_C;
                }
                if src == 0b110 { 7 } else { 4 }
            }
            0xD9 => {
                std::mem::swap(&mut self.b, &mut self.b_alt);
                std::mem::swap(&mut self.c, &mut self.c_alt);
                std::mem::swap(&mut self.d, &mut self.d_alt);
                std::mem::swap(&mut self.e, &mut self.e_alt);
                std::mem::swap(&mut self.h, &mut self.h_alt);
                std::mem::swap(&mut self.l, &mut self.l_alt);
                4
            }
            0x08 => {
                std::mem::swap(&mut self.a, &mut self.a_alt);
                std::mem::swap(&mut self.f, &mut self.f_alt);
                4
            }
            0x10 => {
                let disp = self.fetch_u8(bus) as i8;
                self.b = self.b.wrapping_sub(1);
                if self.b != 0 {
                    self.pc = self.pc.wrapping_add_signed(disp as i16);
                    13
                } else {
                    8
                }
            }
            0x1F => {
                let carry_in = if self.flag_c() { 1u8 } else { 0 };
                let carry_out = (self.a & 0x01) != 0;
                self.a = (self.a >> 1) | (carry_in << 7);
                let mut flags = self.f & (FLAG_S | FLAG_Z);
                if carry_out {
                    flags |= FLAG_C;
                }
                self.f = flags;
                4
            }
            0x07 => {
                let carry_out = (self.a & 0x80) != 0;
                self.a = self.a.rotate_left(1);
                let mut flags = self.f & (FLAG_S | FLAG_Z);
                if carry_out {
                    flags |= FLAG_C;
                }
                self.f = flags;
                4
            }
            0x0F => {
                let carry_out = (self.a & 0x01) != 0;
                self.a = self.a.rotate_right(1);
                let mut flags = self.f & (FLAG_S | FLAG_Z);
                if carry_out {
                    flags |= FLAG_C;
                }
                self.f = flags;
                4
            }
            0xFE => {
                let value = self.fetch_u8(bus);
                let result = self.a.wrapping_sub(value);
                self.f = 0;
                if result == 0 {
                    self.f |= FLAG_Z;
                }
                if (result & 0x80) != 0 {
                    self.f |= FLAG_S;
                }
                if value > self.a {
                    self.f |= FLAG_C;
                }
                7
            }
            0xC6 => {
                let value = self.fetch_u8(bus);
                self.add_a(value);
                7
            }
            0xCE => {
                let value = self.fetch_u8(bus);
                self.adc_a(value);
                7
            }
            0x18 => {
                let disp = self.fetch_u8(bus) as i8;
                self.pc = self.pc.wrapping_add_signed(disp as i16);
                12
            }
            0x20 => {
                let disp = self.fetch_u8(bus) as i8;
                if !self.flag_z() {
                    self.pc = self.pc.wrapping_add_signed(disp as i16);
                    12
                } else {
                    7
                }
            }
            0x38 => {
                let disp = self.fetch_u8(bus) as i8;
                if self.flag_c() {
                    self.pc = self.pc.wrapping_add_signed(disp as i16);
                    12
                } else {
                    7
                }
            }
            0x30 => {
                let disp = self.fetch_u8(bus) as i8;
                if !self.flag_c() {
                    self.pc = self.pc.wrapping_add_signed(disp as i16);
                    12
                } else {
                    7
                }
            }
            0x28 => {
                let disp = self.fetch_u8(bus) as i8;
                if self.flag_z() {
                    self.pc = self.pc.wrapping_add_signed(disp as i16);
                    12
                } else {
                    7
                }
            }
            0xC3 => {
                self.pc = self.fetch_u16(bus);
                10
            }
            0xC2 => {
                let addr = self.fetch_u16(bus);
                if !self.flag_z() {
                    self.pc = addr;
                }
                10
            }
            0xD2 => {
                let addr = self.fetch_u16(bus);
                if !self.flag_c() {
                    self.pc = addr;
                }
                10
            }
            0xCA => {
                let addr = self.fetch_u16(bus);
                if self.flag_z() {
                    self.pc = addr;
                }
                10
            }
            0xDA => {
                let addr = self.fetch_u16(bus);
                if self.flag_c() {
                    self.pc = addr;
                }
                10
            }
            0xFA => {
                let addr = self.fetch_u16(bus);
                if self.flag_s() {
                    self.pc = addr;
                }
                10
            }
            0xF2 => {
                let addr = self.fetch_u16(bus);
                if !self.flag_s() {
                    self.pc = addr;
                }
                10
            }
            0xCD => {
                let addr = self.fetch_u16(bus);
                self.push_u16(self.pc, bus);
                self.pc = addr;
                17
            }
            0xC4 => {
                let addr = self.fetch_u16(bus);
                if !self.flag_z() {
                    self.push_u16(self.pc, bus);
                    self.pc = addr;
                    17
                } else {
                    10
                }
            }
            0xCC => {
                let addr = self.fetch_u16(bus);
                if self.flag_z() {
                    self.push_u16(self.pc, bus);
                    self.pc = addr;
                    17
                } else {
                    10
                }
            }
            0xFC => {
                let addr = self.fetch_u16(bus);
                if self.flag_s() {
                    self.push_u16(self.pc, bus);
                    self.pc = addr;
                    17
                } else {
                    10
                }
            }
            0xC0 => {
                if !self.flag_z() {
                    self.pc = self.pop_u16(bus);
                    11
                } else {
                    5
                }
            }
            0xC8 => {
                if self.flag_z() {
                    self.pc = self.pop_u16(bus);
                    11
                } else {
                    5
                }
            }
            0xD0 => {
                if !self.flag_c() {
                    self.pc = self.pop_u16(bus);
                    11
                } else {
                    5
                }
            }
            0xF8 => {
                if self.flag_s() {
                    self.pc = self.pop_u16(bus);
                    11
                } else {
                    5
                }
            }
            0xF0 => {
                if !self.flag_s() {
                    self.pc = self.pop_u16(bus);
                    11
                } else {
                    5
                }
            }
            0xC9 => {
                self.pc = self.pop_u16(bus);
                10
            }
            0xC5 => {
                self.push_u16(self.bc(), bus);
                11
            }
            0xD5 => {
                self.push_u16(self.de(), bus);
                11
            }
            0xE3 => {
                let lo = self.read_byte(self.sp, bus);
                let hi = self.read_byte(self.sp.wrapping_add(1), bus);
                let stack_hl = u16::from_le_bytes([lo, hi]);
                let old_hl = self.hl();
                let [old_lo, old_hi] = old_hl.to_le_bytes();
                self.write_byte(self.sp, old_lo, bus);
                self.write_byte(self.sp.wrapping_add(1), old_hi, bus);
                self.set_hl(stack_hl);
                19
            }
            0xE5 => {
                self.push_u16(self.hl(), bus);
                11
            }
            0xF5 => {
                let af = u16::from_le_bytes([self.f, self.a]);
                self.push_u16(af, bus);
                11
            }
            0xC1 => {
                let value = self.pop_u16(bus);
                self.set_bc(value);
                10
            }
            0xD1 => {
                let value = self.pop_u16(bus);
                self.set_de(value);
                10
            }
            0xE1 => {
                let value = self.pop_u16(bus);
                self.set_hl(value);
                10
            }
            0xF1 => {
                let value = self.pop_u16(bus);
                let [f, a] = value.to_le_bytes();
                self.a = a;
                self.f = f & (FLAG_S | FLAG_Z | FLAG_C);
                10
            }
            0xE9 => {
                self.pc = self.hl();
                4
            }
            0xEB => {
                let de = self.de();
                self.set_de(self.hl());
                self.set_hl(de);
                4
            }
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
                self.push_u16(self.pc, bus);
                self.pc = (opcode as u16) & 0x0038;
                11
            }
            0xE6 => {
                let value = self.fetch_u8(bus);
                self.a &= value;
                self.update_sz_clear_c(self.a);
                7
            }
            0xF6 => {
                let value = self.fetch_u8(bus);
                self.a |= value;
                self.update_sz_clear_c(self.a);
                7
            }
            0xEE => {
                let value = self.fetch_u8(bus);
                self.a ^= value;
                self.update_sz_clear_c(self.a);
                7
            }
            0xD6 => {
                let value = self.fetch_u8(bus);
                self.sub_a(value);
                7
            }
            0xF3 => {
                self.iff1 = false;
                self.iff2 = false;
                4
            }
            0xFB => {
                self.iff1 = true;
                self.iff2 = true;
                4
            }
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
                let reg = (opcode >> 3) & 0x7;
                let value = self.read_reg_code(reg, bus).wrapping_add(1);
                self.write_reg_code(reg, value, bus);
                self.update_sz_preserve_c(value);
                if reg == 0b110 { 11 } else { 4 }
            }
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => {
                let reg = (opcode >> 3) & 0x7;
                let value = self.read_reg_code(reg, bus).wrapping_sub(1);
                self.write_reg_code(reg, value, bus);
                self.update_sz_preserve_c(value);
                if reg == 0b110 { 11 } else { 4 }
            }
            0x40..=0x7F => {
                // 0x76 (HALT) is handled above.
                let dst = (opcode >> 3) & 0x7;
                let src = opcode & 0x7;
                let value = self.read_reg_code(src, bus);
                self.write_reg_code(dst, value, bus);
                if dst == 0b110 || src == 0b110 { 7 } else { 4 }
            }
            _ => {
                self.record_unknown(opcode, opcode_pc);
                4
            }
        }
    }

    fn exec_cb(&mut self, opcode: u8, bus: &mut Z80Bus<'_>) -> u8 {
        let x = opcode >> 6;
        let y = (opcode >> 3) & 0x7;
        let z = opcode & 0x7;
        let value = self.read_reg_code(z, bus);
        let (result, write_back, cycles) = self.apply_cb_to_value(x, y, value);
        if write_back {
            self.write_reg_code(z, result, bus);
        }
        if z == 0b110 {
            if x == 0 { 15 } else { 12 }
        } else {
            cycles
        }
    }

    fn exec_ed(&mut self, opcode_pc: u16, opcode: u8, bus: &mut Z80Bus<'_>) -> u8 {
        match opcode {
            0x53 => {
                let addr = self.fetch_u16(bus);
                let [lo, hi] = self.de().to_le_bytes();
                self.write_byte(addr, lo, bus);
                self.write_byte(addr.wrapping_add(1), hi, bus);
                20
            }
            0x5B => {
                let addr = self.fetch_u16(bus);
                let lo = self.read_byte(addr, bus);
                let hi = self.read_byte(addr.wrapping_add(1), bus);
                self.set_de(u16::from_le_bytes([lo, hi]));
                20
            }
            0x47 | 0x4F => 9, // LD I,A / LD R,A (not modeled)
            0x57 | 0x5F => {
                // LD A,I / LD A,R (use a coarse refresh approximation).
                let carry = self.f & FLAG_C;
                self.a = (self.cycles as u8).wrapping_add(self.pc as u8);
                let mut flags = carry;
                if self.a == 0 {
                    flags |= FLAG_Z;
                }
                if (self.a & 0x80) != 0 {
                    flags |= FLAG_S;
                }
                self.f = flags;
                9
            }
            0xA0 => {
                // LDI
                let value = self.read_byte(self.hl(), bus);
                self.write_byte(self.de(), value, bus);
                self.set_hl(self.hl().wrapping_add(1));
                self.set_de(self.de().wrapping_add(1));
                self.set_bc(self.bc().wrapping_sub(1));
                16
            }
            0x45 | 0x4D => {
                self.pc = self.pop_u16(bus);
                self.iff1 = self.iff2;
                14
            }
            0xB0 => {
                let value = self.read_byte(self.hl(), bus);
                self.write_byte(self.de(), value, bus);
                self.set_hl(self.hl().wrapping_add(1));
                self.set_de(self.de().wrapping_add(1));
                self.set_bc(self.bc().wrapping_sub(1));
                if self.bc() != 0 {
                    self.pc = self.pc.wrapping_sub(2);
                    21
                } else {
                    16
                }
            }
            0x46 | 0x56 | 0x5E | 0x66 | 0x76 | 0x7E => 8, // IM 0/1/2
            _ => {
                self.record_unknown(0xED, opcode_pc);
                8
            }
        }
    }

    fn exec_index_prefix(&mut self, opcode_pc: u16, use_ix: bool, bus: &mut Z80Bus<'_>) -> u8 {
        let op2 = self.fetch_u8(bus);
        match op2 {
            0x09 => {
                self.set_index_reg(use_ix, self.index_reg(use_ix).wrapping_add(self.bc()));
                15
            }
            0x19 => {
                self.set_index_reg(use_ix, self.index_reg(use_ix).wrapping_add(self.de()));
                15
            }
            0x29 => {
                let idx = self.index_reg(use_ix);
                self.set_index_reg(use_ix, idx.wrapping_add(idx));
                15
            }
            0x39 => {
                self.set_index_reg(use_ix, self.index_reg(use_ix).wrapping_add(self.sp));
                15
            }
            0x21 => {
                let value = self.fetch_u16(bus);
                self.set_index_reg(use_ix, value);
                14
            }
            0x22 => {
                let addr = self.fetch_u16(bus);
                let value = self.index_reg(use_ix);
                let [lo, hi] = value.to_le_bytes();
                self.write_byte(addr, lo, bus);
                self.write_byte(addr.wrapping_add(1), hi, bus);
                20
            }
            0x2A => {
                let addr = self.fetch_u16(bus);
                let lo = self.read_byte(addr, bus);
                let hi = self.read_byte(addr.wrapping_add(1), bus);
                self.set_index_reg(use_ix, u16::from_le_bytes([lo, hi]));
                20
            }
            0x23 => {
                self.set_index_reg(use_ix, self.index_reg(use_ix).wrapping_add(1));
                10
            }
            0x2B => {
                self.set_index_reg(use_ix, self.index_reg(use_ix).wrapping_sub(1));
                10
            }
            0x34 => {
                let disp = self.fetch_u8(bus) as i8;
                let addr = self.indexed_addr(use_ix, disp);
                let value = self.read_byte(addr, bus).wrapping_add(1);
                self.write_byte(addr, value, bus);
                self.update_sz_preserve_c(value);
                23
            }
            0x35 => {
                let disp = self.fetch_u8(bus) as i8;
                let addr = self.indexed_addr(use_ix, disp);
                let value = self.read_byte(addr, bus).wrapping_sub(1);
                self.write_byte(addr, value, bus);
                self.update_sz_preserve_c(value);
                23
            }
            0x36 => {
                let disp = self.fetch_u8(bus) as i8;
                let value = self.fetch_u8(bus);
                let addr = self.indexed_addr(use_ix, disp);
                self.write_byte(addr, value, bus);
                19
            }
            0x7E => {
                let disp = self.fetch_u8(bus) as i8;
                let addr = self.indexed_addr(use_ix, disp);
                self.a = self.read_byte(addr, bus);
                19
            }
            0x46 | 0x4E | 0x56 | 0x5E | 0x66 | 0x6E => {
                let disp = self.fetch_u8(bus) as i8;
                let addr = self.indexed_addr(use_ix, disp);
                let value = self.read_byte(addr, bus);
                self.write_reg_code_no_mem((op2 >> 3) & 0x7, value);
                19
            }
            0x70..=0x77 => {
                if op2 == 0x76 {
                    self.record_unknown(if use_ix { 0xDD } else { 0xFD }, opcode_pc);
                    return 4;
                }
                let disp = self.fetch_u8(bus) as i8;
                let addr = self.indexed_addr(use_ix, disp);
                let value = self.read_reg_code(op2 & 0x7, bus);
                self.write_byte(addr, value, bus);
                19
            }
            0x86 => {
                let disp = self.fetch_u8(bus) as i8;
                let addr = self.indexed_addr(use_ix, disp);
                let value = self.read_byte(addr, bus);
                self.add_a(value);
                19
            }
            0xAE => {
                let disp = self.fetch_u8(bus) as i8;
                let addr = self.indexed_addr(use_ix, disp);
                let value = self.read_byte(addr, bus);
                self.a ^= value;
                self.update_sz_clear_c(self.a);
                19
            }
            0xB6 => {
                let disp = self.fetch_u8(bus) as i8;
                let addr = self.indexed_addr(use_ix, disp);
                let value = self.read_byte(addr, bus);
                self.a |= value;
                self.update_sz_clear_c(self.a);
                19
            }
            0xBE => {
                let disp = self.fetch_u8(bus) as i8;
                let addr = self.indexed_addr(use_ix, disp);
                let value = self.read_byte(addr, bus);
                let result = self.a.wrapping_sub(value);
                self.f = 0;
                if result == 0 {
                    self.f |= FLAG_Z;
                }
                if (result & 0x80) != 0 {
                    self.f |= FLAG_S;
                }
                if value > self.a {
                    self.f |= FLAG_C;
                }
                19
            }
            0xE5 => {
                self.push_u16(self.index_reg(use_ix), bus);
                15
            }
            0xE1 => {
                let value = self.pop_u16(bus);
                self.set_index_reg(use_ix, value);
                14
            }
            0xF9 => {
                self.sp = self.index_reg(use_ix);
                10
            }
            0xCB => {
                let disp = self.fetch_u8(bus) as i8;
                let op3 = self.fetch_u8(bus);
                self.exec_index_cb(use_ix, disp, op3, bus)
            }
            _ => {
                self.record_unknown(if use_ix { 0xDD } else { 0xFD }, opcode_pc);
                4
            }
        }
    }

    fn exec_index_cb(&mut self, use_ix: bool, disp: i8, opcode: u8, bus: &mut Z80Bus<'_>) -> u8 {
        let x = opcode >> 6;
        let y = (opcode >> 3) & 0x7;
        let z = opcode & 0x7;
        let addr = self.indexed_addr(use_ix, disp);
        let value = self.read_byte(addr, bus);
        let (result, write_back, _cycles) = self.apply_cb_to_value(x, y, value);
        if write_back {
            self.write_byte(addr, result, bus);
            if z != 0b110 {
                self.write_reg_code_no_mem(z, result);
            }
        }
        23
    }

    fn apply_cb_to_value(&mut self, x: u8, y: u8, value: u8) -> (u8, bool, u8) {
        match x {
            0 => {
                let (result, carry) = match y {
                    0 => (value.rotate_left(1), (value & 0x80) != 0), // RLC
                    1 => (value.rotate_right(1), (value & 0x01) != 0), // RRC
                    2 => {
                        let c = (self.f & FLAG_C) != 0;
                        let result = (value << 1) | (c as u8);
                        (result, (value & 0x80) != 0) // RL
                    }
                    3 => {
                        let c = (self.f & FLAG_C) != 0;
                        let result = (value >> 1) | ((c as u8) << 7);
                        (result, (value & 0x01) != 0) // RR
                    }
                    4 => (value << 1, (value & 0x80) != 0), // SLA
                    5 => ((value >> 1) | (value & 0x80), (value & 0x01) != 0), // SRA
                    6 => ((value << 1) | 1, (value & 0x80) != 0), // SLL (undoc)
                    7 => (value >> 1, (value & 0x01) != 0), // SRL
                    _ => (value, false),
                };
                let mut flags = 0;
                if result == 0 {
                    flags |= FLAG_Z;
                }
                if (result & 0x80) != 0 {
                    flags |= FLAG_S;
                }
                if carry {
                    flags |= FLAG_C;
                }
                self.f = flags;
                (result, true, 8)
            }
            1 => {
                // BIT y,value
                let bit_set = (value & (1 << y)) != 0;
                let carry = self.f & FLAG_C;
                let mut flags = carry;
                if !bit_set {
                    flags |= FLAG_Z;
                }
                if y == 7 && bit_set {
                    flags |= FLAG_S;
                }
                self.f = flags;
                (value, false, 8)
            }
            2 => (value & !(1 << y), true, 8), // RES
            3 => (value | (1 << y), true, 8),  // SET
            _ => (value, false, 8),
        }
    }

    fn read_reg_code(&self, code: u8, bus: &Z80Bus<'_>) -> u8 {
        match code & 0x7 {
            0b000 => self.b,
            0b001 => self.c,
            0b010 => self.d,
            0b011 => self.e,
            0b100 => self.h,
            0b101 => self.l,
            0b110 => self.read_byte(self.hl(), bus),
            0b111 => self.a,
            _ => 0,
        }
    }

    fn write_reg_code(&mut self, code: u8, value: u8, bus: &mut Z80Bus<'_>) {
        match code & 0x7 {
            0b000 => self.b = value,
            0b001 => self.c = value,
            0b010 => self.d = value,
            0b011 => self.e = value,
            0b100 => self.h = value,
            0b101 => self.l = value,
            0b110 => {
                let addr = self.hl();
                self.write_byte(addr, value, bus);
            }
            0b111 => self.a = value,
            _ => {}
        }
    }

    fn write_reg_code_no_mem(&mut self, code: u8, value: u8) {
        match code & 0x7 {
            0b000 => self.b = value,
            0b001 => self.c = value,
            0b010 => self.d = value,
            0b011 => self.e = value,
            0b100 => self.h = value,
            0b101 => self.l = value,
            0b111 => self.a = value,
            _ => {}
        }
    }

    fn fetch_u8(&mut self, bus: &Z80Bus<'_>) -> u8 {
        let value = self.read_byte(self.pc, bus);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    fn fetch_u16(&mut self, bus: &Z80Bus<'_>) -> u16 {
        let lo = self.fetch_u8(bus);
        let hi = self.fetch_u8(bus);
        u16::from_le_bytes([lo, hi])
    }

    fn push_u16(&mut self, value: u16, bus: &mut Z80Bus<'_>) {
        let [lo, hi] = value.to_le_bytes();
        self.sp = self.sp.wrapping_sub(1);
        self.write_byte(self.sp, hi, bus);
        self.sp = self.sp.wrapping_sub(1);
        self.write_byte(self.sp, lo, bus);
    }

    fn pop_u16(&mut self, bus: &Z80Bus<'_>) -> u16 {
        let lo = self.read_byte(self.sp, bus);
        self.sp = self.sp.wrapping_add(1);
        let hi = self.read_byte(self.sp, bus);
        self.sp = self.sp.wrapping_add(1);
        u16::from_le_bytes([lo, hi])
    }

    fn read_byte(&self, addr: u16, bus: &Z80Bus<'_>) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.ram[(addr as usize) & 0x1FFF],
            0x4000..=0x5FFF => bus.audio.read_ym2612((addr & 0x03) as u8),
            0x8000..=0xFFFF => self.read_68k_window(addr, bus),
            _ => 0xFF,
        }
    }

    fn write_byte(&mut self, addr: u16, value: u8, bus: &mut Z80Bus<'_>) {
        match addr {
            0x0000..=0x3FFF => {
                self.ram[(addr as usize) & 0x1FFF] = value;
            }
            0x4000..=0x5FFF => bus.audio.write_ym2612((addr & 0x03) as u8, value),
            0x6000..=0x60FF => self.write_bank_register(value),
            0x7F11 => bus.audio.write_psg(value),
            0x8000..=0xFFFF => self.write_68k_window(addr, value, bus),
            _ => {}
        }
    }

    fn write_bank_register(&mut self, value: u8) {
        // Genesis Z80 bank register is a serial latch fed by bit0 writes.
        self.bank_address = (self.bank_address >> 1) | (((value as u32) & 1) << 23);
        self.bank_address &= 0x00FF_8000;
    }

    fn resolve_68k_window_addr(&self, z80_addr: u16) -> u32 {
        let offset = (z80_addr as u32).wrapping_sub(0x8000) & 0x7FFF;
        (self.bank_address & 0x00FF_8000) | offset
    }

    fn read_68k_window(&self, z80_addr: u16, bus: &Z80Bus<'_>) -> u8 {
        let addr = self.resolve_68k_window_addr(z80_addr);
        match addr {
            0x000000..=0x3FFFFF => bus.cartridge.read_u8(addr),
            0xA04000..=0xA04003 => bus.audio.read_ym2612((addr - 0xA04000) as u8),
            x if x == IO_VERSION_ADDR || x == IO_VERSION_ADDR + 1 => bus.io.read_version(),
            x if x == IO_PORT1_DATA_ADDR || x == IO_PORT1_DATA_ADDR + 1 => bus.io.read_port1_data(),
            x if x == IO_PORT2_DATA_ADDR || x == IO_PORT2_DATA_ADDR + 1 => bus.io.read_port2_data(),
            x if x == IO_PORT1_CTRL_ADDR || x == IO_PORT1_CTRL_ADDR + 1 => bus.io.read_port1_ctrl(),
            x if x == IO_PORT2_CTRL_ADDR || x == IO_PORT2_CTRL_ADDR + 1 => bus.io.read_port2_ctrl(),
            0xFF0000..=0xFFFFFF => bus.work_ram[(addr - 0xFF0000) as usize],
            _ => 0xFF,
        }
    }

    fn write_68k_window(&mut self, z80_addr: u16, value: u8, bus: &mut Z80Bus<'_>) {
        let addr = self.resolve_68k_window_addr(z80_addr);
        match addr {
            0xA04000..=0xA04003 => bus.audio.write_ym2612((addr - 0xA04000) as u8, value),
            x if x == IO_PORT1_DATA_ADDR || x == IO_PORT1_DATA_ADDR + 1 => {
                bus.io.write_port1_data(value)
            }
            x if x == IO_PORT2_DATA_ADDR || x == IO_PORT2_DATA_ADDR + 1 => {
                bus.io.write_port2_data(value)
            }
            x if x == IO_PORT1_CTRL_ADDR || x == IO_PORT1_CTRL_ADDR + 1 => {
                bus.io.write_port1_ctrl(value)
            }
            x if x == IO_PORT2_CTRL_ADDR || x == IO_PORT2_CTRL_ADDR + 1 => {
                bus.io.write_port2_ctrl(value)
            }
            0xC00011 => bus.audio.write_psg(value),
            0xC00000..=0xC0001F => self.write_vdp_port_byte(addr, value, bus),
            0xFF0000..=0xFFFFFF => {
                bus.work_ram[(addr - 0xFF0000) as usize] = value;
            }
            _ => {}
        }
    }

    fn write_vdp_port_byte(&mut self, addr: u32, value: u8, bus: &mut Z80Bus<'_>) {
        let aligned = addr & !1;
        let next = match aligned {
            0xC00000 | 0xC00002 => {
                let current = self.vdp_data_write_latch;
                let next = if (addr & 1) == 0 {
                    ((value as u16) << 8) | (current & 0x00FF)
                } else {
                    (current & 0xFF00) | value as u16
                };
                self.vdp_data_write_latch = next;
                next
            }
            0xC00004 | 0xC00006 => {
                let current = self.vdp_control_write_latch;
                let next = if (addr & 1) == 0 {
                    ((value as u16) << 8) | (current & 0x00FF)
                } else {
                    (current & 0xFF00) | value as u16
                };
                self.vdp_control_write_latch = next;
                next
            }
            _ => return,
        };
        match aligned {
            0xC00000 | 0xC00002 => bus.vdp.write_data_port(next),
            0xC00004 | 0xC00006 => bus.vdp.write_control_port(next),
            _ => {}
        }
    }

    fn hl(&self) -> u16 {
        ((self.h as u16) << 8) | self.l as u16
    }

    fn bc(&self) -> u16 {
        ((self.b as u16) << 8) | self.c as u16
    }

    fn de(&self) -> u16 {
        ((self.d as u16) << 8) | self.e as u16
    }

    fn index_reg(&self, use_ix: bool) -> u16 {
        if use_ix { self.ix } else { self.iy }
    }

    fn set_index_reg(&mut self, use_ix: bool, value: u16) {
        if use_ix {
            self.ix = value;
        } else {
            self.iy = value;
        }
    }

    fn indexed_addr(&self, use_ix: bool, disp: i8) -> u16 {
        self.index_reg(use_ix).wrapping_add_signed(disp as i16)
    }

    fn set_hl(&mut self, value: u16) {
        self.h = (value >> 8) as u8;
        self.l = value as u8;
    }

    fn set_bc(&mut self, value: u16) {
        self.b = (value >> 8) as u8;
        self.c = value as u8;
    }

    fn set_de(&mut self, value: u16) {
        self.d = (value >> 8) as u8;
        self.e = value as u8;
    }

    fn flag_z(&self) -> bool {
        (self.f & FLAG_Z) != 0
    }

    fn flag_c(&self) -> bool {
        (self.f & FLAG_C) != 0
    }

    fn flag_s(&self) -> bool {
        (self.f & FLAG_S) != 0
    }

    fn update_sz_preserve_c(&mut self, value: u8) {
        let carry = self.f & FLAG_C;
        let mut next = carry;
        if value == 0 {
            next |= FLAG_Z;
        }
        if (value & 0x80) != 0 {
            next |= FLAG_S;
        }
        self.f = next;
    }

    fn update_sz_clear_c(&mut self, value: u8) {
        let mut next = 0;
        if value == 0 {
            next |= FLAG_Z;
        }
        if (value & 0x80) != 0 {
            next |= FLAG_S;
        }
        self.f = next;
    }

    fn add_a(&mut self, value: u8) {
        let (result, carry) = self.a.overflowing_add(value);
        self.a = result;
        let mut flags = 0;
        if result == 0 {
            flags |= FLAG_Z;
        }
        if (result & 0x80) != 0 {
            flags |= FLAG_S;
        }
        if carry {
            flags |= FLAG_C;
        }
        self.f = flags;
    }

    fn adc_a(&mut self, value: u8) {
        let carry_in = if self.flag_c() { 1u16 } else { 0 };
        let sum = self.a as u16 + value as u16 + carry_in;
        let result = sum as u8;
        self.a = result;
        let mut flags = 0;
        if result == 0 {
            flags |= FLAG_Z;
        }
        if (result & 0x80) != 0 {
            flags |= FLAG_S;
        }
        if sum > 0xFF {
            flags |= FLAG_C;
        }
        self.f = flags;
    }

    fn sub_a(&mut self, value: u8) {
        let (result, borrow) = self.a.overflowing_sub(value);
        self.a = result;
        let mut flags = 0;
        if result == 0 {
            flags |= FLAG_Z;
        }
        if (result & 0x80) != 0 {
            flags |= FLAG_S;
        }
        if borrow {
            flags |= FLAG_C;
        }
        self.f = flags;
    }

    fn sbc_a(&mut self, value: u8) {
        let carry_in = if self.flag_c() { 1u16 } else { 0 };
        let lhs = self.a as u16;
        let rhs = value as u16 + carry_in;
        let result16 = lhs.wrapping_sub(rhs);
        let result = result16 as u8;
        self.a = result;
        let mut flags = 0;
        if result == 0 {
            flags |= FLAG_Z;
        }
        if (result & 0x80) != 0 {
            flags |= FLAG_S;
        }
        if rhs > lhs {
            flags |= FLAG_C;
        }
        self.f = flags;
    }

    fn record_unknown(&mut self, opcode: u8, pc: u16) {
        self.unknown_opcode_total = self.unknown_opcode_total.saturating_add(1);
        *self.unknown_opcode_histogram.entry(opcode).or_insert(0) += 1;
        *self.unknown_opcode_pc_histogram.entry(pc).or_insert(0) += 1;
    }

    fn add_hl(&mut self, value: u16) {
        let hl = self.hl();
        let (result, carry) = hl.overflowing_add(value);
        self.set_hl(result);
        let mut flags = self.f & (FLAG_S | FLAG_Z);
        if carry {
            flags |= FLAG_C;
        }
        self.f = flags;
    }
}

#[cfg(test)]
mod tests {
    use super::Z80;
    use crate::audio::AudioBus;
    use crate::cartridge::Cartridge;
    use crate::input::IoBus;
    use crate::vdp::Vdp;

    fn dummy_cart() -> Cartridge {
        Cartridge::from_bytes(vec![0; 0x200]).expect("valid cart")
    }

    #[test]
    fn bus_request_register_controls_halt_state() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        assert_eq!(z80.read_busreq_byte(), 0x01);

        z80.write_busreq_byte(0x01);
        assert_eq!(z80.read_busreq_byte(), 0x01);
        z80.step(16, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.read_busreq_byte(), 0x00);
        z80.write_busreq_byte(0x00);
        assert_eq!(z80.read_busreq_byte(), 0x01);
    }

    #[test]
    fn reset_register_controls_run_state_and_cycles() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.step(100, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.cycles(), 0);

        z80.write_reset_byte(0x01); // release reset
        z80.step(100, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.cycles(), 50);

        z80.write_busreq_byte(0x01); // bus requested -> grant pending, still running
        z80.step(8, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.cycles(), 54);

        z80.step(8, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io); // grant reached at the end of this slice
        assert_eq!(z80.cycles(), 58);

        z80.step(100, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io); // bus granted -> halt
        assert_eq!(z80.cycles(), 58);
    }

    #[test]
    fn m68k_ram_access_becomes_available_after_bus_grant_delay() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_busreq_byte(0x01);
        assert!(!z80.m68k_can_access_ram());

        z80.step(8, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert!(!z80.m68k_can_access_ram());

        z80.step(100, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert!(z80.m68k_can_access_ram());
    }

    #[test]
    fn z80_ram_is_8kb_and_mirrored() {
        let mut z80 = Z80::new();
        z80.write_ram_u8(0x0001, 0x12);
        z80.write_ram_u8(0x2001, 0x34); // mirror of 0x0001

        assert_eq!(z80.read_ram_u8(0x0001), 0x34);
        assert_eq!(z80.read_ram_u8(0x2001), 0x34);
    }

    #[test]
    fn executes_program_that_writes_ym2612_register() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);

        // ld a,0x22 ; ld (0x4000),a ; ld a,0x0F ; ld (0x4001),a ; halt
        let program = [
            0x3E, 0x22, 0x32, 0x00, 0x40, 0x3E, 0x0F, 0x32, 0x01, 0x40, 0x76,
        ];
        for (i, byte) in program.iter().enumerate() {
            z80.write_ram_u8(i as u16, *byte);
        }

        z80.step(400, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(audio.ym2612().register(0, 0x22), 0x0F);
    }

    #[test]
    fn executes_program_that_writes_psg() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);

        // ld a,0x9F ; ld (0x7F11),a ; halt
        let program = [0x3E, 0x9F, 0x32, 0x11, 0x7F, 0x76];
        for (i, byte) in program.iter().enumerate() {
            z80.write_ram_u8(i as u16, *byte);
        }

        z80.step(200, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(audio.psg().last_data(), 0x9F);
    }

    #[test]
    fn unknown_opcode_counter_increments_for_unimplemented_prefix() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);
        z80.write_ram_u8(0x0000, 0xED);
        z80.write_ram_u8(0x0001, 0xFF);

        z80.step(32, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.unknown_opcode_total(), 1);
    }

    #[test]
    fn or_a_updates_flags_and_is_not_unknown() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);
        // xor a ; or a ; halt
        z80.write_ram_u8(0x0000, 0xAF);
        z80.write_ram_u8(0x0001, 0xB7);
        z80.write_ram_u8(0x0002, 0x76);

        z80.step(64, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.unknown_opcode_total(), 0);
        assert_eq!(z80.a, 0);
        assert_ne!(z80.f & super::FLAG_Z, 0);
        assert_eq!(z80.f & super::FLAG_C, 0);
    }

    #[test]
    fn djnz_and_ret_nz_execute_control_flow() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);

        // ld b,3
        z80.write_ram_u8(0x0000, 0x06);
        z80.write_ram_u8(0x0001, 0x03);
        // ld a,0
        z80.write_ram_u8(0x0002, 0x3E);
        z80.write_ram_u8(0x0003, 0x00);
        // add a,1
        z80.write_ram_u8(0x0004, 0xC6);
        z80.write_ram_u8(0x0005, 0x01);
        // djnz -4 (to add a,1)
        z80.write_ram_u8(0x0006, 0x10);
        z80.write_ram_u8(0x0007, 0xFC);
        // call 0x0010
        z80.write_ram_u8(0x0008, 0xCD);
        z80.write_ram_u8(0x0009, 0x10);
        z80.write_ram_u8(0x000A, 0x00);
        // halt
        z80.write_ram_u8(0x000B, 0x76);
        // subroutine @0x0010: or a ; ret nz
        z80.write_ram_u8(0x0010, 0xB7);
        z80.write_ram_u8(0x0011, 0xC0);
        // halt (should not reach)
        z80.write_ram_u8(0x0012, 0x76);

        z80.step(512, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.unknown_opcode_total(), 0);
        assert_eq!(z80.a, 3);
        assert_eq!(z80.pc, 0x000C);
    }

    #[test]
    fn pop_af_restores_accumulator_and_flags() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);

        // Seed stack with AF value 0xAA41 and execute POP AF.
        z80.sp = 0x0100;
        z80.write_ram_u8(0x0100, 0x41);
        z80.write_ram_u8(0x0101, 0xAA);
        z80.write_ram_u8(0x0000, 0xF1);
        z80.write_ram_u8(0x0001, 0x76);

        z80.step(64, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.unknown_opcode_total(), 0);
        assert_eq!(z80.a, 0xAA);
        assert_eq!(
            z80.f & (super::FLAG_S | super::FLAG_Z | super::FLAG_C),
            0x41
        );
        assert_eq!(z80.sp, 0x0102);
    }

    #[test]
    fn push_pop_bc_and_conditional_call_nz() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);

        // ld bc,0x1234 ; push bc ; ld bc,0 ; pop bc ; call nz,0x0010 ; halt
        // 0x0010: and 0x0F ; sub 0x01 ; ret
        let program = [
            0x01, 0x34, 0x12, 0xC5, 0x01, 0x00, 0x00, 0xC1, 0xC4, 0x10, 0x00, 0x76, 0x00, 0x00,
            0x00, 0x00, 0xE6, 0x0F, 0xD6, 0x01, 0xC9,
        ];
        for (i, byte) in program.iter().enumerate() {
            z80.write_ram_u8(i as u16, *byte);
        }
        z80.a = 0x3C;
        z80.f = 0; // NZ true

        z80.step(512, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.unknown_opcode_total(), 0);
        assert_eq!(z80.bc(), 0x1234);
        assert_eq!(z80.a, 0x0B);
    }

    #[test]
    fn bank_window_reads_from_68k_rom_space() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let mut rom = vec![0u8; 0x200];
        rom[0x0000] = 0xAB;
        let cart = Cartridge::from_bytes(rom).expect("valid cart");
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);

        // ld a,(0x8000) ; halt
        z80.write_ram_u8(0x0000, 0x3A);
        z80.write_ram_u8(0x0001, 0x00);
        z80.write_ram_u8(0x0002, 0x80);
        z80.write_ram_u8(0x0003, 0x76);

        z80.step(128, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.a, 0xAB);
    }

    #[test]
    fn bank_window_writes_to_68k_work_ram_space() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);
        z80.bank_address = 0x00FF_0000;

        // ld a,0x5A ; ld (0x8000),a ; halt
        z80.write_ram_u8(0x0000, 0x3E);
        z80.write_ram_u8(0x0001, 0x5A);
        z80.write_ram_u8(0x0002, 0x32);
        z80.write_ram_u8(0x0003, 0x00);
        z80.write_ram_u8(0x0004, 0x80);
        z80.write_ram_u8(0x0005, 0x76);

        z80.step(160, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(work_ram[0], 0x5A);
    }

    #[test]
    fn bank_window_reads_io_version_register() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);
        z80.bank_address = 0x00A1_0000;

        // ld a,(0x8000) ; halt
        z80.write_ram_u8(0x0000, 0x3A);
        z80.write_ram_u8(0x0001, 0x00);
        z80.write_ram_u8(0x0002, 0x80);
        z80.write_ram_u8(0x0003, 0x76);

        z80.step(128, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.a, 0x20);
    }

    #[test]
    fn bank_window_can_write_psg_through_68k_bus_address() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);
        z80.bank_address = 0x00C0_0000;

        // ld a,0x9A ; ld (0x8011),a ; halt
        z80.write_ram_u8(0x0000, 0x3E);
        z80.write_ram_u8(0x0001, 0x9A);
        z80.write_ram_u8(0x0002, 0x32);
        z80.write_ram_u8(0x0003, 0x11);
        z80.write_ram_u8(0x0004, 0x80);
        z80.write_ram_u8(0x0005, 0x76);

        z80.step(160, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(audio.psg().last_data(), 0x9A);
    }

    #[test]
    fn bank_register_uses_serial_bit_latch() {
        let mut z80 = Z80::new();
        for _ in 0..8 {
            z80.write_bank_register(1);
        }
        assert_eq!(z80.bank_address, 0x00FF_0000);
    }

    #[test]
    fn interrupt_requests_vector_to_0038_and_reti_returns() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);

        // ei ; halt
        z80.write_ram_u8(0x0000, 0xFB);
        z80.write_ram_u8(0x0001, 0x76);
        z80.write_ram_u8(0x0002, 0x76);
        // IRQ vector @0x0038: ld a,0x42 ; reti
        z80.write_ram_u8(0x0038, 0x3E);
        z80.write_ram_u8(0x0039, 0x42);
        z80.write_ram_u8(0x003A, 0xED);
        z80.write_ram_u8(0x003B, 0x4D);

        z80.step(64, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.pc, 0x0002);

        z80.request_interrupt();
        z80.step(256, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);

        assert_eq!(z80.unknown_opcode_total(), 0);
        assert_eq!(z80.a, 0x42);
        assert_eq!(z80.pc, 0x0003);
    }

    #[test]
    fn inc_de_opcode_updates_register_pair() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);

        // ld de,0x00FF ; inc de ; halt
        z80.write_ram_u8(0x0000, 0x11);
        z80.write_ram_u8(0x0001, 0xFF);
        z80.write_ram_u8(0x0002, 0x00);
        z80.write_ram_u8(0x0003, 0x13);
        z80.write_ram_u8(0x0004, 0x76);

        z80.step(128, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.de(), 0x0100);
    }

    #[test]
    fn ldi_copies_byte_and_updates_pairs() {
        let mut z80 = Z80::new();
        let mut audio = AudioBus::new();
        let cart = dummy_cart();
        let mut work_ram = [0u8; 0x10000];
        let mut vdp = Vdp::new();
        let mut io = IoBus::new();
        z80.write_reset_byte(0x01);

        z80.set_hl(0x0100);
        z80.set_de(0x0200);
        z80.set_bc(0x0001);
        z80.write_ram_u8(0x0100, 0x5A);
        z80.write_ram_u8(0x0000, 0xED);
        z80.write_ram_u8(0x0001, 0xA0);
        z80.write_ram_u8(0x0002, 0x76);

        z80.step(128, &mut audio, &cart, &mut work_ram, &mut vdp, &mut io);
        assert_eq!(z80.read_ram_u8(0x0200), 0x5A);
        assert_eq!(z80.hl(), 0x0101);
        assert_eq!(z80.de(), 0x0201);
        assert_eq!(z80.bc(), 0x0000);
    }
}
