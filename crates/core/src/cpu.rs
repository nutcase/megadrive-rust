use crate::memory::MemoryMap;
use std::collections::BTreeMap;

const CCR_C: u16 = 0x0001;
const CCR_V: u16 = 0x0002;
const CCR_Z: u16 = 0x0004;
const CCR_N: u16 = 0x0008;
const CCR_X: u16 = 0x0010;
const SR_INT_MASK: u16 = 0x0700;
const SR_SUPERVISOR: u16 = 0x2000;

#[derive(Debug, Clone)]
pub struct M68k {
    d_regs: [u32; 8],
    a_regs: [u32; 8],
    usp: u32,
    ssp: u32,
    sr: u16,
    pc: u32,
    cycles: u64,
    unknown_opcode_total: u64,
    unknown_opcode_histogram: BTreeMap<u16, u64>,
    unknown_opcode_pc_histogram: BTreeMap<u32, u64>,
    exception_histogram: BTreeMap<u32, u64>,
}

impl Default for M68k {
    fn default() -> Self {
        Self {
            d_regs: [0; 8],
            a_regs: [0; 8],
            usp: 0,
            ssp: 0,
            sr: 0x2700,
            pc: 0,
            cycles: 0,
            unknown_opcode_total: 0,
            unknown_opcode_histogram: BTreeMap::new(),
            unknown_opcode_pc_histogram: BTreeMap::new(),
            exception_histogram: BTreeMap::new(),
        }
    }
}

impl M68k {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self, memory: &mut MemoryMap) {
        // Initial SSP/PC are taken from vectors at 0x000000 and 0x000004.
        self.d_regs = [0; 8];
        self.a_regs = [0; 8];
        self.usp = 0;
        self.ssp = memory.read_u32(0x000000);
        self.sr = 0x2700;
        self.a_regs[7] = self.ssp;
        self.pc = memory.read_u32(0x000004);
        self.cycles = 0;
        self.unknown_opcode_total = 0;
        self.unknown_opcode_histogram.clear();
        self.unknown_opcode_pc_histogram.clear();
        self.exception_histogram.clear();
    }

    pub fn step(&mut self, memory: &mut MemoryMap) -> u32 {
        if let Some(level) = memory.pending_interrupt_level() {
            if self.service_interrupt(level, memory) {
                memory.acknowledge_interrupt(level);
                let cycles = 44;
                self.cycles += cycles as u64;
                return cycles;
            }
        }

        let opcode = self.fetch_u16(memory);
        macro_rules! opt_cycles {
            ($expr:expr) => {{
                match $expr {
                    Some(cycles) => cycles,
                    None => {
                        self.record_unknown_opcode(opcode, self.pc.wrapping_sub(2));
                        4
                    }
                }
            }};
        }

        let cycles = match opcode {
            0x4E71 => 4, // NOP
            0x4E75 => self.exec_rts(memory),
            0x4E73 => self.exec_rte(memory),
            0x4AFC => self.exec_illegal(memory),
            _ if (opcode & 0xFFF0) == 0x4E60 => self.exec_move_usp(opcode, memory),
            _ if (opcode & 0xFFF8) == 0x4E50 => self.exec_link(opcode, memory),
            _ if (opcode & 0xFFF8) == 0x4E58 => self.exec_unlk(opcode, memory),
            _ if (opcode & 0xFFC0) == 0x4E80 => opt_cycles!(self.exec_jsr(opcode, memory)),
            _ if (opcode & 0xFFC0) == 0x4EC0 => opt_cycles!(self.exec_jmp(opcode, memory)),
            _ if (opcode & 0xFFC0) == 0x40C0 => opt_cycles!(self.exec_move_from_sr(opcode, memory)),
            _ if (opcode & 0xFFC0) == 0x46C0 => opt_cycles!(self.exec_move_to_sr(opcode, memory)),
            _ if (opcode & 0xFFC0) == 0x4840 && ((opcode >> 3) & 0x7) != 0b000 => {
                opt_cycles!(self.exec_pea(opcode, memory))
            }
            _ if (opcode & 0xFFF8) == 0x4840 => self.exec_swap(opcode),
            _ if (opcode & 0xFFF8) == 0x4880 => self.exec_ext_w(opcode),
            _ if (opcode & 0xFFF8) == 0x48C0 => self.exec_ext_l(opcode),
            _ if (opcode & 0xFB80) == 0x4880 && ((opcode >> 3) & 0x7) >= 0b010 => {
                opt_cycles!(self.exec_movem(opcode, memory))
            }
            _ if (opcode & 0xFFF0) == 0x4E40 => self.exec_trap(opcode, memory),
            _ if (opcode & 0xFF00) == 0x6000 => self.exec_branch(opcode, memory, 0x0),
            _ if (opcode & 0xFF00) == 0x6100 => self.exec_bsr(opcode, memory),
            _ if (opcode & 0xFF00) == 0x6600 => self.exec_branch(opcode, memory, 0x6),
            _ if (opcode & 0xFF00) == 0x6700 => self.exec_branch(opcode, memory, 0x7),
            _ if (opcode & 0xF000) == 0x6000 => opt_cycles!(self.exec_bcc(opcode, memory)),
            _ if (opcode & 0xF000) == 0x5000 => opt_cycles!(self.exec_addq_subq(opcode, memory)),
            _ if (opcode & 0xF100) == 0x7000 => self.exec_moveq(opcode),
            _ if (opcode & 0xFFC0) == 0x44C0 => opt_cycles!(self.exec_move_to_ccr(opcode, memory)),
            _ if (opcode & 0xFF00) == 0x4200 => opt_cycles!(self.exec_clr(opcode, memory)),
            _ if (opcode & 0xFF00) == 0x4400 => opt_cycles!(self.exec_neg(opcode, memory)),
            _ if (opcode & 0xFF00) == 0x4600 => opt_cycles!(self.exec_not(opcode, memory)),
            _ if (opcode & 0xF138) == 0x0108 => opt_cycles!(self.exec_movep(opcode, memory)),
            _ if (opcode & 0xF100) == 0x0100 => opt_cycles!(self.exec_bit_dynamic(opcode, memory)),
            _ if (opcode & 0xFF00) == 0x0800 => {
                opt_cycles!(self.exec_bit_immediate(opcode, memory))
            }
            0x003C => self.exec_ori_to_ccr(memory),
            0x007C => self.exec_ori_to_sr(memory),
            _ if (opcode & 0xFF00) == 0x0000 => opt_cycles!(self.exec_ori(opcode, memory)),
            0x023C => self.exec_andi_to_ccr(memory),
            0x027C => self.exec_andi_to_sr(memory),
            _ if (opcode & 0xFF00) == 0x0400 => opt_cycles!(self.exec_subi(opcode, memory)),
            _ if (opcode & 0xFF00) == 0x0200 => opt_cycles!(self.exec_andi(opcode, memory)),
            _ if (opcode & 0xFF00) == 0x0600 => opt_cycles!(self.exec_addi(opcode, memory)),
            0x0A3C => self.exec_eori_to_ccr(memory),
            0x0A7C => self.exec_eori_to_sr(memory),
            _ if (opcode & 0xFF00) == 0x0A00 => opt_cycles!(self.exec_eori(opcode, memory)),
            _ if (opcode & 0xFF00) == 0x0C00 => opt_cycles!(self.exec_cmpi(opcode, memory)),
            _ if (opcode & 0xF1C0) == 0xB0C0 => opt_cycles!(self.exec_cmpa_w(opcode, memory)),
            _ if (opcode & 0xF1C0) == 0xB1C0 => opt_cycles!(self.exec_cmpa_l(opcode, memory)),
            _ if (opcode & 0xF000) == 0xB000 => opt_cycles!(self.exec_cmp_ea_to_dn(opcode, memory)),
            _ if (opcode & 0xFF00) == 0x4A00 => opt_cycles!(self.exec_tst(opcode, memory)),
            _ if (opcode & 0xF1C0) == 0x3040 => opt_cycles!(self.exec_movea_w(opcode, memory)),
            _ if (opcode & 0xF1C0) == 0x2040 => opt_cycles!(self.exec_movea_l(opcode, memory)),
            _ if (opcode & 0xF1C0) == 0x80C0 => opt_cycles!(self.exec_divu_w(opcode, memory)),
            _ if (opcode & 0xF1C0) == 0x81C0 => opt_cycles!(self.exec_divs_w(opcode, memory)),
            _ if (opcode & 0xF1F8) == 0x8108 => opt_cycles!(self.exec_sbcd(opcode, memory)),
            _ if (opcode & 0xF000) == 0x8000 => opt_cycles!(self.exec_or_ea_to_dn(opcode, memory)),
            _ if (opcode & 0xF1F8) == 0xC108 => opt_cycles!(self.exec_abcd(opcode, memory)),
            _ if (opcode & 0xF1F8) == 0xC140 => self.exec_exg_dd(opcode),
            _ if (opcode & 0xF1F8) == 0xC148 => self.exec_exg_aa(opcode),
            _ if (opcode & 0xF1F8) == 0xC188 => self.exec_exg_da(opcode),
            _ if (opcode & 0xF1C0) == 0xC0C0 => opt_cycles!(self.exec_mulu_w(opcode, memory)),
            _ if (opcode & 0xF1C0) == 0xC1C0 => opt_cycles!(self.exec_muls_w(opcode, memory)),
            _ if (opcode & 0xF000) == 0xC000 => opt_cycles!(self.exec_and_ea_to_dn(opcode, memory)),
            _ if (opcode & 0xF1C0) == 0xD0C0 => opt_cycles!(self.exec_adda_w(opcode, memory)),
            _ if (opcode & 0xF1C0) == 0xD1C0 => opt_cycles!(self.exec_adda_l(opcode, memory)),
            _ if (opcode & 0xF1C0) == 0x90C0 => opt_cycles!(self.exec_suba_w(opcode, memory)),
            _ if (opcode & 0xF1C0) == 0x91C0 => opt_cycles!(self.exec_suba_l(opcode, memory)),
            _ if (opcode & 0xF000) == 0x9000 => opt_cycles!(self.exec_sub_ea_to_dn(opcode, memory)),
            _ if (opcode & 0xF000) == 0xD000 => opt_cycles!(self.exec_add_ea_to_dn(opcode, memory)),
            _ if (opcode & 0xF000) == 0xE000 => opt_cycles!(self.exec_shift_rotate(opcode, memory)),
            _ if (opcode & 0xF1FF) == 0x203C => self.exec_move_l_imm_dn(opcode, memory),
            _ if (opcode & 0xFFF8) == 0x23C0 => self.exec_move_l_dn_abs_l(opcode, memory),
            _ if (opcode & 0xF1C0) == 0x41C0 => opt_cycles!(self.exec_lea(opcode, memory)),
            _ if (opcode & 0xF000) == 0x1000 => {
                opt_cycles!(self.exec_move_b_family(opcode, memory))
            }
            _ if (opcode & 0xF000) == 0x3000 => {
                opt_cycles!(self.exec_move_w_family(opcode, memory))
            }
            _ if opcode == 0x23FC => self.exec_move_l_imm_abs_l(memory),
            _ if (opcode & 0xF000) == 0x2000 => {
                opt_cycles!(self.exec_move_l_family(opcode, memory))
            }
            _ => {
                self.record_unknown_opcode(opcode, self.pc.wrapping_sub(2));
                4 // Unknown opcodes are treated as NOP for now.
            }
        };

        self.cycles += cycles as u64;
        cycles
    }

    pub fn pc(&self) -> u32 {
        self.pc
    }

    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    pub fn a7(&self) -> u32 {
        self.a_regs[7]
    }

    pub fn d_reg(&self, index: usize) -> u32 {
        self.d_regs[index]
    }

    pub fn a_reg(&self, index: usize) -> u32 {
        self.a_regs[index]
    }

    pub fn sr_raw(&self) -> u16 {
        self.sr
    }

    pub fn unknown_opcode_total(&self) -> u64 {
        self.unknown_opcode_total
    }

    pub fn unknown_opcode_histogram(&self) -> Vec<(u16, u64)> {
        let mut entries: Vec<(u16, u64)> = self
            .unknown_opcode_histogram
            .iter()
            .map(|(opcode, count)| (*opcode, *count))
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        entries
    }

    pub fn unknown_opcode_pc_histogram(&self) -> Vec<(u32, u64)> {
        let mut entries: Vec<(u32, u64)> = self
            .unknown_opcode_pc_histogram
            .iter()
            .map(|(pc, count)| (*pc, *count))
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        entries
    }

    pub fn exception_histogram(&self) -> Vec<(u32, u64)> {
        let mut entries: Vec<(u32, u64)> = self
            .exception_histogram
            .iter()
            .map(|(vector, count)| (*vector, *count))
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        entries
    }

    #[cfg(test)]
    pub fn sr(&self) -> u16 {
        self.sr
    }

    fn exec_move_b_family(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst_reg = ((opcode >> 9) & 0x7) as usize;
        let dst_mode = ((opcode >> 6) & 0x7) as u8;
        let src_mode = ((opcode >> 3) & 0x7) as u8;
        let src_reg = (opcode & 0x7) as usize;

        // Destination for MOVE.B cannot be An direct or immediate.
        if dst_mode == 0b001 || (dst_mode == 0b111 && dst_reg == 0b100) {
            return None;
        }

        let src = self.read_ea_byte(src_mode, src_reg, memory)?;
        self.write_ea_byte(dst_mode, dst_reg, src, memory)?;
        self.update_move_flags_byte(src);

        let mut cycles = 8;
        if src_mode == 0b101 || src_mode == 0b111 {
            cycles += 4;
        }
        if dst_mode == 0b101 || dst_mode == 0b111 {
            cycles += 4;
        }
        Some(cycles)
    }

    fn exec_move_w_family(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst_reg = ((opcode >> 9) & 0x7) as usize;
        let dst_mode = ((opcode >> 6) & 0x7) as u8;
        let src_mode = ((opcode >> 3) & 0x7) as u8;
        let src_reg = (opcode & 0x7) as usize;

        // Destination for MOVE.W cannot be An direct or immediate.
        if dst_mode == 0b001 || (dst_mode == 0b111 && dst_reg == 0b100) {
            return None;
        }

        let src = self.read_ea_word(src_mode, src_reg, memory)?;
        self.write_ea_word(dst_mode, dst_reg, src, memory)?;
        self.update_move_flags_word(src);

        let mut cycles = 8;
        if src_mode == 0b101 || src_mode == 0b111 {
            cycles += 4;
        }
        if dst_mode == 0b101 || dst_mode == 0b111 {
            cycles += 4;
        }
        Some(cycles)
    }

    fn exec_move_l_imm_abs_l(&mut self, memory: &mut MemoryMap) -> u32 {
        let value = self.fetch_u32(memory);
        let dst = self.fetch_u32(memory);
        memory.write_u32(dst, value);
        self.update_move_flags_long(value);
        20
    }

    fn exec_move_l_family(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst_reg = ((opcode >> 9) & 0x7) as usize;
        let dst_mode = ((opcode >> 6) & 0x7) as u8;
        let src_mode = ((opcode >> 3) & 0x7) as u8;
        let src_reg = (opcode & 0x7) as usize;

        // Destination for MOVE.L cannot be An direct or immediate.
        if dst_mode == 0b001 || (dst_mode == 0b111 && dst_reg == 0b100) {
            return None;
        }

        let src = self.read_ea_long(src_mode, src_reg, memory)?;
        self.write_ea_long(dst_mode, dst_reg, src, memory)?;
        self.update_move_flags_long(src);

        let mut cycles = 12;
        if src_mode == 0b101 || src_mode == 0b111 {
            cycles += 4;
        }
        if dst_mode == 0b101 || dst_mode == 0b111 {
            cycles += 4;
        }
        Some(cycles)
    }

    fn exec_move_l_imm_dn(&mut self, opcode: u16, memory: &mut MemoryMap) -> u32 {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let value = self.fetch_u32(memory);
        self.d_regs[dst] = value;
        self.update_move_flags_long(value);
        12
    }

    fn exec_move_l_dn_abs_l(&mut self, opcode: u16, memory: &mut MemoryMap) -> u32 {
        let src = (opcode & 0x7) as usize;
        let dst = self.fetch_u32(memory);
        let value = self.d_regs[src];
        memory.write_u32(dst, value);
        self.update_move_flags_long(value);
        16
    }

    fn exec_moveq(&mut self, opcode: u16) -> u32 {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let imm = (opcode & 0x00FF) as u8 as i8 as i32 as u32;
        self.d_regs[dst] = imm;
        self.update_move_flags_long(imm);
        4
    }

    fn exec_movea_w(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let value = self.read_ea_word(mode, reg, memory)? as i16 as i32 as u32;
        self.a_regs[dst] = value;
        Some(8)
    }

    fn exec_movea_l(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let value = self.read_ea_long(mode, reg, memory)?;
        self.a_regs[dst] = value;
        Some(12)
    }

    fn exec_sub_ea_to_dn(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        self.exec_add_sub_ea_to_dn(opcode, memory, ArithOp::Sub)
    }

    fn exec_add_ea_to_dn(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        self.exec_add_sub_ea_to_dn(opcode, memory, ArithOp::Add)
    }

    fn exec_abcd(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        self.exec_bcd_arith(opcode, memory, true)
    }

    fn exec_sbcd(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        self.exec_bcd_arith(opcode, memory, false)
    }

    fn exec_exg_dd(&mut self, opcode: u16) -> u32 {
        let rx = ((opcode >> 9) & 0x7) as usize;
        let ry = (opcode & 0x7) as usize;
        self.d_regs.swap(rx, ry);
        6
    }

    fn exec_exg_aa(&mut self, opcode: u16) -> u32 {
        let rx = ((opcode >> 9) & 0x7) as usize;
        let ry = (opcode & 0x7) as usize;
        self.a_regs.swap(rx, ry);
        6
    }

    fn exec_exg_da(&mut self, opcode: u16) -> u32 {
        let dx = ((opcode >> 9) & 0x7) as usize;
        let ay = (opcode & 0x7) as usize;
        let d = self.d_regs[dx];
        self.d_regs[dx] = self.a_regs[ay];
        self.a_regs[ay] = d;
        6
    }

    fn exec_bcd_arith(&mut self, opcode: u16, memory: &mut MemoryMap, add: bool) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let src = (opcode & 0x7) as usize;
        let mem_mode = (opcode & 0x0008) != 0;
        let x_in = if self.flag_set(CCR_X) { 1i32 } else { 0i32 };

        let (src_byte, dst_byte, dst_addr) = if mem_mode {
            self.a_regs[src] = self.a_regs[src].wrapping_sub(self.byte_addr_step(src));
            let src_addr = self.a_regs[src];
            self.a_regs[dst] = self.a_regs[dst].wrapping_sub(self.byte_addr_step(dst));
            let dst_addr = self.a_regs[dst];
            (
                memory.read_u8(src_addr),
                memory.read_u8(dst_addr),
                Some(dst_addr),
            )
        } else {
            (self.d_regs[src] as u8, self.d_regs[dst] as u8, None)
        };

        let src_dec = ((src_byte >> 4) as i32) * 10 + (src_byte & 0x0F) as i32;
        let dst_dec = ((dst_byte >> 4) as i32) * 10 + (dst_byte & 0x0F) as i32;
        let (result_dec, carry_or_borrow) = if add {
            let sum = dst_dec + src_dec + x_in;
            (sum % 100, sum > 99)
        } else {
            let mut diff = dst_dec - src_dec - x_in;
            let borrow = diff < 0;
            if borrow {
                diff += 100;
            }
            (diff, borrow)
        };

        let result = (((result_dec / 10) as u8) << 4) | ((result_dec % 10) as u8);
        if let Some(addr) = dst_addr {
            memory.write_u8(addr, result);
        } else {
            self.d_regs[dst] = (self.d_regs[dst] & 0xFFFF_FF00) | result as u32;
        }

        self.set_flag(CCR_C, carry_or_borrow);
        self.set_flag(CCR_X, carry_or_borrow);
        self.set_flag(CCR_V, false);
        self.set_flag(CCR_N, (result & 0x80) != 0);
        if result != 0 {
            self.sr &= !CCR_Z;
        }

        Some(if mem_mode { 18 } else { 6 })
    }

    fn exec_or_ea_to_dn(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        self.exec_logic_ea_to_dn(opcode, memory, LogicOp::Or)
    }

    fn exec_and_ea_to_dn(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        self.exec_logic_ea_to_dn(opcode, memory, LogicOp::And)
    }

    fn exec_logic_ea_to_dn(
        &mut self,
        opcode: u16,
        memory: &mut MemoryMap,
        op: LogicOp,
    ) -> Option<u32> {
        let reg_x = ((opcode >> 9) & 0x7) as usize;
        let opmode = ((opcode >> 6) & 0x7) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        if (0b100..=0b110).contains(&opmode) {
            return self.exec_logic_dn_to_ea(reg_x, opmode, mode, reg, memory, op);
        }
        if opmode > 0b010 {
            return None;
        }
        // Logical source for <ea>,Dn cannot be An direct or immediate.
        if mode == 0b001 || (mode == 0b111 && reg == 0b100) {
            return None;
        }

        match opmode {
            0b000 => {
                let src = self.read_ea_byte(mode, reg, memory)?;
                let dst_val = self.d_regs[reg_x] as u8;
                let result = match op {
                    LogicOp::And => dst_val & src,
                    LogicOp::Or => dst_val | src,
                };
                self.d_regs[reg_x] = (self.d_regs[reg_x] & 0xFFFF_FF00) | result as u32;
                self.update_test_flags_byte(result);
                let mut cycles = 4;
                if mode != 0b000 {
                    cycles += 4;
                }
                if mode == 0b101 || mode == 0b111 {
                    cycles += 4;
                }
                Some(cycles)
            }
            0b001 => {
                let src = self.read_ea_word(mode, reg, memory)?;
                let dst_val = self.d_regs[reg_x] as u16;
                let result = match op {
                    LogicOp::And => dst_val & src,
                    LogicOp::Or => dst_val | src,
                };
                self.d_regs[reg_x] = (self.d_regs[reg_x] & 0xFFFF_0000) | result as u32;
                self.update_test_flags_word(result);
                let mut cycles = 4;
                if mode != 0b000 {
                    cycles += 4;
                }
                if mode == 0b101 || mode == 0b111 {
                    cycles += 4;
                }
                Some(cycles)
            }
            0b010 => {
                let src = self.read_ea_long(mode, reg, memory)?;
                let dst_val = self.d_regs[reg_x];
                let result = match op {
                    LogicOp::And => dst_val & src,
                    LogicOp::Or => dst_val | src,
                };
                self.d_regs[reg_x] = result;
                self.update_test_flags_long(result);
                let mut cycles = 8;
                if mode != 0b000 {
                    cycles += 4;
                }
                if mode == 0b101 || mode == 0b111 {
                    cycles += 4;
                }
                Some(cycles)
            }
            _ => None,
        }
    }

    fn exec_logic_dn_to_ea(
        &mut self,
        src_dn: usize,
        opmode: u8,
        mode: u8,
        reg: usize,
        memory: &mut MemoryMap,
        op: LogicOp,
    ) -> Option<u32> {
        // Destination EA for AND/OR Dn,<ea> must be data alterable.
        if mode == 0b001 || (mode == 0b111 && reg >= 0b010) {
            return None;
        }

        match opmode {
            0b100 => {
                let src = self.d_regs[src_dn] as u8;
                let result = if mode == 0b000 {
                    let dst = self.d_regs[reg] as u8;
                    let result = match op {
                        LogicOp::And => dst & src,
                        LogicOp::Or => dst | src,
                    };
                    self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_FF00) | result as u32;
                    result
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 1, memory)?;
                    let dst = memory.read_u8(addr);
                    let result = match op {
                        LogicOp::And => dst & src,
                        LogicOp::Or => dst | src,
                    };
                    memory.write_u8(addr, result);
                    result
                };
                self.update_test_flags_byte(result);
                Some(if mode == 0b000 { 4 } else { 8 })
            }
            0b101 => {
                let src = self.d_regs[src_dn] as u16;
                let result = if mode == 0b000 {
                    let dst = self.d_regs[reg] as u16;
                    let result = match op {
                        LogicOp::And => dst & src,
                        LogicOp::Or => dst | src,
                    };
                    self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_0000) | result as u32;
                    result
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 2, memory)?;
                    let dst = memory.read_u16(addr);
                    let result = match op {
                        LogicOp::And => dst & src,
                        LogicOp::Or => dst | src,
                    };
                    memory.write_u16(addr, result);
                    result
                };
                self.update_test_flags_word(result);
                Some(if mode == 0b000 { 4 } else { 8 })
            }
            0b110 => {
                let src = self.d_regs[src_dn];
                let result = if mode == 0b000 {
                    let dst = self.d_regs[reg];
                    let result = match op {
                        LogicOp::And => dst & src,
                        LogicOp::Or => dst | src,
                    };
                    self.d_regs[reg] = result;
                    result
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 4, memory)?;
                    let dst = memory.read_u32(addr);
                    let result = match op {
                        LogicOp::And => dst & src,
                        LogicOp::Or => dst | src,
                    };
                    memory.write_u32(addr, result);
                    result
                };
                self.update_test_flags_long(result);
                Some(if mode == 0b000 { 8 } else { 12 })
            }
            _ => None,
        }
    }

    fn exec_add_sub_ea_to_dn(
        &mut self,
        opcode: u16,
        memory: &mut MemoryMap,
        op: ArithOp,
    ) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let opmode = ((opcode >> 6) & 0x7) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        match opmode {
            0b000 => {
                // Immediate source is not allowed for ADD/SUB to Dn.
                if mode == 0b111 && reg == 0b100 {
                    return None;
                }
                let src = self.read_ea_byte(mode, reg, memory)?;
                let dst_val = self.d_regs[dst] as u8;
                match op {
                    ArithOp::Add => {
                        let (result, carry) = dst_val.overflowing_add(src);
                        let overflow = ((!(dst_val ^ src)) & (dst_val ^ result) & 0x80) != 0;
                        self.d_regs[dst] = (self.d_regs[dst] & 0xFFFF_FF00) | result as u32;
                        self.update_add_flags_byte_with_extend(result, carry, overflow);
                    }
                    ArithOp::Sub => {
                        let (result, _) = dst_val.overflowing_sub(src);
                        self.d_regs[dst] = (self.d_regs[dst] & 0xFFFF_FF00) | result as u32;
                        self.update_sub_flags_byte_with_extend(dst_val, src, result);
                    }
                }
                let mut cycles = 4;
                if mode != 0b000 {
                    cycles += 4;
                }
                if mode == 0b101 || mode == 0b111 {
                    cycles += 4;
                }
                Some(cycles)
            }
            0b001 => {
                // Immediate source is not allowed for ADD/SUB to Dn.
                if mode == 0b111 && reg == 0b100 {
                    return None;
                }
                let src = self.read_ea_word(mode, reg, memory)?;
                let dst_val = self.d_regs[dst] as u16;
                match op {
                    ArithOp::Add => {
                        let (result, carry) = dst_val.overflowing_add(src);
                        let overflow = ((!(dst_val ^ src)) & (dst_val ^ result) & 0x8000) != 0;
                        self.d_regs[dst] = (self.d_regs[dst] & 0xFFFF_0000) | result as u32;
                        self.update_add_flags_word_with_extend(result, carry, overflow);
                    }
                    ArithOp::Sub => {
                        let result = dst_val.wrapping_sub(src);
                        self.d_regs[dst] = (self.d_regs[dst] & 0xFFFF_0000) | result as u32;
                        self.update_sub_flags_word_with_extend(dst_val, src, result);
                    }
                }
                let mut cycles = 4;
                if mode != 0b000 {
                    cycles += 4;
                }
                if mode == 0b101 || mode == 0b111 {
                    cycles += 4;
                }
                Some(cycles)
            }
            0b010 => {
                // Immediate source is not allowed for ADD/SUB to Dn.
                if mode == 0b111 && reg == 0b100 {
                    return None;
                }
                let src = self.read_ea_long(mode, reg, memory)?;
                let dst_val = self.d_regs[dst];
                match op {
                    ArithOp::Add => {
                        let (result, carry) = dst_val.overflowing_add(src);
                        let overflow = ((!(dst_val ^ src)) & (dst_val ^ result) & 0x8000_0000) != 0;
                        self.d_regs[dst] = result;
                        self.update_add_flags_long_with_extend(result, carry, overflow);
                    }
                    ArithOp::Sub => {
                        let result = dst_val.wrapping_sub(src);
                        self.d_regs[dst] = result;
                        self.update_sub_flags_long_with_extend(dst_val, src, result);
                    }
                }
                let mut cycles = 8;
                if mode != 0b000 {
                    cycles += 4;
                }
                if mode == 0b101 || mode == 0b111 {
                    cycles += 4;
                }
                Some(cycles)
            }
            0b100 => {
                // Destination EA must be data alterable.
                if mode == 0b001 || (mode == 0b111 && reg >= 0b010) {
                    return None;
                }
                let src = self.d_regs[dst] as u8;
                let dst_val = if mode == 0b000 {
                    self.d_regs[reg] as u8
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 1, memory)?;
                    let dst_val = memory.read_u8(addr);
                    let (result, carry, overflow) = match op {
                        ArithOp::Add => {
                            let (result, carry) = dst_val.overflowing_add(src);
                            let overflow = ((!(dst_val ^ src)) & (dst_val ^ result) & 0x80) != 0;
                            (result, carry, overflow)
                        }
                        ArithOp::Sub => {
                            let result = dst_val.wrapping_sub(src);
                            let carry = src > dst_val;
                            let overflow = ((dst_val ^ src) & (dst_val ^ result) & 0x80) != 0;
                            (result, carry, overflow)
                        }
                    };
                    memory.write_u8(addr, result);
                    match op {
                        ArithOp::Add => {
                            self.update_add_flags_byte_with_extend(result, carry, overflow)
                        }
                        ArithOp::Sub => {
                            self.update_sub_flags_byte_with_extend(dst_val, src, result)
                        }
                    }
                    let mut cycles = 8;
                    if mode == 0b101 || mode == 0b110 || mode == 0b111 {
                        cycles += 4;
                    }
                    return Some(cycles);
                };

                let (result, carry, overflow) = match op {
                    ArithOp::Add => {
                        let (result, carry) = dst_val.overflowing_add(src);
                        let overflow = ((!(dst_val ^ src)) & (dst_val ^ result) & 0x80) != 0;
                        (result, carry, overflow)
                    }
                    ArithOp::Sub => {
                        let result = dst_val.wrapping_sub(src);
                        let carry = src > dst_val;
                        let overflow = ((dst_val ^ src) & (dst_val ^ result) & 0x80) != 0;
                        (result, carry, overflow)
                    }
                };
                self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_FF00) | result as u32;
                match op {
                    ArithOp::Add => self.update_add_flags_byte_with_extend(result, carry, overflow),
                    ArithOp::Sub => self.update_sub_flags_byte_with_extend(dst_val, src, result),
                }
                Some(4)
            }
            0b101 => {
                // Destination EA must be data alterable.
                if mode == 0b001 || (mode == 0b111 && reg >= 0b010) {
                    return None;
                }
                let src = self.d_regs[dst] as u16;
                let dst_val = if mode == 0b000 {
                    self.d_regs[reg] as u16
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 2, memory)?;
                    let dst_val = memory.read_u16(addr);
                    let (result, carry, overflow) = match op {
                        ArithOp::Add => {
                            let (result, carry) = dst_val.overflowing_add(src);
                            let overflow = ((!(dst_val ^ src)) & (dst_val ^ result) & 0x8000) != 0;
                            (result, carry, overflow)
                        }
                        ArithOp::Sub => {
                            let result = dst_val.wrapping_sub(src);
                            let carry = src > dst_val;
                            let overflow = ((dst_val ^ src) & (dst_val ^ result) & 0x8000) != 0;
                            (result, carry, overflow)
                        }
                    };
                    memory.write_u16(addr, result);
                    match op {
                        ArithOp::Add => {
                            self.update_add_flags_word_with_extend(result, carry, overflow)
                        }
                        ArithOp::Sub => {
                            self.update_sub_flags_word_with_extend(dst_val, src, result)
                        }
                    }
                    let mut cycles = 8;
                    if mode == 0b101 || mode == 0b110 || mode == 0b111 {
                        cycles += 4;
                    }
                    return Some(cycles);
                };

                let (result, carry, overflow) = match op {
                    ArithOp::Add => {
                        let (result, carry) = dst_val.overflowing_add(src);
                        let overflow = ((!(dst_val ^ src)) & (dst_val ^ result) & 0x8000) != 0;
                        (result, carry, overflow)
                    }
                    ArithOp::Sub => {
                        let result = dst_val.wrapping_sub(src);
                        let carry = src > dst_val;
                        let overflow = ((dst_val ^ src) & (dst_val ^ result) & 0x8000) != 0;
                        (result, carry, overflow)
                    }
                };
                self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_0000) | result as u32;
                match op {
                    ArithOp::Add => self.update_add_flags_word_with_extend(result, carry, overflow),
                    ArithOp::Sub => self.update_sub_flags_word_with_extend(dst_val, src, result),
                }
                Some(4)
            }
            0b110 => {
                // Destination EA must be data alterable.
                if mode == 0b001 || (mode == 0b111 && reg >= 0b010) {
                    return None;
                }
                let src = self.d_regs[dst];
                let dst_val = if mode == 0b000 {
                    self.d_regs[reg]
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 4, memory)?;
                    let dst_val = memory.read_u32(addr);
                    let (result, carry, overflow) = match op {
                        ArithOp::Add => {
                            let (result, carry) = dst_val.overflowing_add(src);
                            let overflow =
                                ((!(dst_val ^ src)) & (dst_val ^ result) & 0x8000_0000) != 0;
                            (result, carry, overflow)
                        }
                        ArithOp::Sub => {
                            let result = dst_val.wrapping_sub(src);
                            let carry = src > dst_val;
                            let overflow =
                                ((dst_val ^ src) & (dst_val ^ result) & 0x8000_0000) != 0;
                            (result, carry, overflow)
                        }
                    };
                    memory.write_u32(addr, result);
                    match op {
                        ArithOp::Add => {
                            self.update_add_flags_long_with_extend(result, carry, overflow)
                        }
                        ArithOp::Sub => {
                            self.update_sub_flags_long_with_extend(dst_val, src, result)
                        }
                    }
                    let mut cycles = 12;
                    if mode == 0b101 || mode == 0b110 || mode == 0b111 {
                        cycles += 4;
                    }
                    return Some(cycles);
                };

                let (result, carry, overflow) = match op {
                    ArithOp::Add => {
                        let (result, carry) = dst_val.overflowing_add(src);
                        let overflow = ((!(dst_val ^ src)) & (dst_val ^ result) & 0x8000_0000) != 0;
                        (result, carry, overflow)
                    }
                    ArithOp::Sub => {
                        let result = dst_val.wrapping_sub(src);
                        let carry = src > dst_val;
                        let overflow = ((dst_val ^ src) & (dst_val ^ result) & 0x8000_0000) != 0;
                        (result, carry, overflow)
                    }
                };
                self.d_regs[reg] = result;
                match op {
                    ArithOp::Add => self.update_add_flags_long_with_extend(result, carry, overflow),
                    ArithOp::Sub => self.update_sub_flags_long_with_extend(dst_val, src, result),
                }
                Some(8)
            }
            _ => None,
        }
    }

    fn exec_addq_subq(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let cond = ((opcode >> 8) & 0xF) as u8;
        let quick_raw = ((opcode >> 9) & 0x7) as u32;
        let quick = if quick_raw == 0 { 8 } else { quick_raw };
        let is_sub = ((opcode >> 8) & 0x1) != 0;
        let size = ((opcode >> 6) & 0x3) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        // ADDQ/SUBQ with size=0b11 are Scc/DBcc encodings.
        if size == 0b11 {
            if mode == 0b001 {
                return self.exec_dbcc(cond, reg, memory);
            }
            return self.exec_scc(cond, mode, reg, memory);
        }

        // Destination cannot be immediate or PC-relative.
        if mode == 0b111 && reg >= 0b010 {
            return None;
        }

        if mode == 0b000 {
            match size {
                0b00 => {
                    let src = quick as u8;
                    let dst = self.d_regs[reg] as u8;
                    if is_sub {
                        let result = dst.wrapping_sub(src);
                        self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_FF00) | result as u32;
                        self.update_sub_flags_byte_with_extend(dst, src, result);
                    } else {
                        let (result, carry) = dst.overflowing_add(src);
                        let overflow = ((!(dst ^ src)) & (dst ^ result) & 0x80) != 0;
                        self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_FF00) | result as u32;
                        self.update_add_flags_byte_with_extend(result, carry, overflow);
                    }
                    return Some(4);
                }
                0b01 => {
                    let src = quick as u16;
                    let dst = self.d_regs[reg] as u16;
                    if is_sub {
                        let result = dst.wrapping_sub(src);
                        self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_0000) | result as u32;
                        self.update_sub_flags_word_with_extend(dst, src, result);
                    } else {
                        let (result, carry) = dst.overflowing_add(src);
                        let overflow = ((!(dst ^ src)) & (dst ^ result) & 0x8000) != 0;
                        self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_0000) | result as u32;
                        self.update_add_flags_word_with_extend(result, carry, overflow);
                    }
                    return Some(4);
                }
                0b10 => {
                    let src = quick;
                    let dst = self.d_regs[reg];
                    if is_sub {
                        let result = dst.wrapping_sub(src);
                        self.d_regs[reg] = result;
                        self.update_sub_flags_long_with_extend(dst, src, result);
                    } else {
                        let (result, carry) = dst.overflowing_add(src);
                        let overflow = ((!(dst ^ src)) & (dst ^ result) & 0x8000_0000) != 0;
                        self.d_regs[reg] = result;
                        self.update_add_flags_long_with_extend(result, carry, overflow);
                    }
                    return Some(8);
                }
                _ => return None,
            }
        }

        // Address register direct is valid for word/long only and does not affect CCR.
        if mode == 0b001 {
            if size == 0b00 {
                return None;
            }
            if is_sub {
                self.a_regs[reg] = self.a_regs[reg].wrapping_sub(quick);
            } else {
                self.a_regs[reg] = self.a_regs[reg].wrapping_add(quick);
            }
            return Some(8);
        }

        let cycles = match size {
            0b00 => {
                let addr = self.resolve_data_alterable_address(mode, reg, 1, memory)?;
                let dst = memory.read_u8(addr);
                let src = quick as u8;
                if is_sub {
                    let result = dst.wrapping_sub(src);
                    memory.write_u8(addr, result);
                    self.update_sub_flags_byte_with_extend(dst, src, result);
                } else {
                    let (result, carry) = dst.overflowing_add(src);
                    let overflow = ((!(dst ^ src)) & (dst ^ result) & 0x80) != 0;
                    memory.write_u8(addr, result);
                    self.update_add_flags_byte_with_extend(result, carry, overflow);
                }
                8
            }
            0b01 => {
                let addr = self.resolve_data_alterable_address(mode, reg, 2, memory)?;
                let dst = memory.read_u16(addr);
                let src = quick as u16;
                if is_sub {
                    let result = dst.wrapping_sub(src);
                    memory.write_u16(addr, result);
                    self.update_sub_flags_word_with_extend(dst, src, result);
                } else {
                    let (result, carry) = dst.overflowing_add(src);
                    let overflow = ((!(dst ^ src)) & (dst ^ result) & 0x8000) != 0;
                    memory.write_u16(addr, result);
                    self.update_add_flags_word_with_extend(result, carry, overflow);
                }
                8
            }
            0b10 => {
                let addr = self.resolve_data_alterable_address(mode, reg, 4, memory)?;
                let dst = memory.read_u32(addr);
                let src = quick;
                if is_sub {
                    let result = dst.wrapping_sub(src);
                    memory.write_u32(addr, result);
                    self.update_sub_flags_long_with_extend(dst, src, result);
                } else {
                    let (result, carry) = dst.overflowing_add(src);
                    let overflow = ((!(dst ^ src)) & (dst ^ result) & 0x8000_0000) != 0;
                    memory.write_u32(addr, result);
                    self.update_add_flags_long_with_extend(result, carry, overflow);
                }
                12
            }
            _ => return None,
        };
        Some(cycles)
    }

    fn exec_scc(&mut self, cond: u8, mode: u8, reg: usize, memory: &mut MemoryMap) -> Option<u32> {
        // Scc destination is data alterable (Dn + memory), but not An direct or immediate/PC-relative.
        if mode == 0b001 || (mode == 0b111 && reg >= 0b010) {
            return None;
        }

        let value = if self.condition_true(cond) {
            0xFF
        } else {
            0x00
        };
        if mode == 0b000 {
            self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_FF00) | value as u32;
            Some(4)
        } else {
            let addr = self.resolve_data_alterable_address(mode, reg, 1, memory)?;
            memory.write_u8(addr, value);
            Some(8)
        }
    }

    fn exec_dbcc(&mut self, cond: u8, reg: usize, memory: &mut MemoryMap) -> Option<u32> {
        let base_pc = self.pc;
        let disp = self.fetch_u16(memory) as i16 as i32;
        if self.condition_true(cond) {
            return Some(12);
        }

        let counter = self.d_regs[reg] as u16;
        let next = counter.wrapping_sub(1);
        self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_0000) | next as u32;

        if next != 0xFFFF {
            self.pc = base_pc.wrapping_add_signed(disp);
            Some(10)
        } else {
            Some(14)
        }
    }

    fn exec_adda_w(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let value = self.read_ea_word(mode, reg, memory)? as i16 as i32 as u32;
        self.a_regs[dst] = self.a_regs[dst].wrapping_add(value);
        Some(8)
    }

    fn exec_mulu_w(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        // MULU source is data EA; An direct is not valid.
        if mode == 0b001 {
            return None;
        }

        let src = self.read_ea_word(mode, reg, memory)? as u32;
        let dst_word = (self.d_regs[dst] & 0xFFFF) as u32;
        let result = dst_word.wrapping_mul(src);
        self.d_regs[dst] = result;
        self.update_test_flags_long(result);
        Some(if mode == 0b000 { 38 } else { 42 })
    }

    fn exec_muls_w(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        // MULS source is data EA; An direct is not valid.
        if mode == 0b001 {
            return None;
        }

        let src = self.read_ea_word(mode, reg, memory)? as i16 as i32;
        let dst_word = (self.d_regs[dst] as u16) as i16 as i32;
        let result = dst_word.wrapping_mul(src) as u32;
        self.d_regs[dst] = result;
        self.update_test_flags_long(result);
        Some(if mode == 0b000 { 54 } else { 58 })
    }

    fn exec_divu_w(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        // DIVU source is data EA; An direct is not valid.
        if mode == 0b001 {
            return None;
        }

        let divisor = self.read_ea_word(mode, reg, memory)? as u32;
        if divisor == 0 {
            self.raise_exception(5, memory, None);
            return Some(38);
        }

        let dividend = self.d_regs[dst];
        let quotient = dividend / divisor;
        let remainder = dividend % divisor;
        if quotient > 0xFFFF {
            self.set_flag(CCR_V, true);
            self.set_flag(CCR_C, false);
            return Some(if mode == 0b000 { 140 } else { 144 });
        }

        self.d_regs[dst] = ((remainder & 0xFFFF) << 16) | (quotient & 0xFFFF);
        let q16 = quotient as u16;
        self.set_flag(CCR_N, (q16 & 0x8000) != 0);
        self.set_flag(CCR_Z, q16 == 0);
        self.set_flag(CCR_V, false);
        self.set_flag(CCR_C, false);
        Some(if mode == 0b000 { 140 } else { 144 })
    }

    fn exec_divs_w(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        // DIVS source is data EA; An direct is not valid.
        if mode == 0b001 {
            return None;
        }

        let divisor = self.read_ea_word(mode, reg, memory)? as i16 as i32;
        if divisor == 0 {
            self.raise_exception(5, memory, None);
            return Some(38);
        }

        let dividend = self.d_regs[dst] as i32;
        let (quotient, remainder) =
            match (dividend.checked_div(divisor), dividend.checked_rem(divisor)) {
                (Some(q), Some(r)) => (q, r),
                _ => {
                    self.set_flag(CCR_V, true);
                    self.set_flag(CCR_C, false);
                    return Some(if mode == 0b000 { 158 } else { 162 });
                }
            };

        if !(-0x8000..=0x7FFF).contains(&quotient) {
            self.set_flag(CCR_V, true);
            self.set_flag(CCR_C, false);
            return Some(if mode == 0b000 { 158 } else { 162 });
        }

        let q16 = quotient as i16 as u16 as u32;
        let r16 = remainder as i16 as u16 as u32;
        self.d_regs[dst] = (r16 << 16) | q16;
        self.set_flag(CCR_N, (q16 as u16 & 0x8000) != 0);
        self.set_flag(CCR_Z, (q16 as u16) == 0);
        self.set_flag(CCR_V, false);
        self.set_flag(CCR_C, false);
        Some(if mode == 0b000 { 158 } else { 162 })
    }

    fn exec_adda_l(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let value = self.read_ea_long(mode, reg, memory)?;
        self.a_regs[dst] = self.a_regs[dst].wrapping_add(value);
        Some(8)
    }

    fn exec_suba_w(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let value = self.read_ea_word(mode, reg, memory)? as i16 as i32 as u32;
        self.a_regs[dst] = self.a_regs[dst].wrapping_sub(value);
        Some(8)
    }

    fn exec_suba_l(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let value = self.read_ea_long(mode, reg, memory)?;
        self.a_regs[dst] = self.a_regs[dst].wrapping_sub(value);
        Some(8)
    }

    fn exec_lea(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let addr = self.resolve_control_address(mode, reg, memory)?;
        self.a_regs[dst] = addr;
        Some(8)
    }

    fn exec_cmpi(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let size = ((opcode >> 6) & 0x3) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        // Destination EA for CMPI cannot be immediate.
        if mode == 0b111 && reg == 0b100 {
            return None;
        }

        match size {
            0b00 => {
                let imm = self.fetch_u16(memory) as u8;
                let value = self.read_ea_byte(mode, reg, memory)?;
                let result = value.wrapping_sub(imm);
                self.update_sub_flags_byte(value, imm, result);
                Some(8)
            }
            0b01 => {
                let imm = self.fetch_u16(memory);
                let value = self.read_ea_word(mode, reg, memory)?;
                let result = value.wrapping_sub(imm);
                self.update_sub_flags_word(value, imm, result);
                Some(8)
            }
            0b10 => {
                let imm = self.fetch_u32(memory);
                let value = self.read_ea_long(mode, reg, memory)?;
                let result = value.wrapping_sub(imm);
                self.update_sub_flags_long(value, imm, result);
                Some(12)
            }
            _ => None,
        }
    }

    fn exec_cmp_ea_to_dn(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let reg_x = ((opcode >> 9) & 0x7) as usize;
        let opmode = ((opcode >> 6) & 0x7) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        if (0b100..=0b110).contains(&opmode) {
            return self.exec_eor_dn_to_ea(reg_x, opmode, mode, reg, memory);
        }

        match opmode {
            0b000 => {
                // Source for CMP <ea>,Dn cannot be An direct or immediate.
                if mode == 0b001 || (mode == 0b111 && reg == 0b100) {
                    return None;
                }
                let src = self.read_ea_byte(mode, reg, memory)?;
                let dst_val = self.d_regs[reg_x] as u8;
                let result = dst_val.wrapping_sub(src);
                self.update_sub_flags_byte(dst_val, src, result);
                let mut cycles = 4;
                if mode != 0b000 {
                    cycles += 4;
                }
                if mode == 0b101 || mode == 0b111 {
                    cycles += 4;
                }
                Some(cycles)
            }
            0b001 => {
                // Source for CMP.W <ea>,Dn cannot be immediate.
                if mode == 0b111 && reg == 0b100 {
                    return None;
                }
                let src = self.read_ea_word(mode, reg, memory)?;
                let dst_val = self.d_regs[reg_x] as u16;
                let result = dst_val.wrapping_sub(src);
                self.update_sub_flags_word(dst_val, src, result);
                let mut cycles = 4;
                if mode != 0b000 {
                    cycles += 4;
                }
                if mode == 0b101 || mode == 0b111 {
                    cycles += 4;
                }
                Some(cycles)
            }
            0b010 => {
                // Source for CMP.L <ea>,Dn cannot be immediate.
                if mode == 0b111 && reg == 0b100 {
                    return None;
                }
                let src = self.read_ea_long(mode, reg, memory)?;
                let dst_val = self.d_regs[reg_x];
                let result = dst_val.wrapping_sub(src);
                self.update_sub_flags_long(dst_val, src, result);
                let mut cycles = 6;
                if mode != 0b000 {
                    cycles += 4;
                }
                if mode == 0b101 || mode == 0b111 {
                    cycles += 4;
                }
                Some(cycles)
            }
            _ => None,
        }
    }

    fn exec_cmpa_w(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let src = self.read_ea_word(mode, reg, memory)? as i16 as i32 as u32;
        let dst_val = self.a_regs[dst];
        let result = dst_val.wrapping_sub(src);
        self.update_sub_flags_long(dst_val, src, result);
        Some(6)
    }

    fn exec_cmpa_l(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dst = ((opcode >> 9) & 0x7) as usize;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let src = self.read_ea_long(mode, reg, memory)?;
        let dst_val = self.a_regs[dst];
        let result = dst_val.wrapping_sub(src);
        self.update_sub_flags_long(dst_val, src, result);
        Some(6)
    }

    fn exec_eor_dn_to_ea(
        &mut self,
        src_dn: usize,
        opmode: u8,
        mode: u8,
        reg: usize,
        memory: &mut MemoryMap,
    ) -> Option<u32> {
        // Destination EA for EOR Dn,<ea> must be data alterable.
        if mode == 0b001 || (mode == 0b111 && reg >= 0b010) {
            return None;
        }

        match opmode {
            0b100 => {
                let src = self.d_regs[src_dn] as u8;
                let result = if mode == 0b000 {
                    let dst = self.d_regs[reg] as u8;
                    let result = dst ^ src;
                    self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_FF00) | result as u32;
                    result
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 1, memory)?;
                    let dst = memory.read_u8(addr);
                    let result = dst ^ src;
                    memory.write_u8(addr, result);
                    result
                };
                self.update_test_flags_byte(result);
                Some(if mode == 0b000 { 4 } else { 8 })
            }
            0b101 => {
                let src = self.d_regs[src_dn] as u16;
                let result = if mode == 0b000 {
                    let dst = self.d_regs[reg] as u16;
                    let result = dst ^ src;
                    self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_0000) | result as u32;
                    result
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 2, memory)?;
                    let dst = memory.read_u16(addr);
                    let result = dst ^ src;
                    memory.write_u16(addr, result);
                    result
                };
                self.update_test_flags_word(result);
                Some(if mode == 0b000 { 4 } else { 8 })
            }
            0b110 => {
                let src = self.d_regs[src_dn];
                let result = if mode == 0b000 {
                    let dst = self.d_regs[reg];
                    let result = dst ^ src;
                    self.d_regs[reg] = result;
                    result
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 4, memory)?;
                    let dst = memory.read_u32(addr);
                    let result = dst ^ src;
                    memory.write_u32(addr, result);
                    result
                };
                self.update_test_flags_long(result);
                Some(if mode == 0b000 { 8 } else { 12 })
            }
            _ => None,
        }
    }

    fn exec_tst(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let size = ((opcode >> 6) & 0x3) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        // TST allows data alterable modes, so An direct and immediate are excluded.
        if mode == 0b001 || (mode == 0b111 && reg == 0b100) {
            return None;
        }

        match size {
            0b00 => {
                let value = self.read_ea_byte(mode, reg, memory)?;
                self.update_test_flags_byte(value);
                Some(4)
            }
            0b01 => {
                let value = self.read_ea_word(mode, reg, memory)?;
                self.update_test_flags_word(value);
                Some(4)
            }
            0b10 => {
                let value = self.read_ea_long(mode, reg, memory)?;
                self.update_test_flags_long(value);
                Some(4)
            }
            _ => None,
        }
    }

    fn exec_ori(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        self.exec_imm_logical(opcode, memory, |dst, imm| dst | imm)
    }

    fn exec_andi(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        self.exec_imm_logical(opcode, memory, |dst, imm| dst & imm)
    }

    fn exec_eori(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        self.exec_imm_logical(opcode, memory, |dst, imm| dst ^ imm)
    }

    fn exec_addi(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        self.exec_imm_arith(opcode, memory, ArithOp::Add)
    }

    fn exec_subi(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        self.exec_imm_arith(opcode, memory, ArithOp::Sub)
    }

    fn exec_imm_logical<F>(&mut self, opcode: u16, memory: &mut MemoryMap, op: F) -> Option<u32>
    where
        F: Fn(u32, u32) -> u32,
    {
        let size = ((opcode >> 6) & 0x3) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        // Logical immediate destination is data alterable only.
        if mode == 0b001 || (mode == 0b111 && reg == 0b100) {
            return None;
        }

        match size {
            0b00 => {
                let imm = self.fetch_u16(memory) as u8;
                let result = if mode == 0b000 {
                    let dst = self.d_regs[reg] as u8;
                    let result = op(dst as u32, imm as u32) as u8;
                    self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_FF00) | result as u32;
                    result
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 1, memory)?;
                    let dst = memory.read_u8(addr);
                    let result = op(dst as u32, imm as u32) as u8;
                    memory.write_u8(addr, result);
                    result
                };
                self.update_test_flags_byte(result);
                Some(if mode == 0b000 { 8 } else { 12 })
            }
            0b01 => {
                let imm = self.fetch_u16(memory);
                let result = if mode == 0b000 {
                    let dst = self.d_regs[reg] as u16;
                    let result = op(dst as u32, imm as u32) as u16;
                    self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_0000) | result as u32;
                    result
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 2, memory)?;
                    let dst = memory.read_u16(addr);
                    let result = op(dst as u32, imm as u32) as u16;
                    memory.write_u16(addr, result);
                    result
                };
                self.update_test_flags_word(result);
                Some(if mode == 0b000 { 8 } else { 12 })
            }
            0b10 => {
                let imm = self.fetch_u32(memory);
                let result = if mode == 0b000 {
                    let dst = self.d_regs[reg];
                    let result = op(dst, imm);
                    self.d_regs[reg] = result;
                    result
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 4, memory)?;
                    let dst = memory.read_u32(addr);
                    let result = op(dst, imm);
                    memory.write_u32(addr, result);
                    result
                };
                self.update_test_flags_long(result);
                Some(if mode == 0b000 { 16 } else { 20 })
            }
            _ => None,
        }
    }

    fn exec_imm_arith(&mut self, opcode: u16, memory: &mut MemoryMap, op: ArithOp) -> Option<u32> {
        let size = ((opcode >> 6) & 0x3) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        if mode == 0b001 || (mode == 0b111 && reg == 0b100) {
            return None;
        }

        match size {
            0b00 => {
                let imm = self.fetch_u16(memory) as u8;
                let (dst, store) = if mode == 0b000 {
                    let dst = self.d_regs[reg] as u8;
                    (dst, ImmStore::DnByte(reg))
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 1, memory)?;
                    (memory.read_u8(addr), ImmStore::MemByte(addr))
                };
                let (result, carry, overflow) = match op {
                    ArithOp::Add => {
                        let (result, carry) = dst.overflowing_add(imm);
                        let overflow = ((!(dst ^ imm)) & (dst ^ result) & 0x80) != 0;
                        (result, carry, overflow)
                    }
                    ArithOp::Sub => {
                        let (result, borrow) = dst.overflowing_sub(imm);
                        let overflow = ((dst ^ imm) & (dst ^ result) & 0x80) != 0;
                        (result, borrow, overflow)
                    }
                };
                match store {
                    ImmStore::DnByte(r) => {
                        self.d_regs[r] = (self.d_regs[r] & 0xFFFF_FF00) | result as u32;
                    }
                    ImmStore::MemByte(addr) => memory.write_u8(addr, result),
                    _ => unreachable!(),
                }
                match op {
                    ArithOp::Add => self.update_add_flags_byte_with_extend(result, carry, overflow),
                    ArithOp::Sub => self.update_sub_flags_byte_with_extend(dst, imm, result),
                }
                Some(if mode == 0b000 { 8 } else { 12 })
            }
            0b01 => {
                let imm = self.fetch_u16(memory);
                let (dst, store) = if mode == 0b000 {
                    let dst = self.d_regs[reg] as u16;
                    (dst, ImmStore::DnWord(reg))
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 2, memory)?;
                    (memory.read_u16(addr), ImmStore::MemWord(addr))
                };
                let (result, carry, overflow) = match op {
                    ArithOp::Add => {
                        let (result, carry) = dst.overflowing_add(imm);
                        let overflow = ((!(dst ^ imm)) & (dst ^ result) & 0x8000) != 0;
                        (result, carry, overflow)
                    }
                    ArithOp::Sub => {
                        let result = dst.wrapping_sub(imm);
                        let carry = imm > dst;
                        let overflow = ((dst ^ imm) & (dst ^ result) & 0x8000) != 0;
                        (result, carry, overflow)
                    }
                };
                match store {
                    ImmStore::DnWord(r) => {
                        self.d_regs[r] = (self.d_regs[r] & 0xFFFF_0000) | result as u32;
                    }
                    ImmStore::MemWord(addr) => memory.write_u16(addr, result),
                    _ => unreachable!(),
                }
                match op {
                    ArithOp::Add => self.update_add_flags_word_with_extend(result, carry, overflow),
                    ArithOp::Sub => self.update_sub_flags_word_with_extend(dst, imm, result),
                }
                Some(if mode == 0b000 { 8 } else { 12 })
            }
            0b10 => {
                let imm = self.fetch_u32(memory);
                let (dst, store) = if mode == 0b000 {
                    let dst = self.d_regs[reg];
                    (dst, ImmStore::DnLong(reg))
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 4, memory)?;
                    (memory.read_u32(addr), ImmStore::MemLong(addr))
                };
                let (result, carry, overflow) = match op {
                    ArithOp::Add => {
                        let (result, carry) = dst.overflowing_add(imm);
                        let overflow = ((!(dst ^ imm)) & (dst ^ result) & 0x8000_0000) != 0;
                        (result, carry, overflow)
                    }
                    ArithOp::Sub => {
                        let result = dst.wrapping_sub(imm);
                        let carry = imm > dst;
                        let overflow = ((dst ^ imm) & (dst ^ result) & 0x8000_0000) != 0;
                        (result, carry, overflow)
                    }
                };
                match store {
                    ImmStore::DnLong(r) => self.d_regs[r] = result,
                    ImmStore::MemLong(addr) => memory.write_u32(addr, result),
                    _ => unreachable!(),
                }
                match op {
                    ArithOp::Add => self.update_add_flags_long_with_extend(result, carry, overflow),
                    ArithOp::Sub => self.update_sub_flags_long_with_extend(dst, imm, result),
                }
                Some(if mode == 0b000 { 16 } else { 20 })
            }
            _ => None,
        }
    }

    fn exec_clr(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let size = ((opcode >> 6) & 0x3) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        // CLR supports data alterable destinations (Dn + memory), but not An direct or immediate.
        if mode == 0b001 || (mode == 0b111 && reg == 0b100) {
            return None;
        }

        match size {
            0b00 => {
                self.write_ea_byte(mode, reg, 0, memory)?;
                self.update_test_flags_byte(0);
                Some(if mode == 0b000 { 4 } else { 8 })
            }
            0b01 => {
                self.write_ea_word(mode, reg, 0, memory)?;
                self.update_test_flags_word(0);
                Some(if mode == 0b000 { 4 } else { 8 })
            }
            0b10 => {
                self.write_ea_long(mode, reg, 0, memory)?;
                self.update_test_flags_long(0);
                Some(if mode == 0b000 { 6 } else { 12 })
            }
            _ => None,
        }
    }

    fn exec_jsr(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let target = self.resolve_control_address(mode, reg, memory)?;
        let return_addr = self.pc;
        self.push_u32(memory, return_addr);
        self.pc = target;
        Some(16)
    }

    fn exec_jmp(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let target = self.resolve_control_address(mode, reg, memory)?;
        self.pc = target;
        Some(10)
    }

    fn exec_link(&mut self, opcode: u16, memory: &mut MemoryMap) -> u32 {
        let reg = (opcode & 0x7) as usize;
        let displacement = self.fetch_u16(memory) as i16 as i32;
        self.push_u32(memory, self.a_regs[reg]);
        self.a_regs[reg] = self.a_regs[7];
        self.a_regs[7] = self.a_regs[7].wrapping_add_signed(displacement);
        16
    }

    fn exec_unlk(&mut self, opcode: u16, memory: &mut MemoryMap) -> u32 {
        let reg = (opcode & 0x7) as usize;
        self.a_regs[7] = self.a_regs[reg];
        self.a_regs[reg] = self.pop_u32(memory);
        12
    }

    fn exec_move_usp(&mut self, opcode: u16, memory: &mut MemoryMap) -> u32 {
        if (self.sr & SR_SUPERVISOR) == 0 {
            self.raise_exception(8, memory, None);
            return 34;
        }

        let reg = (opcode & 0x7) as usize;
        if (opcode & 0x0008) == 0 {
            self.usp = self.a_regs[reg];
        } else {
            self.a_regs[reg] = self.usp;
        }
        4
    }

    fn exec_move_from_sr(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        // Destination must be data alterable; An direct and immediate are invalid.
        if mode == 0b001 || (mode == 0b111 && reg == 0b100) {
            return None;
        }
        self.write_ea_word(mode, reg, self.sr, memory)?;
        Some(if mode == 0b000 { 6 } else { 8 })
    }

    fn exec_move_to_sr(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        if (self.sr & SR_SUPERVISOR) == 0 {
            self.raise_exception(8, memory, None);
            return Some(34);
        }

        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        // Source must be data addressing mode; An direct is invalid.
        if mode == 0b001 {
            return None;
        }
        let value = self.read_ea_word(mode, reg, memory)?;
        self.write_sr(value);
        Some(if mode == 0b000 || (mode == 0b111 && reg == 0b100) {
            12
        } else {
            16
        })
    }

    fn exec_ori_to_ccr(&mut self, memory: &mut MemoryMap) -> u32 {
        let imm = self.fetch_u16(memory);
        self.sr = (self.sr & !0x001F) | ((self.sr | imm) & 0x001F);
        20
    }

    fn exec_ori_to_sr(&mut self, memory: &mut MemoryMap) -> u32 {
        if (self.sr & SR_SUPERVISOR) == 0 {
            self.raise_exception(8, memory, None);
            return 34;
        }
        let imm = self.fetch_u16(memory);
        self.write_sr(self.sr | imm);
        20
    }

    fn exec_andi_to_ccr(&mut self, memory: &mut MemoryMap) -> u32 {
        let imm = self.fetch_u16(memory) & 0x001F;
        self.sr = (self.sr & !0x001F) | ((self.sr & imm) & 0x001F);
        20
    }

    fn exec_andi_to_sr(&mut self, memory: &mut MemoryMap) -> u32 {
        if (self.sr & SR_SUPERVISOR) == 0 {
            self.raise_exception(8, memory, None);
            return 34;
        }
        let imm = self.fetch_u16(memory);
        self.write_sr(self.sr & imm);
        20
    }

    fn exec_eori_to_ccr(&mut self, memory: &mut MemoryMap) -> u32 {
        let imm = self.fetch_u16(memory) & 0x001F;
        self.sr = (self.sr & !0x001F) | ((self.sr ^ imm) & 0x001F);
        20
    }

    fn exec_eori_to_sr(&mut self, memory: &mut MemoryMap) -> u32 {
        if (self.sr & SR_SUPERVISOR) == 0 {
            self.raise_exception(8, memory, None);
            return 34;
        }
        let imm = self.fetch_u16(memory);
        self.write_sr(self.sr ^ imm);
        20
    }

    fn exec_move_to_ccr(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        // Source must be data addressing mode; An direct is invalid.
        if mode == 0b001 {
            return None;
        }
        let value = self.read_ea_word(mode, reg, memory)?;
        self.sr = (self.sr & !0x001F) | (value & 0x001F);
        Some(if mode == 0b000 || (mode == 0b111 && reg == 0b100) {
            12
        } else {
            16
        })
    }

    fn exec_neg(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let size = ((opcode >> 6) & 0x3) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        // NEG supports data alterable destinations (Dn + memory), but not An direct or immediate.
        if mode == 0b001 || (mode == 0b111 && reg == 0b100) {
            return None;
        }

        match size {
            0b00 => {
                let (dst, addr) = if mode == 0b000 {
                    (self.d_regs[reg] as u8, None)
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 1, memory)?;
                    (memory.read_u8(addr), Some(addr))
                };
                let result = (0u8).wrapping_sub(dst);
                if mode == 0b000 {
                    self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_FF00) | result as u32;
                } else {
                    memory.write_u8(addr.expect("memory mode must resolve address"), result);
                }
                self.update_sub_flags_byte_with_extend(0, dst, result);
                self.set_flag(CCR_X, dst != 0);
                Some(if mode == 0b000 { 4 } else { 8 })
            }
            0b01 => {
                let (dst, addr) = if mode == 0b000 {
                    (self.d_regs[reg] as u16, None)
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 2, memory)?;
                    (memory.read_u16(addr), Some(addr))
                };
                let result = (0u16).wrapping_sub(dst);
                if mode == 0b000 {
                    self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_0000) | result as u32;
                } else {
                    memory.write_u16(addr.expect("memory mode must resolve address"), result);
                }
                self.update_sub_flags_word_with_extend(0, dst, result);
                self.set_flag(CCR_X, dst != 0);
                Some(if mode == 0b000 { 4 } else { 8 })
            }
            0b10 => {
                let (dst, addr) = if mode == 0b000 {
                    (self.d_regs[reg], None)
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 4, memory)?;
                    (memory.read_u32(addr), Some(addr))
                };
                let result = (0u32).wrapping_sub(dst);
                if mode == 0b000 {
                    self.d_regs[reg] = result;
                } else {
                    memory.write_u32(addr.expect("memory mode must resolve address"), result);
                }
                self.update_sub_flags_long_with_extend(0, dst, result);
                self.set_flag(CCR_X, dst != 0);
                Some(if mode == 0b000 { 6 } else { 12 })
            }
            _ => None,
        }
    }

    fn exec_not(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let size = ((opcode >> 6) & 0x3) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;

        // NOT supports data alterable destinations (Dn + memory), but not An direct or immediate.
        if mode == 0b001 || (mode == 0b111 && reg == 0b100) {
            return None;
        }

        match size {
            0b00 => {
                let (dst, addr) = if mode == 0b000 {
                    (self.d_regs[reg] as u8, None)
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 1, memory)?;
                    (memory.read_u8(addr), Some(addr))
                };
                let result = !dst;
                if mode == 0b000 {
                    self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_FF00) | result as u32;
                } else {
                    memory.write_u8(addr.expect("memory mode must resolve address"), result);
                }
                self.update_test_flags_byte(result);
                Some(if mode == 0b000 { 4 } else { 8 })
            }
            0b01 => {
                let (dst, addr) = if mode == 0b000 {
                    (self.d_regs[reg] as u16, None)
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 2, memory)?;
                    (memory.read_u16(addr), Some(addr))
                };
                let result = !dst;
                if mode == 0b000 {
                    self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_0000) | result as u32;
                } else {
                    memory.write_u16(addr.expect("memory mode must resolve address"), result);
                }
                self.update_test_flags_word(result);
                Some(if mode == 0b000 { 4 } else { 8 })
            }
            0b10 => {
                let (dst, addr) = if mode == 0b000 {
                    (self.d_regs[reg], None)
                } else {
                    let addr = self.resolve_data_alterable_address(mode, reg, 4, memory)?;
                    (memory.read_u32(addr), Some(addr))
                };
                let result = !dst;
                if mode == 0b000 {
                    self.d_regs[reg] = result;
                } else {
                    memory.write_u32(addr.expect("memory mode must resolve address"), result);
                }
                self.update_test_flags_long(result);
                Some(if mode == 0b000 { 6 } else { 12 })
            }
            _ => None,
        }
    }

    fn exec_swap(&mut self, opcode: u16) -> u32 {
        let reg = (opcode & 0x7) as usize;
        let result = self.d_regs[reg].rotate_left(16);
        self.d_regs[reg] = result;
        self.update_test_flags_long(result);
        4
    }

    fn exec_pea(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let addr = match mode {
            0b010 => self.a_regs[reg],
            0b101 => {
                let disp = self.fetch_u16(memory) as i16 as i32;
                self.a_regs[reg].wrapping_add_signed(disp)
            }
            0b111 => match reg {
                0b000 => self.fetch_u16(memory) as i16 as i32 as u32,
                0b001 => self.fetch_u32(memory),
                0b010 => {
                    let base_pc = self.pc;
                    let disp = self.fetch_u16(memory) as i16 as i32;
                    base_pc.wrapping_add_signed(disp)
                }
                _ => return None,
            },
            _ => return None,
        };
        self.push_u32(memory, addr);
        Some(if mode == 0b010 { 12 } else { 16 })
    }

    fn exec_ext_w(&mut self, opcode: u16) -> u32 {
        let reg = (opcode & 0x7) as usize;
        let extended = (self.d_regs[reg] as u8 as i8 as i16) as u16;
        self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_0000) | extended as u32;
        self.update_test_flags_word(extended);
        4
    }

    fn exec_ext_l(&mut self, opcode: u16) -> u32 {
        let reg = (opcode & 0x7) as usize;
        let extended = (self.d_regs[reg] as u16 as i16 as i32) as u32;
        self.d_regs[reg] = extended;
        self.update_test_flags_long(extended);
        4
    }

    fn exec_movem(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let mem_to_regs = (opcode & 0x0400) != 0;
        let size_long = (opcode & 0x0040) != 0;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let mask = self.fetch_u16(memory);
        let count = mask.count_ones();
        if count == 0 {
            return Some(8);
        }

        if mem_to_regs {
            let (mut addr, postinc_reg) = self.movem_resolve_mem_source(mode, reg, memory)?;
            let step = if size_long { 4 } else { 2 };

            for bit in 0..16 {
                if (mask & (1u16 << bit)) == 0 {
                    continue;
                }
                let value = if size_long {
                    memory.read_u32(addr)
                } else {
                    memory.read_u16(addr) as i16 as i32 as u32
                };
                self.movem_set_register(bit as usize, value);
                addr = addr.wrapping_add(step);
            }

            if let Some(an) = postinc_reg {
                self.a_regs[an] = addr;
            }
        } else {
            let (mut addr, predec_reg) = self.movem_resolve_mem_dest(mode, reg, memory)?;
            let step = if size_long { 4 } else { 2 };

            if let Some(an) = predec_reg {
                for bit in 0..16 {
                    if (mask & (1u16 << bit)) == 0 {
                        continue;
                    }
                    let reg_index = 15 - bit as usize;
                    let value = self.movem_get_register(reg_index);
                    addr = addr.wrapping_sub(step);
                    if size_long {
                        memory.write_u32(addr, value);
                    } else {
                        memory.write_u16(addr, value as u16);
                    }
                }
                self.a_regs[an] = addr;
            } else {
                for bit in 0..16 {
                    if (mask & (1u16 << bit)) == 0 {
                        continue;
                    }
                    let value = self.movem_get_register(bit as usize);
                    if size_long {
                        memory.write_u32(addr, value);
                    } else {
                        memory.write_u16(addr, value as u16);
                    }
                    addr = addr.wrapping_add(step);
                }
            }
        }

        Some(if size_long {
            12 + count * 4
        } else {
            8 + count * 4
        })
    }

    fn exec_shift_rotate(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let size = ((opcode >> 6) & 0x3) as u8;
        if size == 0b11 {
            let op = ((opcode >> 9) & 0x7) as u8;
            let mode = ((opcode >> 3) & 0x7) as u8;
            let reg = (opcode & 0x7) as usize;

            // Memory form uses data-alterable memory EA only.
            if mode == 0b000 || mode == 0b001 || (mode == 0b111 && reg >= 0b010) {
                return None;
            }

            let addr = self.resolve_data_alterable_address(mode, reg, 2, memory)?;
            let value = memory.read_u16(addr);
            let (result, carry_out) = match op {
                // ASR.W <ea>
                0b000 => {
                    let carry = (value & 0x0001) != 0;
                    let result = ((value as i16) >> 1) as u16;
                    self.set_flag(CCR_X, carry);
                    (result, carry)
                }
                // ASL.W <ea>
                0b001 => {
                    let carry = (value & 0x8000) != 0;
                    let result = value.wrapping_shl(1);
                    self.set_flag(CCR_X, carry);
                    (result, carry)
                }
                // LSR.W <ea>
                0b010 => {
                    let carry = (value & 0x0001) != 0;
                    let result = value >> 1;
                    self.set_flag(CCR_X, carry);
                    (result, carry)
                }
                // LSL.W <ea>
                0b011 => {
                    let carry = (value & 0x8000) != 0;
                    let result = value.wrapping_shl(1);
                    self.set_flag(CCR_X, carry);
                    (result, carry)
                }
                // ROXR.W <ea>
                0b100 => {
                    let x_in = self.flag_set(CCR_X);
                    let carry = (value & 0x0001) != 0;
                    let result = (value >> 1) | ((x_in as u16) << 15);
                    self.set_flag(CCR_X, carry);
                    (result, carry)
                }
                // ROXL.W <ea>
                0b101 => {
                    let x_in = self.flag_set(CCR_X);
                    let carry = (value & 0x8000) != 0;
                    let result = value.wrapping_shl(1) | (x_in as u16);
                    self.set_flag(CCR_X, carry);
                    (result, carry)
                }
                // ROR.W <ea>
                0b110 => {
                    let carry = (value & 0x0001) != 0;
                    let result = (value >> 1) | ((carry as u16) << 15);
                    (result, carry)
                }
                // ROL.W <ea>
                0b111 => {
                    let carry = (value & 0x8000) != 0;
                    let result = value.wrapping_shl(1) | (carry as u16);
                    (result, carry)
                }
                _ => return None,
            };

            memory.write_u16(addr, result);
            self.set_flag(CCR_N, (result & 0x8000) != 0);
            self.set_flag(CCR_Z, result == 0);
            self.set_flag(CCR_V, false);
            self.set_flag(CCR_C, carry_out);
            return Some(8);
        }

        let dst = (opcode & 0x7) as usize;
        let op = ((opcode >> 3) & 0x3) as u8;
        let left = (opcode & 0x0100) != 0;
        let count_from_reg = (opcode & 0x0020) != 0;
        let count_field = ((opcode >> 9) & 0x7) as usize;
        let mut count = if count_from_reg {
            (self.d_regs[count_field] & 0x3F) as u32
        } else {
            let imm = count_field as u32;
            if imm == 0 { 8 } else { imm }
        };

        let (width, mask, sign_bit) = match size {
            0b00 => (8u32, 0x0000_00FFu32, 0x0000_0080u32),
            0b01 => (16u32, 0x0000_FFFFu32, 0x0000_8000u32),
            0b10 => (32u32, 0xFFFF_FFFFu32, 0x8000_0000u32),
            _ => return None,
        };
        let mut value = self.d_regs[dst] & mask;
        let mut carry_out = false;

        if count > 0 {
            while count > 0 {
                match op {
                    // ASx
                    0b00 => {
                        if left {
                            carry_out = (value & sign_bit) != 0;
                            value = (value << 1) & mask;
                        } else {
                            carry_out = (value & 0x1) != 0;
                            let fill = value & sign_bit;
                            value >>= 1;
                            if fill != 0 {
                                value |= sign_bit;
                            }
                        }
                    }
                    // LSx
                    0b01 => {
                        if left {
                            carry_out = (value & sign_bit) != 0;
                            value = (value << 1) & mask;
                        } else {
                            carry_out = (value & 0x1) != 0;
                            value >>= 1;
                        }
                    }
                    // ROXx
                    0b10 => {
                        let x_in = self.flag_set(CCR_X);
                        if left {
                            carry_out = (value & sign_bit) != 0;
                            value = ((value << 1) & mask) | (x_in as u32);
                        } else {
                            carry_out = (value & 0x1) != 0;
                            value = (value >> 1) | ((x_in as u32) << (width - 1));
                            value &= mask;
                        }
                        self.set_flag(CCR_X, carry_out);
                    }
                    // ROx
                    0b11 => {
                        if left {
                            carry_out = (value & sign_bit) != 0;
                            value = ((value << 1) & mask) | (carry_out as u32);
                        } else {
                            carry_out = (value & 0x1) != 0;
                            value = (value >> 1) | ((carry_out as u32) << (width - 1));
                            value &= mask;
                        }
                    }
                    _ => return None,
                }
                count -= 1;
            }
        }

        self.set_shift_rotate_result(dst, size, value);
        self.set_flag(CCR_N, (value & sign_bit) != 0);
        self.set_flag(CCR_Z, value == 0);
        self.set_flag(CCR_V, false);
        self.set_flag(CCR_C, carry_out);
        if op != 0b11 {
            self.set_flag(CCR_X, carry_out);
        }

        let shift_count = if count_from_reg {
            (self.d_regs[count_field] & 0x3F) as u32
        } else {
            let imm = count_field as u32;
            if imm == 0 { 8 } else { imm }
        };
        Some(6 + shift_count * 2)
    }

    fn exec_movep(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let dn = ((opcode >> 9) & 0x7) as usize;
        let opmode = ((opcode >> 6) & 0x3) as u8;
        let an = (opcode & 0x7) as usize;
        let displacement = self.fetch_u16(memory) as i16 as i32;
        let addr = self.a_regs[an].wrapping_add_signed(displacement);

        match opmode {
            // MOVEP.W (d16,An),Dn
            0b00 => {
                let hi = memory.read_u8(addr);
                let lo = memory.read_u8(addr.wrapping_add(2));
                let value = u16::from_be_bytes([hi, lo]) as u32;
                self.d_regs[dn] = (self.d_regs[dn] & 0xFFFF_0000) | value;
                Some(16)
            }
            // MOVEP.L (d16,An),Dn
            0b01 => {
                let b0 = memory.read_u8(addr);
                let b1 = memory.read_u8(addr.wrapping_add(2));
                let b2 = memory.read_u8(addr.wrapping_add(4));
                let b3 = memory.read_u8(addr.wrapping_add(6));
                self.d_regs[dn] = u32::from_be_bytes([b0, b1, b2, b3]);
                Some(24)
            }
            // MOVEP.W Dn,(d16,An)
            0b10 => {
                let bytes = (self.d_regs[dn] as u16).to_be_bytes();
                memory.write_u8(addr, bytes[0]);
                memory.write_u8(addr.wrapping_add(2), bytes[1]);
                Some(16)
            }
            // MOVEP.L Dn,(d16,An)
            0b11 => {
                let bytes = self.d_regs[dn].to_be_bytes();
                memory.write_u8(addr, bytes[0]);
                memory.write_u8(addr.wrapping_add(2), bytes[1]);
                memory.write_u8(addr.wrapping_add(4), bytes[2]);
                memory.write_u8(addr.wrapping_add(6), bytes[3]);
                Some(24)
            }
            _ => None,
        }
    }

    fn exec_bit_dynamic(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let bit_reg = ((opcode >> 9) & 0x7) as usize;
        let op = ((opcode >> 6) & 0x3) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let bit_num = self.d_regs[bit_reg] as u8;
        self.exec_bit_op(op, mode, reg, bit_num, memory, true)
    }

    fn exec_bit_immediate(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let op = ((opcode >> 6) & 0x3) as u8;
        let mode = ((opcode >> 3) & 0x7) as u8;
        let reg = (opcode & 0x7) as usize;
        let bit_num = self.fetch_u16(memory) as u8;
        self.exec_bit_op(op, mode, reg, bit_num, memory, false)
    }

    fn exec_bit_op(
        &mut self,
        op: u8,
        mode: u8,
        reg: usize,
        bit_num: u8,
        memory: &mut MemoryMap,
        dynamic: bool,
    ) -> Option<u32> {
        if mode == 0b000 {
            let bit = (bit_num & 0x1F) as u32;
            let mask = 1u32 << bit;
            let old_set = (self.d_regs[reg] & mask) != 0;
            self.set_flag(CCR_Z, !old_set);
            match op {
                0b00 => {}
                0b01 => self.d_regs[reg] ^= mask,
                0b10 => self.d_regs[reg] &= !mask,
                0b11 => self.d_regs[reg] |= mask,
                _ => return None,
            }
            return Some(if dynamic { 6 } else { 10 });
        }

        // Memory destinations are byte-sized and must be data alterable.
        if mode == 0b001 || (mode == 0b111 && reg == 0b100) {
            return None;
        }
        let addr = self.resolve_data_alterable_address(mode, reg, 1, memory)?;
        let mut value = memory.read_u8(addr);
        let bit = bit_num & 0x07;
        let mask = 1u8 << bit;
        let old_set = (value & mask) != 0;
        self.set_flag(CCR_Z, !old_set);
        match op {
            0b00 => {}
            0b01 => value ^= mask,
            0b10 => value &= !mask,
            0b11 => value |= mask,
            _ => return None,
        }
        if op != 0b00 {
            memory.write_u8(addr, value);
        }
        Some(if dynamic { 8 } else { 12 })
    }

    fn movem_resolve_mem_source(
        &mut self,
        mode: u8,
        reg: usize,
        memory: &mut MemoryMap,
    ) -> Option<(u32, Option<usize>)> {
        match mode {
            0b010 => Some((self.a_regs[reg], None)),
            0b011 => Some((self.a_regs[reg], Some(reg))),
            0b101 => {
                let disp = self.fetch_u16(memory) as i16 as i32;
                Some((self.a_regs[reg].wrapping_add_signed(disp), None))
            }
            0b110 => Some((self.resolve_indexed_address(self.a_regs[reg], memory), None)),
            0b111 => match reg {
                0b000 => Some((self.fetch_u16(memory) as i16 as i32 as u32, None)),
                0b001 => Some((self.fetch_u32(memory), None)),
                0b010 => {
                    let base_pc = self.pc;
                    let disp = self.fetch_u16(memory) as i16 as i32;
                    Some((base_pc.wrapping_add_signed(disp), None))
                }
                0b011 => Some((self.resolve_pc_indexed_address(memory), None)),
                _ => None,
            },
            _ => None,
        }
    }

    fn movem_resolve_mem_dest(
        &mut self,
        mode: u8,
        reg: usize,
        memory: &mut MemoryMap,
    ) -> Option<(u32, Option<usize>)> {
        match mode {
            0b010 => Some((self.a_regs[reg], None)),
            0b100 => Some((self.a_regs[reg], Some(reg))),
            0b101 => {
                let disp = self.fetch_u16(memory) as i16 as i32;
                Some((self.a_regs[reg].wrapping_add_signed(disp), None))
            }
            0b110 => Some((self.resolve_indexed_address(self.a_regs[reg], memory), None)),
            0b111 => match reg {
                0b000 => Some((self.fetch_u16(memory) as i16 as i32 as u32, None)),
                0b001 => Some((self.fetch_u32(memory), None)),
                _ => None,
            },
            _ => None,
        }
    }

    fn movem_get_register(&self, index: usize) -> u32 {
        if index < 8 {
            self.d_regs[index]
        } else {
            self.a_regs[index - 8]
        }
    }

    fn set_shift_rotate_result(&mut self, reg: usize, size: u8, value: u32) {
        match size {
            0b00 => {
                self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_FF00) | (value & 0xFF);
            }
            0b01 => {
                self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_0000) | (value & 0xFFFF);
            }
            0b10 => {
                self.d_regs[reg] = value;
            }
            _ => {}
        }
    }

    fn movem_set_register(&mut self, index: usize, value: u32) {
        if index < 8 {
            self.d_regs[index] = value;
        } else {
            self.a_regs[index - 8] = value;
        }
    }

    fn exec_rts(&mut self, memory: &mut MemoryMap) -> u32 {
        self.pc = self.pop_u32(memory);
        16
    }

    fn exec_rte(&mut self, memory: &mut MemoryMap) -> u32 {
        if (self.sr & SR_SUPERVISOR) == 0 {
            self.raise_exception(8, memory, None);
            return 34;
        }

        let restored_sr = self.pop_u16(memory);
        self.pc = self.pop_u32(memory);
        self.write_sr(restored_sr);
        20
    }

    fn exec_branch(&mut self, opcode: u16, memory: &mut MemoryMap, cond: u8) -> u32 {
        let displacement = (opcode & 0x00FF) as u8;
        let should_branch = self.condition_true(cond);
        if displacement == 0 {
            let base_pc = self.pc;
            let disp16 = self.fetch_u16(memory) as i16 as i32;
            if should_branch {
                self.pc = base_pc.wrapping_add_signed(disp16);
            }
        } else if should_branch {
            let disp8 = displacement as i8 as i32;
            self.pc = self.pc.wrapping_add_signed(disp8);
        }
        10
    }

    fn exec_bcc(&mut self, opcode: u16, memory: &mut MemoryMap) -> Option<u32> {
        let cond = ((opcode >> 8) & 0xF) as u8;
        if cond == 0x0 || cond == 0x1 {
            return None;
        }
        Some(self.exec_branch(opcode, memory, cond))
    }

    fn exec_bsr(&mut self, opcode: u16, memory: &mut MemoryMap) -> u32 {
        let displacement = (opcode & 0x00FF) as u8;
        if displacement == 0 {
            let base_pc = self.pc;
            let disp16 = self.fetch_u16(memory) as i16 as i32;
            let return_addr = self.pc;
            self.push_u32(memory, return_addr);
            self.pc = base_pc.wrapping_add_signed(disp16);
        } else {
            let return_addr = self.pc;
            self.push_u32(memory, return_addr);
            let disp8 = displacement as i8 as i32;
            self.pc = self.pc.wrapping_add_signed(disp8);
        }
        18
    }

    fn exec_trap(&mut self, opcode: u16, memory: &mut MemoryMap) -> u32 {
        let vector = 32 + (opcode as u32 & 0x0F);
        self.raise_exception(vector, memory, None);
        34
    }

    fn exec_illegal(&mut self, memory: &mut MemoryMap) -> u32 {
        self.raise_exception(4, memory, None);
        34
    }

    fn service_interrupt(&mut self, level: u8, memory: &mut MemoryMap) -> bool {
        if !(1..=7).contains(&level) {
            return false;
        }
        let current_mask = ((self.sr & SR_INT_MASK) >> 8) as u8;
        if level <= current_mask {
            return false;
        }

        self.raise_exception(24 + level as u32, memory, Some(level));
        true
    }

    fn raise_exception(
        &mut self,
        vector: u32,
        memory: &mut MemoryMap,
        interrupt_level: Option<u8>,
    ) {
        *self.exception_histogram.entry(vector).or_insert(0) += 1;
        let old_sr = self.sr;

        // Exceptions always stack on the supervisor stack.
        if (self.sr & SR_SUPERVISOR) == 0 {
            self.usp = self.a_regs[7];
            self.a_regs[7] = self.ssp;
        }

        self.push_u32(memory, self.pc);
        self.push_u16(memory, old_sr);
        self.ssp = self.a_regs[7];

        self.sr = old_sr | SR_SUPERVISOR;
        if let Some(level) = interrupt_level {
            self.sr = (self.sr & !SR_INT_MASK) | ((level as u16) << 8);
        }

        let vector_addr = vector * 4;
        self.pc = memory.read_u32(vector_addr);
    }

    fn condition_true(&self, cond: u8) -> bool {
        let n = self.flag_set(CCR_N);
        let z = self.flag_set(CCR_Z);
        let v = self.flag_set(CCR_V);
        let c = self.flag_set(CCR_C);
        match cond & 0xF {
            0x0 => true,
            0x1 => false,
            0x2 => !c && !z,
            0x3 => c || z,
            0x4 => !c,
            0x5 => c,
            0x6 => !z,
            0x7 => z,
            0x8 => !v,
            0x9 => v,
            0xA => !n,
            0xB => n,
            0xC => n == v,
            0xD => n != v,
            0xE => !z && (n == v),
            0xF => z || (n != v),
            _ => unreachable!(),
        }
    }

    fn resolve_data_alterable_address(
        &mut self,
        mode: u8,
        reg: usize,
        size_bytes: u32,
        memory: &mut MemoryMap,
    ) -> Option<u32> {
        let addr_step = if size_bytes == 1 {
            self.byte_addr_step(reg)
        } else {
            size_bytes
        };
        match mode {
            0b010 => Some(self.a_regs[reg]),
            0b011 => {
                let addr = self.a_regs[reg];
                self.a_regs[reg] = self.a_regs[reg].wrapping_add(addr_step);
                Some(addr)
            }
            0b100 => {
                self.a_regs[reg] = self.a_regs[reg].wrapping_sub(addr_step);
                Some(self.a_regs[reg])
            }
            0b101 => {
                let disp = self.fetch_u16(memory) as i16 as i32;
                Some(self.a_regs[reg].wrapping_add_signed(disp))
            }
            0b110 => Some(self.resolve_indexed_address(self.a_regs[reg], memory)),
            0b111 => match reg {
                0b000 => Some(self.fetch_u16(memory) as i16 as i32 as u32),
                0b001 => Some(self.fetch_u32(memory)),
                _ => None,
            },
            _ => None,
        }
    }

    fn read_ea_byte(&mut self, mode: u8, reg: usize, memory: &mut MemoryMap) -> Option<u8> {
        match mode {
            0b000 => Some(self.d_regs[reg] as u8),
            0b001 => None,
            0b010 => Some(memory.read_u8(self.a_regs[reg])),
            0b011 => {
                let addr = self.a_regs[reg];
                let value = memory.read_u8(addr);
                self.a_regs[reg] = self.a_regs[reg].wrapping_add(self.byte_addr_step(reg));
                Some(value)
            }
            0b100 => {
                self.a_regs[reg] = self.a_regs[reg].wrapping_sub(self.byte_addr_step(reg));
                Some(memory.read_u8(self.a_regs[reg]))
            }
            0b101 => {
                let disp = self.fetch_u16(memory) as i16 as i32;
                let addr = self.a_regs[reg].wrapping_add_signed(disp);
                Some(memory.read_u8(addr))
            }
            0b110 => {
                let addr = self.resolve_indexed_address(self.a_regs[reg], memory);
                Some(memory.read_u8(addr))
            }
            0b111 => match reg {
                0b000 => {
                    let addr = self.fetch_u16(memory) as i16 as i32 as u32;
                    Some(memory.read_u8(addr))
                }
                0b001 => {
                    let addr = self.fetch_u32(memory);
                    Some(memory.read_u8(addr))
                }
                0b010 => {
                    let base_pc = self.pc;
                    let disp = self.fetch_u16(memory) as i16 as i32;
                    Some(memory.read_u8(base_pc.wrapping_add_signed(disp)))
                }
                0b011 => {
                    let addr = self.resolve_pc_indexed_address(memory);
                    Some(memory.read_u8(addr))
                }
                0b100 => Some(self.fetch_u16(memory) as u8),
                _ => None,
            },
            _ => None,
        }
    }

    fn write_ea_byte(
        &mut self,
        mode: u8,
        reg: usize,
        value: u8,
        memory: &mut MemoryMap,
    ) -> Option<()> {
        match mode {
            0b000 => {
                self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_FF00) | value as u32;
                Some(())
            }
            0b001 => None,
            0b010 => {
                memory.write_u8(self.a_regs[reg], value);
                Some(())
            }
            0b011 => {
                let addr = self.a_regs[reg];
                memory.write_u8(addr, value);
                self.a_regs[reg] = self.a_regs[reg].wrapping_add(self.byte_addr_step(reg));
                Some(())
            }
            0b100 => {
                self.a_regs[reg] = self.a_regs[reg].wrapping_sub(self.byte_addr_step(reg));
                memory.write_u8(self.a_regs[reg], value);
                Some(())
            }
            0b101 => {
                let disp = self.fetch_u16(memory) as i16 as i32;
                let addr = self.a_regs[reg].wrapping_add_signed(disp);
                memory.write_u8(addr, value);
                Some(())
            }
            0b110 => {
                let addr = self.resolve_indexed_address(self.a_regs[reg], memory);
                memory.write_u8(addr, value);
                Some(())
            }
            0b111 => match reg {
                0b000 => {
                    let addr = self.fetch_u16(memory) as i16 as i32 as u32;
                    memory.write_u8(addr, value);
                    Some(())
                }
                0b001 => {
                    let addr = self.fetch_u32(memory);
                    memory.write_u8(addr, value);
                    Some(())
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn read_ea_word(&mut self, mode: u8, reg: usize, memory: &mut MemoryMap) -> Option<u16> {
        match mode {
            0b000 => Some(self.d_regs[reg] as u16),
            0b001 => Some(self.a_regs[reg] as u16),
            0b010 => Some(memory.read_u16(self.a_regs[reg])),
            0b011 => {
                let addr = self.a_regs[reg];
                let value = memory.read_u16(addr);
                self.a_regs[reg] = self.a_regs[reg].wrapping_add(2);
                Some(value)
            }
            0b100 => {
                self.a_regs[reg] = self.a_regs[reg].wrapping_sub(2);
                Some(memory.read_u16(self.a_regs[reg]))
            }
            0b101 => {
                let disp = self.fetch_u16(memory) as i16 as i32;
                let addr = self.a_regs[reg].wrapping_add_signed(disp);
                Some(memory.read_u16(addr))
            }
            0b110 => {
                let addr = self.resolve_indexed_address(self.a_regs[reg], memory);
                Some(memory.read_u16(addr))
            }
            0b111 => match reg {
                0b000 => {
                    let addr = self.fetch_u16(memory) as i16 as i32 as u32;
                    Some(memory.read_u16(addr))
                }
                0b001 => {
                    let addr = self.fetch_u32(memory);
                    Some(memory.read_u16(addr))
                }
                0b010 => {
                    let base_pc = self.pc;
                    let disp = self.fetch_u16(memory) as i16 as i32;
                    Some(memory.read_u16(base_pc.wrapping_add_signed(disp)))
                }
                0b011 => {
                    let addr = self.resolve_pc_indexed_address(memory);
                    Some(memory.read_u16(addr))
                }
                0b100 => Some(self.fetch_u16(memory)),
                _ => None,
            },
            _ => None,
        }
    }

    fn read_ea_long(&mut self, mode: u8, reg: usize, memory: &mut MemoryMap) -> Option<u32> {
        match mode {
            0b000 => Some(self.d_regs[reg]),
            0b001 => Some(self.a_regs[reg]),
            0b010 => Some(memory.read_u32(self.a_regs[reg])),
            0b011 => {
                let addr = self.a_regs[reg];
                let value = memory.read_u32(addr);
                self.a_regs[reg] = self.a_regs[reg].wrapping_add(4);
                Some(value)
            }
            0b100 => {
                self.a_regs[reg] = self.a_regs[reg].wrapping_sub(4);
                Some(memory.read_u32(self.a_regs[reg]))
            }
            0b101 => {
                let disp = self.fetch_u16(memory) as i16 as i32;
                let addr = self.a_regs[reg].wrapping_add_signed(disp);
                Some(memory.read_u32(addr))
            }
            0b110 => {
                let addr = self.resolve_indexed_address(self.a_regs[reg], memory);
                Some(memory.read_u32(addr))
            }
            0b111 => match reg {
                0b000 => {
                    let addr = self.fetch_u16(memory) as i16 as i32 as u32;
                    Some(memory.read_u32(addr))
                }
                0b001 => {
                    let addr = self.fetch_u32(memory);
                    Some(memory.read_u32(addr))
                }
                0b010 => {
                    let base_pc = self.pc;
                    let disp = self.fetch_u16(memory) as i16 as i32;
                    Some(memory.read_u32(base_pc.wrapping_add_signed(disp)))
                }
                0b011 => {
                    let addr = self.resolve_pc_indexed_address(memory);
                    Some(memory.read_u32(addr))
                }
                0b100 => Some(self.fetch_u32(memory)),
                _ => None,
            },
            _ => None,
        }
    }

    fn write_ea_word(
        &mut self,
        mode: u8,
        reg: usize,
        value: u16,
        memory: &mut MemoryMap,
    ) -> Option<()> {
        match mode {
            0b000 => {
                self.d_regs[reg] = (self.d_regs[reg] & 0xFFFF_0000) | value as u32;
                Some(())
            }
            0b010 => {
                memory.write_u16(self.a_regs[reg], value);
                Some(())
            }
            0b011 => {
                let addr = self.a_regs[reg];
                memory.write_u16(addr, value);
                self.a_regs[reg] = self.a_regs[reg].wrapping_add(2);
                Some(())
            }
            0b100 => {
                self.a_regs[reg] = self.a_regs[reg].wrapping_sub(2);
                memory.write_u16(self.a_regs[reg], value);
                Some(())
            }
            0b101 => {
                let disp = self.fetch_u16(memory) as i16 as i32;
                let addr = self.a_regs[reg].wrapping_add_signed(disp);
                memory.write_u16(addr, value);
                Some(())
            }
            0b110 => {
                let addr = self.resolve_indexed_address(self.a_regs[reg], memory);
                memory.write_u16(addr, value);
                Some(())
            }
            0b111 => match reg {
                0b000 => {
                    let addr = self.fetch_u16(memory) as i16 as i32 as u32;
                    memory.write_u16(addr, value);
                    Some(())
                }
                0b001 => {
                    let addr = self.fetch_u32(memory);
                    memory.write_u16(addr, value);
                    Some(())
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn write_ea_long(
        &mut self,
        mode: u8,
        reg: usize,
        value: u32,
        memory: &mut MemoryMap,
    ) -> Option<()> {
        match mode {
            0b000 => {
                self.d_regs[reg] = value;
                Some(())
            }
            0b010 => {
                memory.write_u32(self.a_regs[reg], value);
                Some(())
            }
            0b011 => {
                let addr = self.a_regs[reg];
                memory.write_u32(addr, value);
                self.a_regs[reg] = self.a_regs[reg].wrapping_add(4);
                Some(())
            }
            0b100 => {
                self.a_regs[reg] = self.a_regs[reg].wrapping_sub(4);
                memory.write_u32(self.a_regs[reg], value);
                Some(())
            }
            0b101 => {
                let disp = self.fetch_u16(memory) as i16 as i32;
                let addr = self.a_regs[reg].wrapping_add_signed(disp);
                memory.write_u32(addr, value);
                Some(())
            }
            0b110 => {
                let addr = self.resolve_indexed_address(self.a_regs[reg], memory);
                memory.write_u32(addr, value);
                Some(())
            }
            0b111 => match reg {
                0b000 => {
                    let addr = self.fetch_u16(memory) as i16 as i32 as u32;
                    memory.write_u32(addr, value);
                    Some(())
                }
                0b001 => {
                    let addr = self.fetch_u32(memory);
                    memory.write_u32(addr, value);
                    Some(())
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn fetch_u16(&mut self, memory: &mut MemoryMap) -> u16 {
        let value = memory.read_u16(self.pc);
        self.pc = self.pc.wrapping_add(2);
        value
    }

    fn fetch_u32(&mut self, memory: &mut MemoryMap) -> u32 {
        let value = memory.read_u32(self.pc);
        self.pc = self.pc.wrapping_add(4);
        value
    }

    fn resolve_control_address(
        &mut self,
        mode: u8,
        reg: usize,
        memory: &mut MemoryMap,
    ) -> Option<u32> {
        match mode {
            0b010 => Some(self.a_regs[reg]),
            0b101 => {
                let disp = self.fetch_u16(memory) as i16 as i32;
                Some(self.a_regs[reg].wrapping_add_signed(disp))
            }
            0b110 => Some(self.resolve_indexed_address(self.a_regs[reg], memory)),
            0b111 => match reg {
                0b000 => Some(self.fetch_u16(memory) as i16 as i32 as u32),
                0b001 => Some(self.fetch_u32(memory)),
                0b010 => {
                    let base_pc = self.pc;
                    let disp = self.fetch_u16(memory) as i16 as i32;
                    Some(base_pc.wrapping_add_signed(disp))
                }
                0b011 => Some(self.resolve_pc_indexed_address(memory)),
                _ => None,
            },
            _ => None,
        }
    }

    fn resolve_indexed_address(&mut self, base: u32, memory: &mut MemoryMap) -> u32 {
        let ext = self.fetch_u16(memory);
        self.resolve_indexed_address_with_ext(base, ext)
    }

    fn resolve_pc_indexed_address(&mut self, memory: &mut MemoryMap) -> u32 {
        let base_pc = self.pc;
        let ext = self.fetch_u16(memory);
        self.resolve_indexed_address_with_ext(base_pc, ext)
    }

    fn resolve_indexed_address_with_ext(&self, base: u32, ext: u16) -> u32 {
        let displacement = (ext & 0x00FF) as u8 as i8 as i32;
        let index_reg = ((ext >> 12) & 0x7) as usize;
        let index_is_addr = (ext & 0x8000) != 0;
        let index_is_long = (ext & 0x0800) != 0;

        let index_value = if index_is_addr {
            self.a_regs[index_reg]
        } else {
            self.d_regs[index_reg]
        };
        let index_offset = if index_is_long {
            index_value as i32
        } else {
            index_value as u16 as i16 as i32
        };

        base.wrapping_add_signed(displacement)
            .wrapping_add_signed(index_offset)
    }

    fn byte_addr_step(&self, reg: usize) -> u32 {
        if reg == 7 { 2 } else { 1 }
    }

    fn push_u32(&mut self, memory: &mut MemoryMap, value: u32) {
        self.a_regs[7] = self.a_regs[7].wrapping_sub(4);
        memory.write_u32(self.a_regs[7], value);
    }

    fn push_u16(&mut self, memory: &mut MemoryMap, value: u16) {
        self.a_regs[7] = self.a_regs[7].wrapping_sub(2);
        memory.write_u16(self.a_regs[7], value);
    }

    fn pop_u32(&mut self, memory: &mut MemoryMap) -> u32 {
        let value = memory.read_u32(self.a_regs[7]);
        self.a_regs[7] = self.a_regs[7].wrapping_add(4);
        value
    }

    fn pop_u16(&mut self, memory: &mut MemoryMap) -> u16 {
        let value = memory.read_u16(self.a_regs[7]);
        self.a_regs[7] = self.a_regs[7].wrapping_add(2);
        value
    }

    fn update_move_flags_word(&mut self, value: u16) {
        self.set_flag(CCR_N, (value & 0x8000) != 0);
        self.set_flag(CCR_Z, value == 0);
        self.set_flag(CCR_V, false);
        self.set_flag(CCR_C, false);
    }

    fn update_move_flags_byte(&mut self, value: u8) {
        self.set_flag(CCR_N, (value & 0x80) != 0);
        self.set_flag(CCR_Z, value == 0);
        self.set_flag(CCR_V, false);
        self.set_flag(CCR_C, false);
    }

    fn update_move_flags_long(&mut self, value: u32) {
        self.set_flag(CCR_N, (value & 0x8000_0000) != 0);
        self.set_flag(CCR_Z, value == 0);
        self.set_flag(CCR_V, false);
        self.set_flag(CCR_C, false);
    }

    fn update_test_flags_word(&mut self, value: u16) {
        self.set_flag(CCR_N, (value & 0x8000) != 0);
        self.set_flag(CCR_Z, value == 0);
        self.set_flag(CCR_V, false);
        self.set_flag(CCR_C, false);
    }

    fn update_test_flags_byte(&mut self, value: u8) {
        self.set_flag(CCR_N, (value & 0x80) != 0);
        self.set_flag(CCR_Z, value == 0);
        self.set_flag(CCR_V, false);
        self.set_flag(CCR_C, false);
    }

    fn update_add_flags_byte(&mut self, result: u8, carry: bool, overflow: bool) {
        self.set_flag(CCR_N, (result & 0x80) != 0);
        self.set_flag(CCR_Z, result == 0);
        self.set_flag(CCR_V, overflow);
        self.set_flag(CCR_C, carry);
    }

    fn update_add_flags_word(&mut self, result: u16, carry: bool, overflow: bool) {
        self.set_flag(CCR_N, (result & 0x8000) != 0);
        self.set_flag(CCR_Z, result == 0);
        self.set_flag(CCR_V, overflow);
        self.set_flag(CCR_C, carry);
    }

    fn update_add_flags_long(&mut self, result: u32, carry: bool, overflow: bool) {
        self.set_flag(CCR_N, (result & 0x8000_0000) != 0);
        self.set_flag(CCR_Z, result == 0);
        self.set_flag(CCR_V, overflow);
        self.set_flag(CCR_C, carry);
    }

    fn update_add_flags_byte_with_extend(&mut self, result: u8, carry: bool, overflow: bool) {
        self.update_add_flags_byte(result, carry, overflow);
        self.set_flag(CCR_X, carry);
    }

    fn update_add_flags_word_with_extend(&mut self, result: u16, carry: bool, overflow: bool) {
        self.update_add_flags_word(result, carry, overflow);
        self.set_flag(CCR_X, carry);
    }

    fn update_add_flags_long_with_extend(&mut self, result: u32, carry: bool, overflow: bool) {
        self.update_add_flags_long(result, carry, overflow);
        self.set_flag(CCR_X, carry);
    }

    fn update_sub_flags_byte(&mut self, dst: u8, src: u8, result: u8) {
        self.set_flag(CCR_N, (result & 0x80) != 0);
        self.set_flag(CCR_Z, result == 0);
        self.set_flag(CCR_V, ((dst ^ src) & (dst ^ result) & 0x80) != 0);
        self.set_flag(CCR_C, src > dst);
    }

    fn update_test_flags_long(&mut self, value: u32) {
        self.set_flag(CCR_N, (value & 0x8000_0000) != 0);
        self.set_flag(CCR_Z, value == 0);
        self.set_flag(CCR_V, false);
        self.set_flag(CCR_C, false);
    }

    fn update_sub_flags_word(&mut self, dst: u16, src: u16, result: u16) {
        self.set_flag(CCR_N, (result & 0x8000) != 0);
        self.set_flag(CCR_Z, result == 0);
        self.set_flag(CCR_V, ((dst ^ src) & (dst ^ result) & 0x8000) != 0);
        self.set_flag(CCR_C, src > dst);
    }

    fn update_sub_flags_long(&mut self, dst: u32, src: u32, result: u32) {
        self.set_flag(CCR_N, (result & 0x8000_0000) != 0);
        self.set_flag(CCR_Z, result == 0);
        self.set_flag(CCR_V, ((dst ^ src) & (dst ^ result) & 0x8000_0000) != 0);
        self.set_flag(CCR_C, src > dst);
    }

    fn update_sub_flags_byte_with_extend(&mut self, dst: u8, src: u8, result: u8) {
        self.update_sub_flags_byte(dst, src, result);
        self.set_flag(CCR_X, src > dst);
    }

    fn update_sub_flags_word_with_extend(&mut self, dst: u16, src: u16, result: u16) {
        self.update_sub_flags_word(dst, src, result);
        self.set_flag(CCR_X, src > dst);
    }

    fn update_sub_flags_long_with_extend(&mut self, dst: u32, src: u32, result: u32) {
        self.update_sub_flags_long(dst, src, result);
        self.set_flag(CCR_X, src > dst);
    }

    fn flag_set(&self, flag: u16) -> bool {
        (self.sr & flag) != 0
    }

    fn set_flag(&mut self, flag: u16, enabled: bool) {
        if enabled {
            self.sr |= flag;
        } else {
            self.sr &= !flag;
        }
    }

    fn write_sr(&mut self, value: u16) {
        let old_supervisor = (self.sr & SR_SUPERVISOR) != 0;
        let new_supervisor = (value & SR_SUPERVISOR) != 0;
        if old_supervisor != new_supervisor {
            if old_supervisor {
                self.ssp = self.a_regs[7];
                self.a_regs[7] = self.usp;
            } else {
                self.usp = self.a_regs[7];
                self.a_regs[7] = self.ssp;
            }
        }
        self.sr = value;
    }

    fn record_unknown_opcode(&mut self, opcode: u16, pc: u32) {
        self.unknown_opcode_total += 1;
        *self.unknown_opcode_histogram.entry(opcode).or_insert(0) += 1;
        *self.unknown_opcode_pc_histogram.entry(pc).or_insert(0) += 1;
    }
}

#[derive(Debug, Clone, Copy)]
enum ArithOp {
    Add,
    Sub,
}

#[derive(Debug, Clone, Copy)]
enum LogicOp {
    And,
    Or,
}

#[derive(Debug, Clone, Copy)]
enum ImmStore {
    DnByte(usize),
    DnWord(usize),
    DnLong(usize),
    MemByte(u32),
    MemWord(u32),
    MemLong(u32),
}

#[cfg(test)]
mod tests {
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
        // moveq #1, d0
        rom[0x106..0x108].copy_from_slice(&0x7001u16.to_be_bytes());
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

        assert_eq!(memory.read_u16(0x00FF_0011), 0x0001);
        assert_eq!(cpu.d_regs[1] & 0xFFFF, 0x0001);
        assert_eq!(cpu.d_regs[2] & 0xFFFF, 0x0001);
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
}
