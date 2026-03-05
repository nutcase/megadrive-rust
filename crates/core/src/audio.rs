use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Eq, bincode::Encode, bincode::Decode)]
enum YmEnvelopePhase {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy, bincode::Encode, bincode::Decode)]
struct YmOperator {
    detune: u8,
    mul: u8,
    tl: u8,
    key_scale: u8,
    am_enable: bool,
    ssg_eg: u8,
    ssg_invert: bool,
    ssg_hold_active: bool,
    attack_rate: u8,
    decay_rate: u8,
    sustain_rate: u8,
    sustain_level: u8,
    release_rate: u8,
    key_on: bool,
    phase: f32,
    envelope_phase: YmEnvelopePhase,
    envelope_level: f32,
    last_output: f32,
}

impl Default for YmOperator {
    fn default() -> Self {
        Self {
            detune: 0,
            mul: 1,
            tl: 0,
            key_scale: 0,
            am_enable: false,
            ssg_eg: 0,
            ssg_invert: false,
            ssg_hold_active: false,
            attack_rate: 31,
            decay_rate: 0,
            sustain_rate: 0,
            sustain_level: 0,
            // Keep default release short to avoid lingering notes when a game
            // hasn't initialized operator envelopes yet.
            release_rate: 15,
            key_on: false,
            phase: 0.0,
            envelope_phase: YmEnvelopePhase::Off,
            envelope_level: 0.0,
            last_output: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, bincode::Encode, bincode::Decode)]
struct YmChannel {
    fnum: u16,
    block: u8,
    special_fnum: [u16; 3],
    special_block: [u8; 3],
    algorithm: u8,
    feedback: u8,
    feedback_sample: f32,
    feedback_sample_prev: f32,
    pan_left: bool,
    pan_right: bool,
    ams: u8,
    fms: u8,
    operators: [YmOperator; 4],
}

impl Default for YmChannel {
    fn default() -> Self {
        Self {
            fnum: 0x200,
            block: 4,
            special_fnum: [0x200; 3],
            special_block: [4; 3],
            algorithm: 0,
            feedback: 0,
            feedback_sample: 0.0,
            feedback_sample_prev: 0.0,
            pan_left: true,
            pan_right: true,
            ams: 0,
            fms: 0,
            operators: [YmOperator::default(); 4],
        }
    }
}

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct Ym2612 {
    addr_port0: u8,
    addr_port1: u8,
    regs: [[u8; 256]; 2],
    writes: u64,
    dac_data_writes: u64,
    busy_z80_cycles: u32,
    timer_status: u8,
    timer_control: u8,
    timer_a_value: u16,
    timer_b_value: u8,
    timer_clock_accumulator: u64,
    timer_a_elapsed_ym_cycles: u64,
    timer_b_elapsed_ym_cycles: u64,
    dac_enabled: bool,
    dac_output: i16,
    dac_enabled_pending: Option<bool>,
    dac_output_pending: Option<i16>,
    lfo_enabled: bool,
    lfo_rate: u8,
    lfo_phase: f32,
    channels: [YmChannel; 6],
}

impl Default for Ym2612 {
    fn default() -> Self {
        Self {
            addr_port0: 0,
            addr_port1: 0,
            regs: [[0; 256]; 2],
            writes: 0,
            dac_data_writes: 0,
            busy_z80_cycles: 0,
            timer_status: 0,
            timer_control: 0,
            timer_a_value: 0,
            timer_b_value: 0,
            timer_clock_accumulator: 0,
            timer_a_elapsed_ym_cycles: 0,
            timer_b_elapsed_ym_cycles: 0,
            dac_enabled: false,
            dac_output: 0,
            dac_enabled_pending: None,
            dac_output_pending: None,
            lfo_enabled: false,
            lfo_rate: 0,
            lfo_phase: 0.0,
            channels: [YmChannel::default(); 6],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, bincode::Encode, bincode::Decode)]
enum YmOperatorParam {
    Mul,
    Tl,
    Attack,
    Decay,
    SustainRate,
    SustainRelease,
    SsgEg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum YmAlgorithmBus {
    M2,
    C1,
    C2,
    Mem,
    Out,
}

impl Ym2612 {
    // YM2612 BUSY stays asserted for roughly 32 master clocks after a write.
    // Converting directly from master-clock cycles matches observed software
    // pacing better than using OPN internal divider cycles.
    const BUSY_DURATION_MASTER_CYCLES: u64 = 32;
    const MASTER_CLOCK_HZ: u64 = 7_670_454;
    const Z80_CLOCK_HZ: u64 = 3_579_545;
    const YM2612_DIVIDER: u64 = 7;
    const BUSY_DURATION_Z80_CYCLES: u32 = ((Self::BUSY_DURATION_MASTER_CYCLES * Self::Z80_CLOCK_HZ
        + (Self::MASTER_CLOCK_HZ - 1))
        / Self::MASTER_CLOCK_HZ) as u32;
    // YM2612 DAC raw 8-bit data maps to a signed range centered at 0x80.
    // Keep output in a moderate range relative to FM mix to avoid clipping.
    const DAC_OUTPUT_SHIFT: i16 = 6;
    // DAC pending output stores a 1-bit ordering tag in bit0.
    // Actual DAC output values are multiples of 64, so bit0 is unused.
    const DAC_PENDING_ORDER_MASK: i16 = 0x0001;

    fn write_port(&mut self, port: u8, value: u8) {
        self.write_port_internal(port, value, false);
    }

    fn write_port_from_z80(&mut self, port: u8, value: u8) {
        self.write_port_internal(port, value, true);
    }

    fn write_port_internal(&mut self, port: u8, value: u8, from_z80: bool) {
        match port & 0x03 {
            0 => {
                self.addr_port0 = value;
                self.arm_busy();
            }
            1 => {
                let reg = self.addr_port0;
                self.regs[0][reg as usize] = value;
                self.apply_write(0, reg, value, from_z80);
                self.writes += 1;
                self.arm_busy();
            }
            2 => {
                self.addr_port1 = value;
                self.arm_busy();
            }
            3 => {
                let reg = self.addr_port1;
                self.regs[1][reg as usize] = value;
                self.apply_write(1, reg, value, from_z80);
                self.writes += 1;
                self.arm_busy();
            }
            _ => {}
        }
    }

    fn arm_busy(&mut self) {
        self.busy_z80_cycles = Self::BUSY_DURATION_Z80_CYCLES;
    }

    fn apply_write(&mut self, bank: usize, reg: u8, value: u8, from_z80: bool) {
        if let Some(channel) = self.decode_fnum_low_channel(bank, reg) {
            self.channels[channel].fnum = (self.channels[channel].fnum & 0x0700) | value as u16;
        } else if let Some(channel) = self.decode_fnum_high_channel(bank, reg) {
            self.channels[channel].fnum =
                (self.channels[channel].fnum & 0x00FF) | (((value & 0x07) as u16) << 8);
            self.channels[channel].block = (value >> 3) & 0x07;
        } else if let Some(slot) = self.decode_channel3_special_low(bank, reg) {
            let channel = &mut self.channels[2];
            channel.special_fnum[slot] = (channel.special_fnum[slot] & 0x0700) | value as u16;
        } else if let Some(slot) = self.decode_channel3_special_high(bank, reg) {
            let channel = &mut self.channels[2];
            channel.special_fnum[slot] =
                (channel.special_fnum[slot] & 0x00FF) | (((value & 0x07) as u16) << 8);
            channel.special_block[slot] = (value >> 3) & 0x07;
        } else if let Some(channel) = self.decode_pan_channel(bank, reg) {
            self.channels[channel].pan_left = (value & 0x80) != 0;
            self.channels[channel].pan_right = (value & 0x40) != 0;
            self.channels[channel].ams = (value >> 4) & 0x03;
            self.channels[channel].fms = value & 0x07;
        } else if let Some(channel) = self.decode_algorithm_channel(bank, reg) {
            self.channels[channel].algorithm = value & 0x07;
            self.channels[channel].feedback = (value >> 3) & 0x07;
        } else if let Some((channel, slot, param)) = Self::decode_operator_target(bank, reg) {
            let op = &mut self.channels[channel].operators[slot];
            match param {
                YmOperatorParam::Mul => {
                    op.detune = (value >> 4) & 0x07;
                    op.mul = value & 0x0F;
                }
                YmOperatorParam::Tl => {
                    op.tl = value & 0x7F;
                }
                YmOperatorParam::Attack => {
                    op.key_scale = (value >> 6) & 0x03;
                    op.attack_rate = value & 0x1F;
                }
                YmOperatorParam::Decay => {
                    op.am_enable = (value & 0x80) != 0;
                    op.decay_rate = value & 0x1F;
                }
                YmOperatorParam::SustainRate => {
                    op.sustain_rate = value & 0x1F;
                }
                YmOperatorParam::SustainRelease => {
                    op.sustain_level = (value >> 4) & 0x0F;
                    op.release_rate = value & 0x0F;
                }
                YmOperatorParam::SsgEg => {
                    op.ssg_eg = value & 0x0F;
                    if (op.ssg_eg & 0x08) == 0 {
                        op.ssg_invert = false;
                        op.ssg_hold_active = false;
                    }
                }
            }
        }

        if bank == 0 {
            match reg {
                0x22 => {
                    self.lfo_enabled = (value & 0x08) != 0;
                    self.lfo_rate = value & 0x07;
                    if !self.lfo_enabled {
                        self.lfo_phase = 0.0;
                    }
                }
                0x24 => {
                    self.timer_a_value = (self.timer_a_value & 0x0003) | ((value as u16) << 2);
                }
                0x25 => {
                    self.timer_a_value = (self.timer_a_value & 0x03FC) | ((value as u16) & 0x03);
                }
                0x26 => {
                    self.timer_b_value = value;
                }
                0x27 => {
                    self.timer_control = value;
                    if (value & 0x10) != 0 {
                        self.timer_status &= !0x01;
                    }
                    if (value & 0x20) != 0 {
                        self.timer_status &= !0x02;
                    }
                    if (value & 0x01) != 0 {
                        self.timer_a_elapsed_ym_cycles = 0;
                    }
                    if (value & 0x02) != 0 {
                        self.timer_b_elapsed_ym_cycles = 0;
                    }
                }
                0x28 => {
                    if let Some(channel) = Self::decode_keyon_channel(value) {
                        let mut reset_feedback = false;
                        let slot_mask = (value >> 4) & 0x0F;
                        for op_index in 0..4 {
                            let next_key_on =
                                Self::keyon_slot_mask_targets_operator(slot_mask, op_index);
                            let op = &mut self.channels[channel].operators[op_index];
                            if next_key_on && !op.key_on {
                                op.phase = 0.0;
                                op.last_output = 0.0;
                                op.envelope_phase = YmEnvelopePhase::Attack;
                                op.envelope_level = 0.0;
                                op.ssg_invert = false;
                                op.ssg_hold_active = false;
                                if op_index == 0 {
                                    reset_feedback = true;
                                }
                            } else if !next_key_on && op.key_on {
                                op.envelope_phase = if op.envelope_level > 0.0 {
                                    YmEnvelopePhase::Release
                                } else {
                                    YmEnvelopePhase::Off
                                };
                                op.ssg_hold_active = false;
                            }
                            op.key_on = next_key_on;
                        }
                        if reset_feedback {
                            // `feedback_sample` keeps OP1 feedback history and
                            // `feedback_sample_prev` keeps YM MEM delayed bus.
                            self.channels[channel].feedback_sample = 0.0;
                            self.channels[channel].feedback_sample_prev = 0.0;
                        }
                    }
                }
                0x2A => {
                    let centered = value as i16 - 0x80;
                    let output = centered << Self::DAC_OUTPUT_SHIFT;
                    if from_z80 {
                        let output_after_enable = self.dac_enabled_pending.is_some();
                        self.dac_output_pending =
                            Some(Self::encode_pending_dac_output(output, output_after_enable));
                    } else {
                        self.dac_output = output;
                    }
                    self.dac_data_writes += 1;
                }
                0x2B => {
                    let enabled = (value & 0x80) != 0;
                    if from_z80 {
                        self.dac_enabled_pending = Some(enabled);
                    } else {
                        self.set_dac_enabled(enabled);
                    }
                }
                _ => {}
            }
        }
    }

    fn decode_fnum_low_channel(&self, bank: usize, reg: u8) -> Option<usize> {
        if (0xA0..=0xA2).contains(&reg) {
            Some((bank & 1) * 3 + (reg as usize - 0xA0))
        } else {
            None
        }
    }

    fn decode_fnum_high_channel(&self, bank: usize, reg: u8) -> Option<usize> {
        if (0xA4..=0xA6).contains(&reg) {
            Some((bank & 1) * 3 + (reg as usize - 0xA4))
        } else {
            None
        }
    }

    fn decode_channel3_special_low(&self, bank: usize, reg: u8) -> Option<usize> {
        if bank == 0 && (0xA8..=0xAA).contains(&reg) {
            Some((reg - 0xA8) as usize)
        } else {
            None
        }
    }

    fn decode_channel3_special_high(&self, bank: usize, reg: u8) -> Option<usize> {
        if bank == 0 && (0xAC..=0xAE).contains(&reg) {
            Some((reg - 0xAC) as usize)
        } else {
            None
        }
    }

    fn decode_keyon_channel(value: u8) -> Option<usize> {
        match value & 0x07 {
            0 => Some(0),
            1 => Some(1),
            2 => Some(2),
            4 => Some(3),
            5 => Some(4),
            6 => Some(5),
            _ => None,
        }
    }

    fn keyon_slot_mask_targets_operator(slot_mask: u8, op_index: usize) -> bool {
        // YM2612 key-on bits: b4=OP1, b5=OP2, b6=OP3, b7=OP4.
        match op_index.min(3) {
            0 => (slot_mask & 0b0001) != 0, // OP1 (b4)
            1 => (slot_mask & 0b0010) != 0, // OP2 (b5)
            2 => (slot_mask & 0b0100) != 0, // OP3 (b6)
            _ => (slot_mask & 0b1000) != 0, // OP4 (b7)
        }
    }

    fn decode_pan_channel(&self, bank: usize, reg: u8) -> Option<usize> {
        if (0xB4..=0xB6).contains(&reg) {
            Some((bank & 1) * 3 + (reg as usize - 0xB4))
        } else {
            None
        }
    }

    fn decode_algorithm_channel(&self, bank: usize, reg: u8) -> Option<usize> {
        if (0xB0..=0xB2).contains(&reg) {
            Some((bank & 1) * 3 + (reg as usize - 0xB0))
        } else {
            None
        }
    }

    fn decode_operator_target(bank: usize, reg: u8) -> Option<(usize, usize, YmOperatorParam)> {
        let param = match reg & 0xF0 {
            0x30 => YmOperatorParam::Mul,
            0x40 => YmOperatorParam::Tl,
            0x50 => YmOperatorParam::Attack,
            0x60 => YmOperatorParam::Decay,
            0x70 => YmOperatorParam::SustainRate,
            0x80 => YmOperatorParam::SustainRelease,
            0x90 => YmOperatorParam::SsgEg,
            _ => return None,
        };

        let low = reg & 0x0F;
        if (low & 0x03) == 0x03 {
            return None;
        }
        let channel_in_bank = (low & 0x03) as usize;
        if channel_in_bank >= 3 {
            return None;
        }
        let slot_group = (low >> 2) as usize;
        let slot = match slot_group {
            0 => 0, // OP1
            1 => 2, // OP3
            2 => 1, // OP2
            3 => 3, // OP4
            _ => return None,
        };
        let channel = (bank & 1) * 3 + channel_in_bank;
        Some((channel, slot, param))
    }

    fn fnum_block_frequency_hz(fnum: u16, block: u8) -> f32 {
        // OPN2/2612 pitch approximation:
        // f ~= FNUM * 2^(BLOCK-1) * (master / (144 * 2^20))
        // This keeps pitch much closer to real hardware than the previous
        // A3-relative heuristic.
        let base = Self::MASTER_CLOCK_HZ as f32 / (144.0 * 1_048_576.0);
        let octave_scale = 2f32.powi(block as i32 - 1);
        let freq = (fnum.max(1) as f32) * octave_scale * base;
        freq.clamp(5.0, 100_000.0)
    }

    fn channel_base_frequency_hz(channel: &YmChannel) -> f32 {
        Self::fnum_block_frequency_hz(channel.fnum, channel.block)
    }

    fn block_fnum_keycode(block: u8, fnum: u16) -> u8 {
        ((block & 0x07) << 2) | (((fnum >> 9) & 0x03) as u8)
    }

    fn channel_keycode(channel: &YmChannel) -> u8 {
        // 5-bit note code used by key-scale rate approximation.
        Self::block_fnum_keycode(channel.block, channel.fnum)
    }

    fn channel3_special_mode_enabled(&self) -> bool {
        (self.channel3_mode_bits() & 0x01) != 0
    }

    fn channel_operator_base_frequency_hz(
        channel: &YmChannel,
        operator_index: usize,
        channel3_special_mode: bool,
    ) -> f32 {
        // YM2612 CH3 special mode follows the internal slot schedule used by
        // ym3438/Nuked core:
        // OP1 <- A9/AD, OP2 <- AA/AE, OP3 <- A8/AC, OP4 <- normal A2/A6.
        if channel3_special_mode
            && let Some(slot) = Self::channel3_special_slot_for_operator(operator_index)
        {
            Self::fnum_block_frequency_hz(channel.special_fnum[slot], channel.special_block[slot])
        } else {
            Self::channel_base_frequency_hz(channel)
        }
    }

    fn channel_operator_keycode(
        channel: &YmChannel,
        operator_index: usize,
        channel3_special_mode: bool,
    ) -> u8 {
        if channel3_special_mode
            && let Some(slot) = Self::channel3_special_slot_for_operator(operator_index)
        {
            Self::block_fnum_keycode(channel.special_block[slot], channel.special_fnum[slot])
        } else {
            Self::channel_keycode(channel)
        }
    }

    fn channel3_special_slot_for_operator(operator_index: usize) -> Option<usize> {
        match operator_index.min(3) {
            0 => Some(1), // OP1 <- A9/AD
            1 => Some(2), // OP2 <- AA/AE
            2 => Some(0), // OP3 <- A8/AC
            _ => None,    // OP4 uses channel FNUM/BLOCK (A2/A6)
        }
    }

    fn key_scale_rate_boost(keycode: u8, key_scale: u8) -> u8 {
        // Nuked OPN2: rks = kc >> (ks ^ 0x03)
        match key_scale & 0x03 {
            0 => keycode >> 3,
            1 => keycode >> 2,
            2 => keycode >> 1,
            _ => keycode,
        }
    }

    fn apply_key_scale_rate(base_rate: u8, key_scale: u8, keycode: u8) -> u8 {
        let boost = Self::key_scale_rate_boost(keycode, key_scale);
        base_rate.saturating_add(boost).min(31)
    }

    fn carrier_mul_factor(raw_mul: u8) -> f32 {
        if (raw_mul & 0x0F) == 0 {
            0.5
        } else {
            (raw_mul & 0x0F) as f32
        }
    }

    fn detune_ratio(detune: u8) -> f32 {
        // Approximate YM2612 DT1 steps in semitones.
        // Real hardware varies with keycode (up to ~1.1 semitones at DT=3).
        // Using mid-range keycode approximation for each DT level.
        const DETUNE_SEMITONES: [f32; 8] = [0.0, 0.2, 0.5, 0.8, -0.8, -0.5, -0.2, 0.0];
        let semitones = DETUNE_SEMITONES[(detune & 0x07) as usize];
        2f32.powf(semitones / 12.0)
    }

    fn lfo_rate_hz(rate: u8) -> f32 {
        // YM2612 LFO frequency steps (approximate).
        const LFO_HZ: [f32; 8] = [3.98, 5.56, 6.02, 6.37, 6.88, 9.63, 48.10, 72.20];
        LFO_HZ[(rate & 0x07) as usize]
    }

    fn channel_ams_depth(ams: u8) -> f32 {
        // Output amplitude modulation depth.
        // Real YM2612 AMS: 0dB, 1.4dB, 5.9dB, 11.8dB → linear ratio.
        const AMS_DEPTH: [f32; 4] = [0.0, 0.148, 0.493, 0.743];
        AMS_DEPTH[(ams & 0x03) as usize]
    }

    fn channel_fms_depth(fms: u8) -> f32 {
        // Frequency modulation sensitivity.
        // Values are derived from YM2612 PMS steps (±0.034, ±0.067, ±0.10,
        // ±0.14, ±0.20, ±0.40, ±0.80 semitones) converted to linear ratio
        // via `2^(semitones/12)-1`.
        const FMS_DEPTH: [f32; 8] = [
            0.0, 0.001965, 0.003877, 0.005793, 0.008118, 0.011619, 0.023374, 0.047294,
        ];
        FMS_DEPTH[(fms & 0x07) as usize]
    }

    fn operator_level_scale(op: &YmOperator) -> f32 {
        if op.tl >= 0x7F {
            return 0.0;
        }
        let attenuation_db = op.tl as f32 * 0.75;
        10f32.powf(-attenuation_db / 20.0)
    }

    fn attack_rate_to_step(rate: u8, sample_rate_hz: f32) -> f32 {
        if rate == 0 {
            // AR=0 on real YM2612 means the envelope does not advance at all.
            return 0.0;
        }
        // Envelope time approximation table (seconds to near-full level).
        // Calibrated against real YM2612 timing: AR=31 completes in ~1ms,
        // lower rates scale roughly logarithmically.
        const ATTACK_SECONDS: [f32; 32] = [
            0.0, 0.100, 0.088, 0.077, 0.068, 0.060, 0.052, 0.046, 0.040, 0.035, 0.030, 0.026,
            0.023, 0.020, 0.017, 0.015, 0.013, 0.0115, 0.010, 0.0087, 0.0076, 0.0066, 0.0057,
            0.0050, 0.0043, 0.0037, 0.0032, 0.0027, 0.0023, 0.0019, 0.0015, 0.001,
        ];
        Self::rate_table_step(rate, sample_rate_hz, &ATTACK_SECONDS)
    }

    fn decay_rate_to_step(rate: u8, sample_rate_hz: f32) -> f32 {
        // Linear decay time (seconds) calibrated against real YM2612 EG at
        // 53.3kHz.  Each 4 rate steps halves the time.  Rate 31 (internal
        // rate 62) completes in ~3.5ms.
        const DECAY_SECONDS: [f32; 32] = [
            0.0, 0.634, 0.534, 0.449, 0.378, 0.318, 0.267, 0.225, 0.189, 0.159, 0.134, 0.112,
            0.094, 0.080, 0.067, 0.056, 0.047, 0.040, 0.033, 0.028, 0.024, 0.020, 0.017, 0.014,
            0.012, 0.010, 0.0084, 0.007, 0.0059, 0.005, 0.0042, 0.0035,
        ];
        Self::rate_table_step(rate, sample_rate_hz, &DECAY_SECONDS)
    }

    fn sustain_rate_to_step(rate: u8, sample_rate_hz: f32) -> f32 {
        // Sustain rate uses the same EG mechanism as decay on real hardware.
        const SUSTAIN_SECONDS: [f32; 32] = [
            0.0, 0.824, 0.694, 0.583, 0.491, 0.413, 0.347, 0.293, 0.246, 0.207, 0.174, 0.146,
            0.122, 0.104, 0.087, 0.073, 0.061, 0.052, 0.043, 0.036, 0.031, 0.026, 0.022, 0.018,
            0.016, 0.013, 0.011, 0.009, 0.0077, 0.0065, 0.0055, 0.0046,
        ];
        Self::rate_table_step(rate, sample_rate_hz, &SUSTAIN_SECONDS)
    }

    fn release_rate_to_step(rate: u8, sample_rate_hz: f32) -> f32 {
        // Release rate is 4-bit; internal rate ≈ 4*RR+1.  Each 4 internal
        // rate steps halves the time.  RR=15 (internal ~61) ≈ 3ms.
        const RELEASE_SECONDS: [f32; 16] = [
            10.0, 6.0, 3.5, 2.0, 1.2, 0.7, 0.42, 0.25, 0.15, 0.09, 0.053, 0.032, 0.019, 0.011,
            0.006, 0.003,
        ];
        let rate = rate.min(15) as usize;
        let seconds = RELEASE_SECONDS[rate];
        if sample_rate_hz <= 0.0 || seconds <= 0.0 {
            return 0.0;
        }
        (1.0 / (seconds * sample_rate_hz)).clamp(0.0, 1.0)
    }

    fn sustain_level_to_amplitude(level: u8) -> f32 {
        if level >= 0x0F {
            0.0
        } else {
            1.0 - (level as f32 / 15.0)
        }
    }

    fn rate_table_step(rate: u8, sample_rate_hz: f32, table: &[f32; 32]) -> f32 {
        let idx = rate.min(31) as usize;
        let seconds = table[idx];
        if idx == 0 || sample_rate_hz <= 0.0 || seconds <= 0.0 {
            return 0.0;
        }
        (1.0 / (seconds * sample_rate_hz)).clamp(0.0, 1.0)
    }

    fn ssg_eg_enabled(op: &YmOperator) -> bool {
        (op.ssg_eg & 0x08) != 0
    }

    fn ssg_eg_effective_sustain_target(op: &YmOperator) -> f32 {
        if Self::ssg_eg_enabled(op) {
            0.0
        } else {
            Self::sustain_level_to_amplitude(op.sustain_level)
        }
    }

    fn ssg_eg_output_level(op: &YmOperator, envelope: f32) -> f32 {
        if !Self::ssg_eg_enabled(op) {
            return envelope;
        }
        let attack_invert = (op.ssg_eg & 0x04) != 0;
        let invert = attack_invert ^ op.ssg_invert;
        if invert {
            (1.0 - envelope).clamp(0.0, 1.0)
        } else {
            envelope.clamp(0.0, 1.0)
        }
    }

    fn advance_ssg_eg_cycle(op: &mut YmOperator) {
        if !Self::ssg_eg_enabled(op) || !op.key_on {
            return;
        }
        if op.envelope_phase != YmEnvelopePhase::Sustain || op.envelope_level > 0.0 {
            return;
        }

        let hold = (op.ssg_eg & 0x01) != 0;
        let alternate = (op.ssg_eg & 0x02) != 0;
        let attack = (op.ssg_eg & 0x04) != 0;
        let current_top = attack ^ op.ssg_invert;
        if hold {
            if alternate {
                op.ssg_invert = !op.ssg_invert;
            }
            // Keep output stable at the held terminal level.
            // In this model, envelope=0 + invert chooses top/bottom.
            if current_top {
                op.ssg_invert = attack;
            }
            op.ssg_hold_active = true;
            return;
        }
        if alternate {
            op.ssg_invert = !op.ssg_invert;
        }
        op.envelope_phase = YmEnvelopePhase::Attack;
        op.envelope_level = 0.0;
    }

    fn advance_envelope(op: &mut YmOperator, sample_rate_hz: f32, keycode: u8) -> f32 {
        if Self::ssg_eg_enabled(op) && op.ssg_hold_active && op.key_on {
            return op.envelope_level;
        }
        let attack_rate = Self::apply_key_scale_rate(op.attack_rate, op.key_scale, keycode);
        let decay_rate = Self::apply_key_scale_rate(op.decay_rate, op.key_scale, keycode);
        let sustain_rate = Self::apply_key_scale_rate(op.sustain_rate, op.key_scale, keycode);
        let release_rate = op
            .release_rate
            .saturating_add((Self::key_scale_rate_boost(keycode, op.key_scale) + 1) >> 1)
            .min(15);
        match op.envelope_phase {
            YmEnvelopePhase::Off => {
                op.envelope_level = 0.0;
            }
            YmEnvelopePhase::Attack => {
                let step = Self::attack_rate_to_step(attack_rate, sample_rate_hz);
                if step > 0.0 {
                    // Attack is exponential toward 1.0 to avoid buzzy starts.
                    op.envelope_level =
                        (op.envelope_level + (1.0 - op.envelope_level) * step * 8.0).min(1.0);
                    if op.envelope_level >= 0.999 {
                        op.envelope_level = 1.0;
                        op.envelope_phase = YmEnvelopePhase::Decay;
                    }
                }
            }
            YmEnvelopePhase::Decay => {
                let sustain_target = Self::ssg_eg_effective_sustain_target(op);
                let step = Self::decay_rate_to_step(decay_rate, sample_rate_hz);
                if step <= 0.0 || op.envelope_level <= sustain_target {
                    op.envelope_level = sustain_target;
                    op.envelope_phase = YmEnvelopePhase::Sustain;
                } else {
                    op.envelope_level = (op.envelope_level - step).max(sustain_target);
                    if op.envelope_level <= sustain_target {
                        op.envelope_phase = YmEnvelopePhase::Sustain;
                    }
                }
            }
            YmEnvelopePhase::Sustain => {
                let step = if Self::ssg_eg_enabled(op) && sustain_rate == 0 {
                    // SSG-EG shapes often keep cycling even when SR is low/zero.
                    // Use DR as fallback in this mode so looping waveforms keep moving.
                    Self::decay_rate_to_step(decay_rate.max(1), sample_rate_hz)
                } else {
                    Self::sustain_rate_to_step(sustain_rate, sample_rate_hz)
                };
                if step > 0.0 {
                    op.envelope_level = (op.envelope_level - step).max(0.0);
                }
            }
            YmEnvelopePhase::Release => {
                let step = Self::release_rate_to_step(release_rate, sample_rate_hz);
                if step <= 0.0 {
                    op.envelope_level = 0.0;
                    op.envelope_phase = YmEnvelopePhase::Off;
                } else {
                    op.envelope_level = (op.envelope_level - step).max(0.0);
                    if op.envelope_level <= 0.0 {
                        op.envelope_phase = YmEnvelopePhase::Off;
                    }
                }
            }
        }
        Self::advance_ssg_eg_cycle(op);
        op.envelope_level
    }

    fn operator_active(op: &YmOperator) -> bool {
        op.key_on || op.envelope_phase != YmEnvelopePhase::Off
    }

    fn channel_active(channel: &YmChannel) -> bool {
        channel.operators.iter().any(Self::operator_active)
    }

    fn advance_lfo(&mut self, sample_rate_hz: f32) -> (f32, f32) {
        if !self.lfo_enabled || sample_rate_hz <= 0.0 {
            return (0.0, 0.0);
        }
        let rate_hz = Self::lfo_rate_hz(self.lfo_rate);
        self.lfo_phase += rate_hz / sample_rate_hz;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= self.lfo_phase.floor();
        }
        let wave = (self.lfo_phase * TAU).sin();
        let am = (wave + 1.0) * 0.5;
        let pm = wave;
        (am, pm)
    }

    fn advance_operator_sample(
        op: &mut YmOperator,
        base_freq_hz: f32,
        sample_rate_hz: f32,
        phase_mod_radians: f32,
        keycode: u8,
        lfo_am: f32,
        channel_ams_depth: f32,
    ) -> f32 {
        if !Self::operator_active(op) {
            op.last_output = 0.0;
            return 0.0;
        }

        let freq = base_freq_hz * Self::carrier_mul_factor(op.mul) * Self::detune_ratio(op.detune);
        op.phase += freq / sample_rate_hz;
        if op.phase >= 1.0 {
            op.phase -= op.phase.floor();
        }
        let mut envelope = Self::advance_envelope(op, sample_rate_hz, keycode);
        if op.am_enable && channel_ams_depth > 0.0 {
            envelope *= 1.0 - (lfo_am * channel_ams_depth * 0.85);
            envelope = envelope.max(0.0);
        }
        envelope = Self::ssg_eg_output_level(op, envelope);
        let sample = ((TAU * op.phase) + phase_mod_radians).sin()
            * Self::operator_level_scale(op)
            * envelope;
        op.last_output = sample;
        sample
    }

    pub fn writes(&self) -> u64 {
        self.writes
    }

    pub fn dac_data_writes(&self) -> u64 {
        self.dac_data_writes
    }

    pub fn active_channels(&self) -> usize {
        self.channels
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                let dac_channel = *index == 5;
                !dac_channel || !self.dac_enabled
            })
            .filter(|(_, channel)| Self::channel_active(channel))
            .count()
    }

    fn render_channel_sample(
        channel: &mut YmChannel,
        sample_rate_hz: f32,
        lfo_am: f32,
        lfo_pm: f32,
        channel3_special_mode: bool,
    ) -> f32 {
        let am_depth = Self::channel_ams_depth(channel.ams);
        let pm_factor = 1.0 + lfo_pm * Self::channel_fms_depth(channel.fms);
        let op_freqs = [
            (Self::channel_operator_base_frequency_hz(channel, 0, channel3_special_mode)
                * pm_factor)
                .max(1.0),
            (Self::channel_operator_base_frequency_hz(channel, 1, channel3_special_mode)
                * pm_factor)
                .max(1.0),
            (Self::channel_operator_base_frequency_hz(channel, 2, channel3_special_mode)
                * pm_factor)
                .max(1.0),
            (Self::channel_operator_base_frequency_hz(channel, 3, channel3_special_mode)
                * pm_factor)
                .max(1.0),
        ];
        let op_keycodes = [
            Self::channel_operator_keycode(channel, 0, channel3_special_mode),
            Self::channel_operator_keycode(channel, 1, channel3_special_mode),
            Self::channel_operator_keycode(channel, 2, channel3_special_mode),
            Self::channel_operator_keycode(channel, 3, channel3_special_mode),
        ];
        let alg = channel.algorithm & 0x07;
        let (connect1, connect2, connect3, mem_restore_to, special_alg5) = match alg {
            0 => (
                Some(YmAlgorithmBus::C1),
                Some(YmAlgorithmBus::Mem),
                Some(YmAlgorithmBus::C2),
                Some(YmAlgorithmBus::M2),
                false,
            ),
            1 => (
                Some(YmAlgorithmBus::Mem),
                Some(YmAlgorithmBus::Mem),
                Some(YmAlgorithmBus::C2),
                Some(YmAlgorithmBus::M2),
                false,
            ),
            2 => (
                Some(YmAlgorithmBus::C2),
                Some(YmAlgorithmBus::Mem),
                Some(YmAlgorithmBus::C2),
                Some(YmAlgorithmBus::M2),
                false,
            ),
            3 => (
                Some(YmAlgorithmBus::C1),
                Some(YmAlgorithmBus::Mem),
                Some(YmAlgorithmBus::C2),
                Some(YmAlgorithmBus::C2),
                false,
            ),
            4 => (
                Some(YmAlgorithmBus::C1),
                Some(YmAlgorithmBus::Out),
                Some(YmAlgorithmBus::C2),
                Some(YmAlgorithmBus::Mem),
                false,
            ),
            5 => (
                None,
                Some(YmAlgorithmBus::Out),
                Some(YmAlgorithmBus::Out),
                Some(YmAlgorithmBus::M2),
                true,
            ),
            6 => (
                Some(YmAlgorithmBus::C1),
                Some(YmAlgorithmBus::Out),
                Some(YmAlgorithmBus::Out),
                Some(YmAlgorithmBus::Mem),
                false,
            ),
            _ => (
                Some(YmAlgorithmBus::Out),
                Some(YmAlgorithmBus::Out),
                Some(YmAlgorithmBus::Out),
                Some(YmAlgorithmBus::Mem),
                false,
            ),
        };
        let mut m2_bus = 0.0f32;
        let mut c1_bus = 0.0f32;
        let mut c2_bus = 0.0f32;
        let mut mem_bus = 0.0f32;
        let mut out_bus = 0.0f32;
        if let Some(destination) = mem_restore_to {
            Self::route_algorithm_bus(
                destination,
                channel.feedback_sample_prev,
                &mut m2_bus,
                &mut c1_bus,
                &mut c2_bus,
                &mut mem_bus,
                &mut out_bus,
            );
        }
        // FM phase modulation depth (radians) for internal operator routing.
        // Real YM2612: 14-bit operator output added to 10-bit phase (1024=2π).
        // Max modulation = ~4096/1024 * 2π = 8π radians.
        let op_mod_index = 8.0 * std::f32::consts::PI;
        // YM2612 feedback level (self-modulation on OP1).
        // Standard OPN values: FB=1→π/16 .. FB=7→4π (peak radians).
        // Code applies to the sum of two samples (±2.0 peak), so table
        // stores half the documented peak: [0, π/32, π/16, .., 2π].
        const FEEDBACK_PHASE_RADIANS: [f32; 8] =
            [0.0, 0.098, 0.196, 0.393, 0.785, 1.571, 3.1416, 6.2832];
        // OPN feedback uses OP1's last two outputs. `operators[0].last_output`
        // stores n-1 and `feedback_sample` tracks n-2.
        let op1_prev = channel.operators[0].last_output;
        let fb_phase_mod =
            (op1_prev + channel.feedback_sample) * FEEDBACK_PHASE_RADIANS[channel.feedback.min(7) as usize];
        let o1 = Self::advance_operator_sample(
            &mut channel.operators[0],
            op_freqs[0],
            sample_rate_hz,
            fb_phase_mod,
            op_keycodes[0],
            lfo_am,
            am_depth,
        );
        channel.feedback_sample = op1_prev;
        if special_alg5 {
            mem_bus += o1;
            c1_bus += o1;
            c2_bus += o1;
        } else if let Some(destination) = connect1 {
            Self::route_algorithm_bus(
                destination,
                o1,
                &mut m2_bus,
                &mut c1_bus,
                &mut c2_bus,
                &mut mem_bus,
                &mut out_bus,
            );
        }
        // YM internal slot order is OP1 -> OP3 -> OP2 -> OP4.
        let o3 = Self::advance_operator_sample(
            &mut channel.operators[2],
            op_freqs[2],
            sample_rate_hz,
            m2_bus * op_mod_index,
            op_keycodes[2],
            lfo_am,
            am_depth,
        );
        if let Some(destination) = connect3 {
            Self::route_algorithm_bus(
                destination,
                o3,
                &mut m2_bus,
                &mut c1_bus,
                &mut c2_bus,
                &mut mem_bus,
                &mut out_bus,
            );
        }
        let o2 = Self::advance_operator_sample(
            &mut channel.operators[1],
            op_freqs[1],
            sample_rate_hz,
            c1_bus * op_mod_index,
            op_keycodes[1],
            lfo_am,
            am_depth,
        );
        if let Some(destination) = connect2 {
            Self::route_algorithm_bus(
                destination,
                o2,
                &mut m2_bus,
                &mut c1_bus,
                &mut c2_bus,
                &mut mem_bus,
                &mut out_bus,
            );
        }
        let o4 = Self::advance_operator_sample(
            &mut channel.operators[3],
            op_freqs[3],
            sample_rate_hz,
            c2_bus * op_mod_index,
            op_keycodes[3],
            lfo_am,
            am_depth,
        );
        out_bus += o4;
        channel.feedback_sample_prev = mem_bus;
        out_bus
    }

    fn route_algorithm_bus(
        destination: YmAlgorithmBus,
        sample: f32,
        m2_bus: &mut f32,
        c1_bus: &mut f32,
        c2_bus: &mut f32,
        mem_bus: &mut f32,
        out_bus: &mut f32,
    ) {
        match destination {
            YmAlgorithmBus::M2 => *m2_bus += sample,
            YmAlgorithmBus::C1 => *c1_bus += sample,
            YmAlgorithmBus::C2 => *c2_bus += sample,
            YmAlgorithmBus::Mem => *mem_bus += sample,
            YmAlgorithmBus::Out => *out_bus += sample,
        }
    }

    fn next_sample_stereo(&mut self, sample_rate_hz: f32) -> (i16, i16) {
        let (lfo_am, lfo_pm) = self.advance_lfo(sample_rate_hz);
        let mut left_mix = 0.0f32;
        let mut right_mix = 0.0f32;
        let mut left_active = 0usize;
        let mut right_active = 0usize;
        let channel3_special_mode = self.channel3_special_mode_enabled();
        for (index, channel) in self.channels.iter_mut().enumerate() {
            if index == 5 && self.dac_enabled {
                // CH6 FM output is muted while DAC is enabled.
                // Skip FM rendering here so DAC cycle accumulators are not
                // overwritten by channel feedback scratch values.
                continue;
            }
            if !Self::channel_active(channel) {
                continue;
            }
            let sample = Self::render_channel_sample(
                channel,
                sample_rate_hz,
                lfo_am,
                lfo_pm,
                channel3_special_mode && index == 2,
            );
            if channel.pan_left {
                left_mix += sample;
                left_active += 1;
            }
            if channel.pan_right {
                right_mix += sample;
                right_active += 1;
            }
        }
        let fm_left = if left_active == 0 {
            0
        } else {
            (left_mix * 18_000.0).clamp(i16::MIN as f32, i16::MAX as f32) as i16
        };
        let fm_right = if right_active == 0 {
            0
        } else {
            (right_mix * 18_000.0).clamp(i16::MIN as f32, i16::MAX as f32) as i16
        };
        let (dac_left, dac_right) = if self.dac_enabled || self.channels[5].feedback_sample_prev > 0.0
        {
            let channel = &mut self.channels[5];
            // Accumulate DAC sample-hold output in Z80 time and average here.
            // This preserves sub-sample DAC write timing and avoids "beep"/alias
            // artifacts when drivers stream PCM between host output samples.
            let dac_sample = if channel.feedback_sample_prev > 0.0 {
                channel.feedback_sample / channel.feedback_sample_prev
            } else {
                self.dac_output as f32
            };
            channel.feedback_sample = 0.0;
            channel.feedback_sample_prev = 0.0;
            let dac_i16 = dac_sample.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            let left = if channel.pan_left { dac_i16 } else { 0 };
            let right = if channel.pan_right { dac_i16 } else { 0 };
            (left, right)
        } else {
            (0, 0)
        };
        let left =
            (fm_left as i32 + dac_left as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        let right =
            (fm_right as i32 + dac_right as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        (left, right)
    }

    fn step_z80_cycles(&mut self, cycles: u32) {
        self.busy_z80_cycles = self.busy_z80_cycles.saturating_sub(cycles);
        if cycles > 0 {
            let pending_enabled = self.dac_enabled_pending.take();
            let pending_output = self
                .dac_output_pending
                .take()
                .map(Self::decode_pending_dac_output);
            let pending_count = pending_enabled.is_some() as u8 + pending_output.is_some() as u8;
            if pending_count == 0 {
                self.accumulate_dac_cycles(cycles);
            } else {
                // Z80 writes happen during the elapsed instruction slice.
                // Apply pending DAC changes inside this slice instead of only
                // at the end to reduce one-instruction quantization artifacts.
                let total_cycles = cycles as u64;
                let mut stage_index = 0u64;
                let stage_count = pending_count as u64 + 1;
                let mut consumed_cycles = 0u32;
                match (pending_enabled, pending_output) {
                    (Some(enabled), Some((output, output_after_enable))) => {
                        if output_after_enable {
                            stage_index += 1;
                            let boundary = ((stage_index * total_cycles) / stage_count) as u32;
                            let span = boundary.saturating_sub(consumed_cycles);
                            self.accumulate_dac_cycles(span);
                            consumed_cycles = boundary;
                            self.set_dac_enabled(enabled);

                            stage_index += 1;
                            let boundary = ((stage_index * total_cycles) / stage_count) as u32;
                            let span = boundary.saturating_sub(consumed_cycles);
                            self.accumulate_dac_cycles(span);
                            consumed_cycles = boundary;
                            self.dac_output = output;
                        } else {
                            stage_index += 1;
                            let boundary = ((stage_index * total_cycles) / stage_count) as u32;
                            let span = boundary.saturating_sub(consumed_cycles);
                            self.accumulate_dac_cycles(span);
                            consumed_cycles = boundary;
                            self.dac_output = output;

                            stage_index += 1;
                            let boundary = ((stage_index * total_cycles) / stage_count) as u32;
                            let span = boundary.saturating_sub(consumed_cycles);
                            self.accumulate_dac_cycles(span);
                            consumed_cycles = boundary;
                            self.set_dac_enabled(enabled);
                        }
                    }
                    (Some(enabled), None) => {
                        stage_index += 1;
                        let boundary = ((stage_index * total_cycles) / stage_count) as u32;
                        let span = boundary.saturating_sub(consumed_cycles);
                        self.accumulate_dac_cycles(span);
                        consumed_cycles = boundary;
                        self.set_dac_enabled(enabled);
                    }
                    (None, Some((output, _))) => {
                        stage_index += 1;
                        let boundary = ((stage_index * total_cycles) / stage_count) as u32;
                        let span = boundary.saturating_sub(consumed_cycles);
                        self.accumulate_dac_cycles(span);
                        consumed_cycles = boundary;
                        self.dac_output = output;
                    }
                    (None, None) => {}
                }
                self.accumulate_dac_cycles(cycles.saturating_sub(consumed_cycles));
            }
        }

        let ym_cycle_divisor = Self::Z80_CLOCK_HZ * Self::YM2612_DIVIDER;
        self.timer_clock_accumulator += (cycles as u64) * Self::MASTER_CLOCK_HZ;
        let ym_cycles = self.timer_clock_accumulator / ym_cycle_divisor;
        self.timer_clock_accumulator %= ym_cycle_divisor;
        if ym_cycles == 0 {
            return;
        }

        if (self.timer_control & 0x01) != 0 {
            let period = self.timer_a_period_ym_cycles();
            self.timer_a_elapsed_ym_cycles += ym_cycles;
            while self.timer_a_elapsed_ym_cycles >= period {
                self.timer_a_elapsed_ym_cycles -= period;
                if (self.timer_control & 0x04) != 0 {
                    self.timer_status |= 0x01;
                }
                if self.csm_mode_enabled() {
                    self.trigger_csm_channel3_key_on();
                }
            }
        }
        if (self.timer_control & 0x02) != 0 {
            let period = self.timer_b_period_ym_cycles();
            self.timer_b_elapsed_ym_cycles += ym_cycles;
            while self.timer_b_elapsed_ym_cycles >= period {
                self.timer_b_elapsed_ym_cycles -= period;
                if (self.timer_control & 0x08) != 0 {
                    self.timer_status |= 0x02;
                }
            }
        }

    }

    fn csm_mode_enabled(&self) -> bool {
        self.channel3_mode_bits() == 0b10
    }

    fn channel3_mode_bits(&self) -> u8 {
        (self.timer_control >> 6) & 0x03
    }

    fn trigger_csm_channel3_key_on(&mut self) {
        // CSM (bit7 of mode register) retriggers channel 3 on Timer A overflow.
        let ch3 = &mut self.channels[2];
        ch3.feedback_sample = 0.0;
        ch3.feedback_sample_prev = 0.0;
        for op in &mut ch3.operators {
            op.phase = 0.0;
            op.last_output = 0.0;
            op.envelope_phase = YmEnvelopePhase::Attack;
            op.envelope_level = 0.0;
            op.key_on = true;
        }
    }

    fn accumulate_dac_cycles(&mut self, cycles: u32) {
        if !self.dac_enabled || cycles == 0 {
            return;
        }
        let channel = &mut self.channels[5];
        channel.feedback_sample += self.dac_output as f32 * cycles as f32;
        channel.feedback_sample_prev += cycles as f32;
    }

    fn set_dac_enabled(&mut self, enabled: bool) {
        let was_enabled = self.dac_enabled;
        self.dac_enabled = enabled;
        if was_enabled != self.dac_enabled {
            // Reset CH6 scratch state when DAC mode toggles.
            self.channels[5].feedback_sample = 0.0;
            self.channels[5].feedback_sample_prev = 0.0;
        }
    }

    fn encode_pending_dac_output(output: i16, output_after_enable: bool) -> i16 {
        (output & !Self::DAC_PENDING_ORDER_MASK)
            | if output_after_enable {
                Self::DAC_PENDING_ORDER_MASK
            } else {
                0
            }
    }

    fn decode_pending_dac_output(tagged_output: i16) -> (i16, bool) {
        let output_after_enable = (tagged_output & Self::DAC_PENDING_ORDER_MASK) != 0;
        let output = tagged_output & !Self::DAC_PENDING_ORDER_MASK;
        (output, output_after_enable)
    }

    fn read_status(&self) -> u8 {
        let mut status = self.timer_status & 0x03;
        if self.busy_z80_cycles > 0 {
            status |= 0x80;
        }
        status
    }

    fn timer_a_period_ym_cycles(&self) -> u64 {
        let value = (self.timer_a_value & 0x03FF) as u64;
        (1024 - value).max(1) * 18
    }

    fn timer_b_period_ym_cycles(&self) -> u64 {
        let value = self.timer_b_value as u64;
        (256 - value).max(1) * 288
    }

    pub fn register(&self, bank: usize, index: u8) -> u8 {
        self.regs[bank & 1][index as usize]
    }

    pub fn dac_enabled(&self) -> bool {
        self.dac_enabled
    }

    pub fn channel_key_on(&self, channel: usize) -> bool {
        self.channels[channel.min(5)]
            .operators
            .iter()
            .any(|op| op.key_on)
    }

    pub fn channel_operator_key_on(&self, channel: usize, operator: usize) -> bool {
        let channel = self.channels[channel.min(5)];
        channel.operators[operator.min(3)].key_on
    }

    pub fn lfo_enabled(&self) -> bool {
        self.lfo_enabled
    }

    pub fn lfo_rate(&self) -> u8 {
        self.lfo_rate
    }

    pub fn channel_frequency_hz_debug(&self, channel: usize) -> f32 {
        self.channel_operator_frequency_hz_debug(channel, 3)
    }

    pub fn channel_operator_frequency_hz_debug(&self, channel: usize, operator: usize) -> f32 {
        let channel_index = channel.min(5);
        let operator_index = operator.min(3);
        let channel = self.channels[channel_index];
        let base_hz = Self::channel_operator_base_frequency_hz(
            &channel,
            operator_index,
            channel_index == 2 && self.channel3_special_mode_enabled(),
        );
        let op = channel.operators[operator_index];
        base_hz * Self::carrier_mul_factor(op.mul) * Self::detune_ratio(op.detune)
    }

    pub fn channel_carrier_mul(&self, channel: usize) -> u8 {
        self.channels[channel.min(5)].operators[3].mul
    }

    pub fn channel_carrier_detune(&self, channel: usize) -> u8 {
        self.channels[channel.min(5)].operators[3].detune
    }

    pub fn channel_carrier_tl(&self, channel: usize) -> u8 {
        self.channels[channel.min(5)].operators[3].tl
    }

    pub fn channel_carrier_ssg_eg(&self, channel: usize) -> u8 {
        self.channels[channel.min(5)].operators[3].ssg_eg
    }

    pub fn channel_algorithm_feedback(&self, channel: usize) -> (u8, u8) {
        let channel = self.channels[channel.min(5)];
        (channel.algorithm, channel.feedback)
    }

    pub fn channel_ams_fms(&self, channel: usize) -> (u8, u8) {
        let channel = self.channels[channel.min(5)];
        (channel.ams, channel.fms)
    }

    pub fn channel_envelope_level(&self, channel: usize) -> f32 {
        self.channels[channel.min(5)].operators[3].envelope_level
    }

    pub fn channel_envelope_params(&self, channel: usize) -> (u8, u8, u8, u8, u8) {
        let op = self.channels[channel.min(5)].operators[3];
        (
            op.attack_rate,
            op.decay_rate,
            op.sustain_rate,
            op.sustain_level,
            op.release_rate,
        )
    }

    pub fn channel_block_and_fnum(&self, channel: usize) -> (u8, u16) {
        let channel = self.channels[channel.min(5)];
        (channel.block, channel.fnum)
    }
}

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct Psg {
    last_data: u8,
    writes: u64,
    latched_channel: usize,
    latched_is_volume: bool,
    tone_period: [u16; 3],
    tone_phase_high: [bool; 3],
    tone_phase_acc: [f32; 3],
    attenuation: [u8; 4],
    noise_control: u8,
    noise_lfsr: u16,
    noise_phase_acc: f32,
}

impl Default for Psg {
    fn default() -> Self {
        Self {
            last_data: 0,
            writes: 0,
            latched_channel: 0,
            latched_is_volume: false,
            tone_period: [1, 1, 1],
            tone_phase_high: [true, true, true],
            tone_phase_acc: [0.0, 0.0, 0.0],
            attenuation: [0x0F; 4],
            noise_control: 0,
            noise_lfsr: 0x4000,
            noise_phase_acc: 0.0,
        }
    }
}

impl Psg {
    const PSG_CLOCK_HZ: f32 = 3_579_545.0;

    fn write_data(&mut self, value: u8) {
        self.last_data = value;
        self.writes += 1;
        if (value & 0x80) != 0 {
            self.latched_channel = ((value >> 5) & 0x3) as usize;
            self.latched_is_volume = (value & 0x10) != 0;
            let data = value & 0x0F;
            self.apply_latched_data(data);
            return;
        }

        if self.latched_is_volume {
            self.attenuation[self.latched_channel] = value & 0x0F;
        } else if self.latched_channel < 3 {
            let lo = self.tone_period[self.latched_channel] & 0x000F;
            let hi = ((value & 0x3F) as u16) << 4;
            self.tone_period[self.latched_channel] = lo | hi;
        }
    }

    pub fn last_data(&self) -> u8 {
        self.last_data
    }

    pub fn writes(&self) -> u64 {
        self.writes
    }

    pub fn tone_period(&self, channel: usize) -> u16 {
        self.tone_period[channel.min(2)]
    }

    pub fn attenuation(&self, channel: usize) -> u8 {
        self.attenuation[channel.min(3)]
    }

    pub fn noise_control(&self) -> u8 {
        self.noise_control
    }

    pub fn tone_frequency_hz_debug(&self, channel: usize) -> f32 {
        self.tone_frequency_hz(channel)
    }

    fn apply_latched_data(&mut self, data: u8) {
        if self.latched_is_volume {
            self.attenuation[self.latched_channel] = data & 0x0F;
            return;
        }

        if self.latched_channel < 3 {
            let hi = self.tone_period[self.latched_channel] & 0x03F0;
            self.tone_period[self.latched_channel] = hi | data as u16;
        } else {
            self.noise_control = data & 0x07;
            self.noise_lfsr = 0x4000;
            self.noise_phase_acc = 0.0;
        }
    }

    fn tone_frequency_hz(&self, channel: usize) -> f32 {
        let raw_period = self.tone_period[channel.min(2)] & 0x03FF;
        // Genesis integrated PSG behavior: period=0 behaves like period=1.
        let period = raw_period.max(1) as f32;
        Self::PSG_CLOCK_HZ / (32.0 * period)
    }

    fn noise_frequency_hz(&self) -> f32 {
        match self.noise_control & 0x03 {
            0x00 => Self::PSG_CLOCK_HZ / 512.0,
            0x01 => Self::PSG_CLOCK_HZ / 1024.0,
            0x02 => Self::PSG_CLOCK_HZ / 2048.0,
            0x03 => self.tone_frequency_hz(2),
            _ => Self::PSG_CLOCK_HZ / 512.0,
        }
    }

    fn channel_amplitude(&self, channel: usize) -> f32 {
        let att = self.attenuation[channel.min(3)] as f32;
        if att >= 15.0 {
            0.0
        } else {
            10f32.powf(-(att * 2.0) / 20.0)
        }
    }

    fn clock_noise_lfsr(&mut self) {
        let bit0 = self.noise_lfsr & 1;
        let feedback = if (self.noise_control & 0x04) != 0 {
            let bit3 = (self.noise_lfsr >> 3) & 1;
            bit0 ^ bit3
        } else {
            bit0
        };
        // SN76489-compatible 15-bit LFSR (bit14 is the injected tap bit).
        self.noise_lfsr = ((self.noise_lfsr >> 1) | (feedback << 14)) & 0x7FFF;
    }

    fn next_sample(&mut self, sample_rate_hz: f32) -> i16 {
        let noise_uses_tone3 = (self.noise_control & 0x03) == 0x03;
        let mut tone3_falling_edges = 0usize;
        for channel in 0..3 {
            // The divider formula returns full square-wave frequency, while
            // this phase accumulator toggles high/low once per wrap.
            self.tone_phase_acc[channel] +=
                (self.tone_frequency_hz(channel) * 2.0) / sample_rate_hz;
            while self.tone_phase_acc[channel] >= 1.0 {
                self.tone_phase_acc[channel] -= 1.0;
                let was_high = self.tone_phase_high[channel];
                self.tone_phase_high[channel] = !self.tone_phase_high[channel];
                if noise_uses_tone3 && channel == 2 && was_high && !self.tone_phase_high[channel] {
                    tone3_falling_edges = tone3_falling_edges.saturating_add(1);
                }
            }
        }

        if noise_uses_tone3 {
            for _ in 0..tone3_falling_edges {
                self.clock_noise_lfsr();
            }
        } else {
            self.noise_phase_acc += self.noise_frequency_hz() / sample_rate_hz;
            while self.noise_phase_acc >= 1.0 {
                self.noise_phase_acc -= 1.0;
                self.clock_noise_lfsr();
            }
        }

        let mut mix = 0.0f32;
        for channel in 0..3 {
            let amp = self.channel_amplitude(channel);
            mix += if self.tone_phase_high[channel] {
                amp
            } else {
                -amp
            };
        }
        let noise_amp = self.channel_amplitude(3);
        mix += if (self.noise_lfsr & 1) != 0 {
            noise_amp
        } else {
            -noise_amp
        };

        (mix * 1800.0).clamp(i16::MIN as f32, i16::MAX as f32) as i16
    }
}

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct AudioBus {
    ym2612: Ym2612,
    psg: Psg,
    ym_writes_from_68k: u64,
    ym_writes_from_z80: u64,
    psg_writes_from_68k: u64,
    psg_writes_from_z80: u64,
    cycles: u64,
    output_sample_rate_hz: u64,
    sample_accumulator: u64,
    sample_buffer: Vec<i16>,
}

impl AudioBus {
    const M68K_CLOCK_HZ: u64 = 7_670_454;
    const DEFAULT_OUTPUT_SAMPLE_RATE_HZ: u64 = 44_100;
    const OUTPUT_CHANNELS: u8 = 2;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn output_sample_rate_hz(&self) -> u32 {
        self.output_sample_rate_hz as u32
    }

    pub fn output_channels(&self) -> u8 {
        Self::OUTPUT_CHANNELS
    }

    pub fn set_output_sample_rate_hz(&mut self, hz: u32) {
        self.output_sample_rate_hz = (hz as u64).clamp(8_000, 192_000);
    }

    pub fn read_ym2612(&self, port: u8) -> u8 {
        if (port & 0x01) == 0 {
            self.ym2612.read_status()
        } else {
            0xFF
        }
    }

    pub fn write_ym2612(&mut self, port: u8, value: u8) {
        self.ym_writes_from_68k += 1;
        self.ym2612.write_port(port, value);
    }

    pub fn write_ym2612_from_z80(&mut self, port: u8, value: u8) {
        // YM2612 BUSY is a status/read-side signal; writes are still latched.
        // Dropping data writes while BUSY causes audible FM/DAC corruption in
        // Z80-driven drivers (e.g. Landstalker effects).
        self.ym_writes_from_z80 += 1;
        self.ym2612.write_port_from_z80(port, value);
    }

    pub fn write_psg(&mut self, value: u8) {
        self.psg_writes_from_68k += 1;
        self.psg.write_data(value);
    }

    pub fn write_psg_from_z80(&mut self, value: u8) {
        self.psg_writes_from_z80 += 1;
        self.psg.write_data(value);
    }

    pub fn step_z80_cycles(&mut self, z80_cycles: u32) {
        self.ym2612.step_z80_cycles(z80_cycles);
    }

    pub fn step(&mut self, m68k_cycles: u32) {
        self.cycles += m68k_cycles as u64;
        let sample_rate_hz = self.output_sample_rate_hz.max(1);
        self.sample_accumulator += m68k_cycles as u64 * sample_rate_hz;
        let produced = (self.sample_accumulator / Self::M68K_CLOCK_HZ) as usize;
        self.sample_accumulator %= Self::M68K_CLOCK_HZ;
        for _ in 0..produced {
            // Keep PSG clearly below FM in the global mix so square-wave beeps
            // do not overpower YM2612 music.
            // PSG can generate very high-frequency components that alias
            // harshly at 44.1kHz. A small oversampling pass reduces "beepy"
            // artifacts without changing the external sample rate.
            let psg_oversample_u64 = 4u64;
            let psg_oversample_i32 = 4i32;
            let mut psg_acc = 0i32;
            for _ in 0..psg_oversample_u64 {
                psg_acc += self
                    .psg
                    .next_sample((sample_rate_hz * psg_oversample_u64) as f32)
                    as i32;
            }
            let psg_sample = (psg_acc / psg_oversample_i32) * 2 / 5;
            // A light YM oversampling pass reduces FM aliasing "beep" artifacts
            // in effect-heavy scenes without changing the external output rate.
            let ym_oversample_u64 = 2u64;
            let ym_oversample_i32 = 2i32;
            let mut ym_left_acc = 0i32;
            let mut ym_right_acc = 0i32;
            for _ in 0..ym_oversample_u64 {
                let (l, r) = self
                    .ym2612
                    .next_sample_stereo((sample_rate_hz * ym_oversample_u64) as f32);
                ym_left_acc += l as i32;
                ym_right_acc += r as i32;
            }
            let ym_left = (ym_left_acc / ym_oversample_i32) as i16;
            let ym_right = (ym_right_acc / ym_oversample_i32) as i16;
            let left = (psg_sample + ym_left as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            let right =
                (psg_sample + ym_right as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            self.sample_buffer.push(left);
            self.sample_buffer.push(right);
        }
    }

    pub fn ym2612(&self) -> &Ym2612 {
        &self.ym2612
    }

    pub fn psg(&self) -> &Psg {
        &self.psg
    }

    pub fn ym_write_count(&self) -> u64 {
        self.ym2612.writes()
    }

    pub fn ym_dac_write_count(&self) -> u64 {
        self.ym2612.dac_data_writes()
    }

    pub fn psg_write_count(&self) -> u64 {
        self.psg.writes()
    }

    pub fn ym_writes_from_68k(&self) -> u64 {
        self.ym_writes_from_68k
    }

    pub fn ym_writes_from_z80(&self) -> u64 {
        self.ym_writes_from_z80
    }

    pub fn psg_writes_from_68k(&self) -> u64 {
        self.psg_writes_from_68k
    }

    pub fn psg_writes_from_z80(&self) -> u64 {
        self.psg_writes_from_z80
    }

    pub fn pending_samples(&self) -> usize {
        self.sample_buffer.len()
    }

    pub fn drain_samples(&mut self, max_samples: usize) -> Vec<i16> {
        let count = max_samples.min(self.sample_buffer.len());
        self.sample_buffer.drain(0..count).collect()
    }
}

impl Default for AudioBus {
    fn default() -> Self {
        Self {
            ym2612: Ym2612::default(),
            psg: Psg::default(),
            ym_writes_from_68k: 0,
            ym_writes_from_z80: 0,
            psg_writes_from_68k: 0,
            psg_writes_from_z80: 0,
            cycles: 0,
            output_sample_rate_hz: Self::DEFAULT_OUTPUT_SAMPLE_RATE_HZ,
            sample_accumulator: 0,
            sample_buffer: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Psg, Ym2612};

    fn write_ym_reg(ym: &mut Ym2612, bank: usize, reg: u8, value: u8) {
        let (addr_port, data_port) = if (bank & 1) == 0 {
            (0u8, 1u8)
        } else {
            (2u8, 3u8)
        };
        ym.write_port(addr_port, reg);
        ym.write_port(data_port, value);
    }

    #[test]
    fn psg_data_byte_updates_latched_volume_register() {
        let mut psg = Psg::default();

        // Latch channel 2 volume register and set attenuation=3.
        psg.write_data(0b1101_0011);
        assert_eq!(psg.attenuation(2), 0x03);

        // Data byte without latch should keep target register and update attenuation.
        psg.write_data(0x0B);
        assert_eq!(psg.attenuation(2), 0x0B);
    }

    #[test]
    fn psg_data_byte_updates_latched_tone_period_high_bits() {
        let mut psg = Psg::default();

        // Latch channel 0 tone low nibble = 0x5.
        psg.write_data(0b1000_0101);
        // Data byte sets upper 6 bits = 0x12.
        psg.write_data(0x12);

        assert_eq!(psg.tone_period(0), 0x125);
    }

    #[test]
    fn ym2612_pms_depth_table_matches_documented_semitone_steps() {
        // PMS documented ranges (in semitones): 0, 0.034, 0.067, 0.10, 0.14,
        // 0.20, 0.40, 0.80. Convert to multiplicative ratio delta.
        let expected = [
            0.0f32,
            2f32.powf(0.034 / 12.0) - 1.0,
            2f32.powf(0.067 / 12.0) - 1.0,
            2f32.powf(0.10 / 12.0) - 1.0,
            2f32.powf(0.14 / 12.0) - 1.0,
            2f32.powf(0.20 / 12.0) - 1.0,
            2f32.powf(0.40 / 12.0) - 1.0,
            2f32.powf(0.80 / 12.0) - 1.0,
        ];
        for (idx, expected_depth) in expected.iter().enumerate() {
            let got = Ym2612::channel_fms_depth(idx as u8);
            assert!(
                (got - expected_depth).abs() < 0.0001,
                "pms={} expected={} got={}",
                idx,
                expected_depth,
                got
            );
        }
    }

    #[test]
    fn ym2612_channel3_special_mode_uses_ym3438_slot_mapping() {
        let mut ym = Ym2612::default();

        // Channel 3 normal frequency (used by operator 4 in CH3 special mode).
        write_ym_reg(&mut ym, 0, 0xA2, 0x34);
        write_ym_reg(&mut ym, 0, 0xA6, 0x21); // block=4, fnum high=1

        // CH3 special slot frequencies:
        // slot0 (A8/AC) -> operator 3
        // slot1 (A9/AD) -> operator 1
        // slot2 (AA/AE) -> operator 2
        write_ym_reg(&mut ym, 0, 0xA8, 0x11);
        write_ym_reg(&mut ym, 0, 0xAC, 0x18); // block=3, fnum high=0
        write_ym_reg(&mut ym, 0, 0xA9, 0x22);
        write_ym_reg(&mut ym, 0, 0xAD, 0x29); // block=5, fnum high=1
        write_ym_reg(&mut ym, 0, 0xAA, 0x33);
        write_ym_reg(&mut ym, 0, 0xAE, 0x31); // block=6, fnum high=1

        // Enable CH3 special mode (mode bits 01 in reg 0x27).
        write_ym_reg(&mut ym, 0, 0x27, 0x40);

        let op1 = ym.channel_operator_frequency_hz_debug(2, 0);
        let op2 = ym.channel_operator_frequency_hz_debug(2, 1);
        let op3 = ym.channel_operator_frequency_hz_debug(2, 2);
        let op4 = ym.channel_operator_frequency_hz_debug(2, 3);

        let expected_op1 = Ym2612::fnum_block_frequency_hz(0x122, 5);
        let expected_op2 = Ym2612::fnum_block_frequency_hz(0x133, 6);
        let expected_op3 = Ym2612::fnum_block_frequency_hz(0x011, 3);
        let expected_op4 = Ym2612::fnum_block_frequency_hz(0x134, 4);

        assert!(
            (op1 - expected_op1).abs() < 0.01,
            "op1={} exp={}",
            op1,
            expected_op1
        );
        assert!(
            (op2 - expected_op2).abs() < 0.01,
            "op2={} exp={}",
            op2,
            expected_op2
        );
        assert!(
            (op3 - expected_op3).abs() < 0.01,
            "op3={} exp={}",
            op3,
            expected_op3
        );
        assert!(
            (op4 - expected_op4).abs() < 0.01,
            "op4={} exp={}",
            op4,
            expected_op4
        );
    }

    #[test]
    fn ym2612_channel3_without_special_mode_uses_normal_frequency_for_all_operators() {
        let mut ym = Ym2612::default();
        write_ym_reg(&mut ym, 0, 0xA2, 0x56);
        write_ym_reg(&mut ym, 0, 0xA6, 0x2B); // block=5, fnum high=3

        // Program distinct CH3 special slot values, but keep special mode off.
        write_ym_reg(&mut ym, 0, 0xA8, 0x01);
        write_ym_reg(&mut ym, 0, 0xAC, 0x10);
        write_ym_reg(&mut ym, 0, 0xA9, 0x02);
        write_ym_reg(&mut ym, 0, 0xAD, 0x18);
        write_ym_reg(&mut ym, 0, 0xAA, 0x03);
        write_ym_reg(&mut ym, 0, 0xAE, 0x20);

        let expected = Ym2612::fnum_block_frequency_hz(0x356, 5);
        for op in 0..4 {
            let got = ym.channel_operator_frequency_hz_debug(2, op);
            assert!(
                (got - expected).abs() < 0.01,
                "op{}={} exp={}",
                op + 1,
                got,
                expected
            );
        }
    }
}
