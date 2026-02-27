use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy)]
struct YmChannel {
    fnum: u16,
    block: u8,
    key_on: bool,
    phase: f32,
}

impl Default for YmChannel {
    fn default() -> Self {
        Self {
            fnum: 0x200,
            block: 4,
            key_on: false,
            phase: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ym2612 {
    addr_port0: u8,
    addr_port1: u8,
    regs: [[u8; 256]; 2],
    writes: u64,
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
            dac_enabled: false,
            dac_output: 0,
            channels: [YmChannel::default(); 6],
        }
    }
}

impl Ym2612 {
    fn write_port(&mut self, port: u8, value: u8) {
        match port & 0x03 {
            0 => self.addr_port0 = value,
            1 => {
                let reg = self.addr_port0;
                self.regs[0][reg as usize] = value;
                self.apply_write(0, reg, value);
                self.writes += 1;
            }
            2 => self.addr_port1 = value,
            3 => {
                let reg = self.addr_port1;
                self.regs[1][reg as usize] = value;
                self.apply_write(1, reg, value);
                self.writes += 1;
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
        }

        if bank == 0 {
            match reg {
                0x28 => {
                    if let Some(channel) = Self::decode_keyon_channel(value) {
                        let next_key_on = (value & 0xF0) != 0;
                        if next_key_on && !self.channels[channel].key_on {
                            self.channels[channel].phase = 0.0;
                        }
                        self.channels[channel].key_on = next_key_on;
                    }
                }
                0x2A => {
                    let centered = value as i16 - 0x80;
                    self.dac_output = centered << 8;
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

    fn channel_frequency_hz(channel: &YmChannel) -> f32 {
        let fnum_scale = (channel.fnum.max(1) as f32) / 1024.0;
        let octave_scale = 2f32.powi(channel.block as i32 - 4);
        let freq = 220.0 * fnum_scale * octave_scale;
        freq.clamp(20.0, 12_000.0)
    }

    pub fn writes(&self) -> u64 {
        self.writes
    }

    pub fn active_channels(&self) -> usize {
        self.channels
            .iter()
            .enumerate()
            .filter(|(index, channel)| {
                let dac_channel = *index == 5;
                channel.key_on && !(dac_channel && self.dac_enabled)
            })
            .count()
    }

    fn next_sample(&mut self, sample_rate_hz: f32) -> i16 {
        let mut fm_mix = 0.0f32;
        let mut active = 0usize;
        for (index, channel) in self.channels.iter_mut().enumerate() {
            if !channel.key_on {
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
            fm_mix += (channel.phase * TAU).sin();
            active += 1;
        }
        let fm_sample = if active == 0 {
            0
        } else {
            ((fm_mix / active as f32) * 7_500.0).clamp(i16::MIN as f32, i16::MAX as f32) as i16
        };
        let dac_sample = if self.dac_enabled { self.dac_output } else { 0 };
        (fm_sample as i32 + dac_sample as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    fn read_status(&self) -> u8 {
        0
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
            self.tone_period[self.latched_channel] = (lo | hi).max(1);
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

    fn apply_latched_data(&mut self, data: u8) {
        if self.latched_is_volume {
            self.attenuation[self.latched_channel] = data & 0x0F;
            return;
        }

        if self.latched_channel < 3 {
            let hi = self.tone_period[self.latched_channel] & 0x03F0;
            self.tone_period[self.latched_channel] = (hi | data as u16).max(1);
        } else {
            self.noise_control = data & 0x07;
            self.noise_lfsr = 0x8000;
        }
    }

    fn tone_frequency_hz(&self, channel: usize) -> f32 {
        let period = self.tone_period[channel.min(2)].max(1) as f32;
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
        for channel in 0..3 {
            self.tone_phase_acc[channel] += self.tone_frequency_hz(channel) / sample_rate_hz;
            while self.tone_phase_acc[channel] >= 1.0 {
                self.tone_phase_acc[channel] -= 1.0;
                self.tone_phase_high[channel] = !self.tone_phase_high[channel];
            }
        }

        self.noise_phase_acc += self.noise_frequency_hz() / sample_rate_hz;
        while self.noise_phase_acc >= 1.0 {
            self.noise_phase_acc -= 1.0;
            self.clock_noise_lfsr();
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

#[derive(Debug, Clone, Default)]
pub struct AudioBus {
    ym2612: Ym2612,
    psg: Psg,
    cycles: u64,
    sample_accumulator: u64,
    sample_buffer: Vec<i16>,
}

impl AudioBus {
    const M68K_CLOCK_HZ: u64 = 7_670_000;
    const OUTPUT_SAMPLE_RATE_HZ: u64 = 44_100;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_ym2612(&self, port: u8) -> u8 {
        if (port & 0x01) == 0 {
            self.ym2612.read_status()
        } else {
            0xFF
        }
    }

    pub fn write_ym2612(&mut self, port: u8, value: u8) {
        self.ym2612.write_port(port, value);
    }

    pub fn write_psg(&mut self, value: u8) {
        self.psg.write_data(value);
    }

    pub fn step(&mut self, m68k_cycles: u32) {
        self.cycles += m68k_cycles as u64;
        self.sample_accumulator += m68k_cycles as u64 * Self::OUTPUT_SAMPLE_RATE_HZ;
        let produced = (self.sample_accumulator / Self::M68K_CLOCK_HZ) as usize;
        self.sample_accumulator %= Self::M68K_CLOCK_HZ;
        for _ in 0..produced {
            let psg_sample = self.psg.next_sample(Self::OUTPUT_SAMPLE_RATE_HZ as f32) as i32;
            let ym_sample = self.ym2612.next_sample(Self::OUTPUT_SAMPLE_RATE_HZ as f32) as i32;
            let mixed = (psg_sample + ym_sample).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            self.sample_buffer.push(mixed);
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

    pub fn psg_write_count(&self) -> u64 {
        self.psg.writes()
    }

    pub fn pending_samples(&self) -> usize {
        self.sample_buffer.len()
    }

    pub fn drain_samples(&mut self, max_samples: usize) -> Vec<i16> {
        let count = max_samples.min(self.sample_buffer.len());
        self.sample_buffer.drain(0..count).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::AudioBus;

    #[test]
    fn writes_ym2612_registers_via_address_and_data_ports() {
        let mut audio = AudioBus::new();

        audio.write_ym2612(0, 0x22);
        audio.write_ym2612(1, 0x0F);
        audio.write_ym2612(2, 0x2B);
        audio.write_ym2612(3, 0x80);

        assert_eq!(audio.ym2612().register(0, 0x22), 0x0F);
        assert_eq!(audio.ym2612().register(1, 0x2B), 0x80);
    }

    #[test]
    fn captures_psg_writes() {
        let mut audio = AudioBus::new();
        audio.write_psg(0x9F);
        assert_eq!(audio.psg().last_data(), 0x9F);
        assert_eq!(audio.psg().writes(), 1);
    }

    #[test]
    fn generates_silence_samples_without_psg_writes() {
        let mut audio = AudioBus::new();
        audio.step(2_000);
        assert!(audio.pending_samples() > 0);
        let samples = audio.drain_samples(64);
        assert!(samples.iter().all(|&s| s == 0));
    }

    #[test]
    fn generates_nonzero_samples_after_psg_write() {
        let mut audio = AudioBus::new();
        audio.write_psg(0x90); // low attenuation -> larger amplitude
        audio.step(2_000);

        let samples = audio.drain_samples(64);
        assert!(!samples.is_empty());
        assert!(samples.iter().any(|&s| s > 0));
        assert!(samples.iter().any(|&s| s < 0));
    }

    #[test]
    fn psg_latch_and_data_bytes_update_tone_period() {
        let mut audio = AudioBus::new();
        // Latch tone 0 low nibble = 0xA.
        audio.write_psg(0x8A);
        // Data byte sets high bits = 0x12.
        audio.write_psg(0x12);
        assert_eq!(audio.psg().tone_period(0), 0x12A);
    }

    #[test]
    fn psg_noise_latch_updates_control_register() {
        let mut audio = AudioBus::new();
        // Latch noise control: white noise + clock mode 2.
        audio.write_psg(0xE6);
        assert_eq!(audio.psg().noise_control(), 0x06);
    }

    #[test]
    fn ym2612_dac_outputs_pcm_when_enabled() {
        let mut audio = AudioBus::new();
        audio.write_ym2612(0, 0x2B);
        audio.write_ym2612(1, 0x80);
        audio.write_ym2612(0, 0x2A);
        audio.write_ym2612(1, 0xFF);
        audio.step(2_000);

        let samples = audio.drain_samples(64);
        assert!(!samples.is_empty());
        assert!(audio.ym2612().dac_enabled());
        assert!(samples.iter().all(|&s| s > 0));
    }

    #[test]
    fn ym2612_dac_is_silent_when_disabled() {
        let mut audio = AudioBus::new();
        audio.write_ym2612(0, 0x2A);
        audio.write_ym2612(1, 0xFF);
        audio.step(2_000);

        let samples = audio.drain_samples(64);
        assert!(!samples.is_empty());
        assert!(samples.iter().all(|&s| s == 0));
    }

    #[test]
    fn ym2612_key_on_generates_nonzero_without_dac() {
        let mut audio = AudioBus::new();
        // CH1 FNUM/BLOCK
        audio.write_ym2612(0, 0xA0);
        audio.write_ym2612(1, 0x98);
        audio.write_ym2612(0, 0xA4);
        audio.write_ym2612(1, 0x22);
        // Key on CH1
        audio.write_ym2612(0, 0x28);
        audio.write_ym2612(1, 0xF0);
        audio.step(2_000);

        let samples = audio.drain_samples(128);
        assert!(!samples.is_empty());
        assert!(audio.ym2612().channel_key_on(0));
        assert!(samples.iter().any(|&s| s != 0));
    }

    #[test]
    fn ym2612_key_off_silences_channel() {
        let mut audio = AudioBus::new();
        audio.write_ym2612(0, 0xA0);
        audio.write_ym2612(1, 0xA0);
        audio.write_ym2612(0, 0xA4);
        audio.write_ym2612(1, 0x24);
        audio.write_ym2612(0, 0x28);
        audio.write_ym2612(1, 0xF0);
        audio.step(2_000);
        let _ = audio.drain_samples(128);

        audio.write_ym2612(0, 0x28);
        audio.write_ym2612(1, 0x00);
        audio.step(2_000);
        let samples = audio.drain_samples(128);
        assert!(!samples.is_empty());
        assert!(samples.iter().all(|&s| s == 0));
    }
}
