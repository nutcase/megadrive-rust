use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum YmEnvelopePhase {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy)]
struct YmChannel {
    fnum: u16,
    block: u8,
    key_on: bool,
    phase: f32,
    algorithm: u8,
    feedback: u8,
    feedback_sample: f32,
    pan_left: bool,
    pan_right: bool,
    carrier_mul: u8,
    carrier_tl: u8,
    attack_rate: u8,
    decay_rate: u8,
    sustain_rate: u8,
    sustain_level: u8,
    release_rate: u8,
    envelope_phase: YmEnvelopePhase,
    envelope_level: f32,
}

impl Default for YmChannel {
    fn default() -> Self {
        Self {
            fnum: 0x200,
            block: 4,
            key_on: false,
            phase: 0.0,
            algorithm: 0,
            feedback: 0,
            feedback_sample: 0.0,
            pan_left: true,
            pan_right: true,
            carrier_mul: 1,
            carrier_tl: 0,
            attack_rate: 31,
            decay_rate: 0,
            sustain_rate: 0,
            sustain_level: 0,
            // Keep default release short to avoid lingering notes when a game
            // hasn't initialized operator envelopes yet.
            release_rate: 15,
            envelope_phase: YmEnvelopePhase::Off,
            envelope_level: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
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
            channels: [YmChannel::default(); 6],
        }
    }
}

impl Ym2612 {
    // YM2612 busy flag is asserted for a short write-cycle window.
    // Approximate in Z80 cycles (about a few microseconds).
    const BUSY_DURATION_Z80_CYCLES: u32 = 16;
    const MASTER_CLOCK_HZ: u64 = 7_670_454;
    const Z80_CLOCK_HZ: u64 = 3_579_545;
    const YM2612_DIVIDER: u64 = 7;

    fn write_port(&mut self, port: u8, value: u8) {
        match port & 0x03 {
            0 => self.addr_port0 = value,
            1 => {
                let reg = self.addr_port0;
                self.regs[0][reg as usize] = value;
                self.apply_write(0, reg, value);
                self.writes += 1;
                self.busy_z80_cycles = Self::BUSY_DURATION_Z80_CYCLES;
            }
            2 => self.addr_port1 = value,
            3 => {
                let reg = self.addr_port1;
                self.regs[1][reg as usize] = value;
                self.apply_write(1, reg, value);
                self.writes += 1;
                self.busy_z80_cycles = Self::BUSY_DURATION_Z80_CYCLES;
            }
            _ => {}
        }
    }

    fn apply_write(&mut self, bank: usize, reg: u8, value: u8) {
        if let Some(channel) = self.decode_fnum_low_channel(bank, reg) {
            self.channels[channel].fnum = (self.channels[channel].fnum & 0x0700) | value as u16;
        } else if let Some(channel) = self.decode_fnum_high_channel(bank, reg) {
            self.channels[channel].fnum =
                (self.channels[channel].fnum & 0x00FF) | (((value & 0x07) as u16) << 8);
            self.channels[channel].block = (value >> 3) & 0x07;
        } else if let Some(channel) = self.decode_pan_channel(bank, reg) {
            self.channels[channel].pan_left = (value & 0x80) != 0;
            self.channels[channel].pan_right = (value & 0x40) != 0;
        } else if let Some(channel) = self.decode_algorithm_channel(bank, reg) {
            self.channels[channel].algorithm = value & 0x07;
            self.channels[channel].feedback = (value >> 3) & 0x07;
        } else if let Some(channel) = self.decode_carrier_mul_channel(bank, reg) {
            self.channels[channel].carrier_mul = value & 0x0F;
        } else if let Some(channel) = self.decode_carrier_tl_channel(bank, reg) {
            self.channels[channel].carrier_tl = value & 0x7F;
        } else if let Some(channel) = self.decode_carrier_attack_channel(bank, reg) {
            self.channels[channel].attack_rate = value & 0x1F;
        } else if let Some(channel) = self.decode_carrier_decay_channel(bank, reg) {
            self.channels[channel].decay_rate = value & 0x1F;
        } else if let Some(channel) = self.decode_carrier_sustain_rate_channel(bank, reg) {
            self.channels[channel].sustain_rate = value & 0x1F;
        } else if let Some(channel) = self.decode_carrier_sustain_release_channel(bank, reg) {
            self.channels[channel].sustain_level = (value >> 4) & 0x0F;
            self.channels[channel].release_rate = value & 0x0F;
        }

        if bank == 0 {
            match reg {
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
                    let previous = self.timer_control;
                    self.timer_control = value;
                    if (value & 0x10) != 0 {
                        self.timer_status &= !0x01;
                    }
                    if (value & 0x20) != 0 {
                        self.timer_status &= !0x02;
                    }
                    if (value & 0x01) != 0 && (previous & 0x01) == 0 {
                        self.timer_a_elapsed_ym_cycles = 0;
                    }
                    if (value & 0x02) != 0 && (previous & 0x02) == 0 {
                        self.timer_b_elapsed_ym_cycles = 0;
                    }
                }
                0x28 => {
                    if let Some(channel) = Self::decode_keyon_channel(value) {
                        let next_key_on = (value & 0xF0) != 0;
                        if next_key_on && !self.channels[channel].key_on {
                            self.channels[channel].phase = 0.0;
                            self.channels[channel].feedback_sample = 0.0;
                            self.channels[channel].envelope_phase = YmEnvelopePhase::Attack;
                            self.channels[channel].envelope_level = 0.0;
                        } else if !next_key_on && self.channels[channel].key_on {
                            self.channels[channel].envelope_phase =
                                if self.channels[channel].envelope_level > 0.0 {
                                    YmEnvelopePhase::Release
                                } else {
                                    YmEnvelopePhase::Off
                                };
                        }
                        self.channels[channel].key_on = next_key_on;
                    }
                }
                0x2A => {
                    let centered = value as i16 - 0x80;
                    self.dac_output = centered << 8;
                    self.dac_data_writes += 1;
                }
                0x2B => {
                    self.dac_enabled = (value & 0x80) != 0;
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

    fn decode_carrier_mul_channel(&self, bank: usize, reg: u8) -> Option<usize> {
        if (0x3C..=0x3E).contains(&reg) {
            Some((bank & 1) * 3 + (reg as usize - 0x3C))
        } else {
            None
        }
    }

    fn decode_carrier_tl_channel(&self, bank: usize, reg: u8) -> Option<usize> {
        if (0x4C..=0x4E).contains(&reg) {
            Some((bank & 1) * 3 + (reg as usize - 0x4C))
        } else {
            None
        }
    }

    fn decode_carrier_attack_channel(&self, bank: usize, reg: u8) -> Option<usize> {
        if (0x5C..=0x5E).contains(&reg) {
            Some((bank & 1) * 3 + (reg as usize - 0x5C))
        } else {
            None
        }
    }

    fn decode_carrier_decay_channel(&self, bank: usize, reg: u8) -> Option<usize> {
        if (0x6C..=0x6E).contains(&reg) {
            Some((bank & 1) * 3 + (reg as usize - 0x6C))
        } else {
            None
        }
    }

    fn decode_carrier_sustain_rate_channel(&self, bank: usize, reg: u8) -> Option<usize> {
        if (0x7C..=0x7E).contains(&reg) {
            Some((bank & 1) * 3 + (reg as usize - 0x7C))
        } else {
            None
        }
    }

    fn decode_carrier_sustain_release_channel(&self, bank: usize, reg: u8) -> Option<usize> {
        if (0x8C..=0x8E).contains(&reg) {
            Some((bank & 1) * 3 + (reg as usize - 0x8C))
        } else {
            None
        }
    }

    fn channel_frequency_hz(channel: &YmChannel) -> f32 {
        let fnum_scale = (channel.fnum.max(1) as f32) / 1024.0;
        let octave_scale = 2f32.powi(channel.block as i32 - 4);
        let multiple = (channel.carrier_mul.max(1)) as f32;
        let freq = 220.0 * fnum_scale * octave_scale * multiple;
        freq.clamp(20.0, 12_000.0)
    }

    fn channel_level_scale(channel: &YmChannel) -> f32 {
        if channel.carrier_tl >= 0x7F {
            return 0.0;
        }
        let attenuation_db = channel.carrier_tl as f32 * 0.75;
        10f32.powf(-attenuation_db / 20.0)
    }

    fn channel_algorithm_gain(channel: &YmChannel) -> f32 {
        // Approximate YM2612 algorithm output scaling by carrier count.
        // This keeps relative loudness closer across algorithms in this
        // simplified one-operator-per-channel model.
        const GAIN_BY_ALG: [f32; 8] = [0.35, 0.35, 0.35, 0.35, 0.55, 0.75, 0.75, 1.0];
        GAIN_BY_ALG[(channel.algorithm & 0x07) as usize]
    }

    fn rate_to_step(rate: u8, sample_rate_hz: f32, min_seconds: f32, max_seconds: f32) -> f32 {
        if rate == 0 || sample_rate_hz <= 0.0 {
            return 0.0;
        }
        let ratio = rate.min(31) as f32 / 31.0;
        let seconds = max_seconds * (min_seconds / max_seconds).powf(ratio);
        (1.0 / (seconds * sample_rate_hz)).clamp(0.0, 1.0)
    }

    fn sustain_level_to_amplitude(level: u8) -> f32 {
        if level >= 0x0F {
            0.0
        } else {
            1.0 - (level as f32 / 15.0)
        }
    }

    fn release_rate_to_step(rate: u8, sample_rate_hz: f32) -> f32 {
        if sample_rate_hz <= 0.0 {
            return 0.0;
        }
        let ratio = rate.min(15) as f32 / 15.0;
        let seconds = 0.8_f32 * (0.004_f32 / 0.8_f32).powf(ratio);
        (1.0 / (seconds * sample_rate_hz)).clamp(0.0, 1.0)
    }

    fn advance_envelope(channel: &mut YmChannel, sample_rate_hz: f32) -> f32 {
        match channel.envelope_phase {
            YmEnvelopePhase::Off => {
                channel.envelope_level = 0.0;
            }
            YmEnvelopePhase::Attack => {
                let step = Self::rate_to_step(channel.attack_rate, sample_rate_hz, 0.002, 0.8);
                if step <= 0.0 {
                    channel.envelope_level = 1.0;
                    channel.envelope_phase = YmEnvelopePhase::Decay;
                } else {
                    channel.envelope_level = (channel.envelope_level + step).min(1.0);
                    if channel.envelope_level >= 1.0 {
                        channel.envelope_phase = YmEnvelopePhase::Decay;
                    }
                }
            }
            YmEnvelopePhase::Decay => {
                let sustain_target = Self::sustain_level_to_amplitude(channel.sustain_level);
                let step = Self::rate_to_step(channel.decay_rate, sample_rate_hz, 0.01, 3.0);
                if step <= 0.0 || channel.envelope_level <= sustain_target {
                    channel.envelope_level = sustain_target;
                    channel.envelope_phase = YmEnvelopePhase::Sustain;
                } else {
                    channel.envelope_level = (channel.envelope_level - step).max(sustain_target);
                    if channel.envelope_level <= sustain_target {
                        channel.envelope_phase = YmEnvelopePhase::Sustain;
                    }
                }
            }
            YmEnvelopePhase::Sustain => {
                let step = Self::rate_to_step(channel.sustain_rate, sample_rate_hz, 0.03, 5.0);
                if step > 0.0 {
                    channel.envelope_level = (channel.envelope_level - step).max(0.0);
                }
            }
            YmEnvelopePhase::Release => {
                let step = Self::release_rate_to_step(channel.release_rate, sample_rate_hz);
                if step <= 0.0 {
                    channel.envelope_level = 0.0;
                    channel.envelope_phase = YmEnvelopePhase::Off;
                } else {
                    channel.envelope_level = (channel.envelope_level - step).max(0.0);
                    if channel.envelope_level <= 0.0 {
                        channel.envelope_phase = YmEnvelopePhase::Off;
                    }
                }
            }
        }
        channel.envelope_level
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
            .filter(|(index, channel)| {
                let dac_channel = *index == 5;
                (channel.key_on || channel.envelope_phase != YmEnvelopePhase::Off)
                    && !(dac_channel && self.dac_enabled)
            })
            .count()
    }

    fn next_sample_stereo(&mut self, sample_rate_hz: f32) -> (i16, i16) {
        let mut left_mix = 0.0f32;
        let mut right_mix = 0.0f32;
        let mut left_active = 0usize;
        let mut right_active = 0usize;
        for (index, channel) in self.channels.iter_mut().enumerate() {
            if !channel.key_on && channel.envelope_phase == YmEnvelopePhase::Off {
                continue;
            }
            if index == 5 && self.dac_enabled {
                continue;
            }
            let freq = Self::channel_frequency_hz(channel);
            channel.phase += freq / sample_rate_hz;
            if channel.phase >= 1.0 {
                channel.phase -= channel.phase.floor();
            }
            let feedback_amount = (channel.feedback as f32 / 7.0) * 0.2;
            let modulated_phase = channel.phase + channel.feedback_sample * feedback_amount;
            let envelope = Self::advance_envelope(channel, sample_rate_hz);
            let sample = (modulated_phase * TAU).sin()
                * Self::channel_level_scale(channel)
                * envelope
                * Self::channel_algorithm_gain(channel);
            channel.feedback_sample = sample;
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
            ((left_mix / left_active as f32) * 7_500.0).clamp(i16::MIN as f32, i16::MAX as f32)
                as i16
        };
        let fm_right = if right_active == 0 {
            0
        } else {
            ((right_mix / right_active as f32) * 7_500.0).clamp(i16::MIN as f32, i16::MAX as f32)
                as i16
        };
        let (dac_left, dac_right) = if self.dac_enabled {
            let channel = &self.channels[5];
            let left = if channel.pan_left { self.dac_output } else { 0 };
            let right = if channel.pan_right {
                self.dac_output
            } else {
                0
            };
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
        self.channels[channel.min(5)].key_on
    }

    pub fn channel_frequency_hz_debug(&self, channel: usize) -> f32 {
        Self::channel_frequency_hz(&self.channels[channel.min(5)])
    }

    pub fn channel_carrier_mul(&self, channel: usize) -> u8 {
        self.channels[channel.min(5)].carrier_mul
    }

    pub fn channel_carrier_tl(&self, channel: usize) -> u8 {
        self.channels[channel.min(5)].carrier_tl
    }

    pub fn channel_algorithm_feedback(&self, channel: usize) -> (u8, u8) {
        let channel = self.channels[channel.min(5)];
        (channel.algorithm, channel.feedback)
    }

    pub fn channel_envelope_level(&self, channel: usize) -> f32 {
        self.channels[channel.min(5)].envelope_level
    }

    pub fn channel_envelope_params(&self, channel: usize) -> (u8, u8, u8, u8, u8) {
        let channel = self.channels[channel.min(5)];
        (
            channel.attack_rate,
            channel.decay_rate,
            channel.sustain_rate,
            channel.sustain_level,
            channel.release_rate,
        )
    }

    pub fn channel_block_and_fnum(&self, channel: usize) -> (u8, u16) {
        let channel = self.channels[channel.min(5)];
        (channel.block, channel.fnum)
    }
}

#[derive(Debug, Clone)]
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
            noise_lfsr: 0x8000,
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

        if !self.latched_is_volume && self.latched_channel < 3 {
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
            self.noise_lfsr = 0x8000;
            self.noise_phase_acc = 0.0;
        }
    }

    fn tone_frequency_hz(&self, channel: usize) -> f32 {
        let raw_period = self.tone_period[channel.min(2)] & 0x03FF;
        // SN76489-compatible behavior: period 0 is treated as divider 1024.
        let period = if raw_period == 0 { 1024 } else { raw_period } as f32;
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
        self.noise_lfsr = (self.noise_lfsr >> 1) | (feedback << 15);
    }

    fn next_sample(&mut self, sample_rate_hz: f32) -> i16 {
        let noise_uses_tone3 = (self.noise_control & 0x03) == 0x03;
        let mut tone3_falling_edges = 0usize;
        for channel in 0..3 {
            self.tone_phase_acc[channel] += self.tone_frequency_hz(channel) / sample_rate_hz;
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

        (mix * 3000.0).clamp(i16::MIN as f32, i16::MAX as f32) as i16
    }
}

#[derive(Debug, Clone)]
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
        self.ym_writes_from_z80 += 1;
        self.ym2612.write_port(port, value);
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
            let psg_sample = self.psg.next_sample(sample_rate_hz as f32) as i32;
            let (ym_left, ym_right) = self.ym2612.next_sample_stereo(sample_rate_hz as f32);
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
