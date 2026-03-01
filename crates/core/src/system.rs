use crate::cartridge::Cartridge;
use crate::cpu::M68k;
use crate::input::{Button, ControllerType};
use crate::memory::MemoryMap;
use crate::vdp::{FRAME_HEIGHT, FRAME_WIDTH};

#[derive(Debug, Clone)]
pub struct Emulator {
    cpu: M68k,
    memory: MemoryMap,
}

impl Emulator {
    pub fn new(cartridge: Cartridge) -> Self {
        let mut emulator = Self {
            cpu: M68k::new(),
            memory: MemoryMap::new(cartridge),
        };
        emulator.reset();
        emulator
    }

    pub fn reset(&mut self) {
        self.cpu.reset(&mut self.memory);
    }

    pub fn step(&mut self) -> StepResult {
        let cpu_cycles = self.cpu.step(&mut self.memory);
        self.memory.step_subsystems(cpu_cycles);
        let frame_ready = self.memory.step_vdp(cpu_cycles);
        if frame_ready {
            self.memory.request_z80_interrupt();
        }

        StepResult {
            cpu_cycles,
            frame_ready,
            pc: self.cpu.pc(),
            total_cycles: self.cpu.cycles(),
            frame_count: self.memory.frame_count(),
        }
    }

    pub fn header(&self) -> &crate::cartridge::RomHeader {
        self.memory.cartridge().header()
    }

    pub fn frame_buffer(&self) -> &[u8] {
        self.memory.frame_buffer()
    }

    pub fn frame_width(&self) -> usize {
        FRAME_WIDTH
    }

    pub fn frame_height(&self) -> usize {
        FRAME_HEIGHT
    }

    pub fn set_button_pressed(&mut self, button: Button, pressed: bool) {
        self.memory.set_button_pressed(button, pressed);
    }

    pub fn set_button2_pressed(&mut self, button: Button, pressed: bool) {
        self.memory.set_button2_pressed(button, pressed);
    }

    pub fn set_controller_type(&mut self, player: u8, controller_type: ControllerType) {
        self.memory.set_controller_type(player, controller_type);
    }

    pub fn pending_audio_samples(&self) -> usize {
        self.memory.pending_audio_samples()
    }

    pub fn drain_audio_samples(&mut self, max_samples: usize) -> Vec<i16> {
        self.memory.drain_audio_samples(max_samples)
    }

    pub fn set_audio_output_sample_rate_hz(&mut self, hz: u32) {
        self.memory.set_audio_output_sample_rate_hz(hz);
    }

    pub fn audio_output_channels(&self) -> u8 {
        self.memory.audio_output_channels()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepResult {
    pub cpu_cycles: u32,
    pub frame_ready: bool,
    pub pc: u32,
    pub total_cycles: u64,
    pub frame_count: u64,
}

#[cfg(test)]
mod tests {
    use crate::{Cartridge, Emulator};

    #[test]
    fn advances_program_counter() {
        let mut rom = vec![0; 0x200];
        rom[4..8].copy_from_slice(&0x00000100u32.to_be_bytes());
        rom[0x100..0x102].copy_from_slice(&0x4E71u16.to_be_bytes()); // NOP

        let cart = Cartridge::from_bytes(rom).expect("valid rom");
        let mut emulator = Emulator::new(cart);

        let step = emulator.step();
        assert_eq!(step.pc, 0x00000102);
    }

    #[test]
    fn drains_audio_samples_through_emulator_api() {
        let mut rom = vec![0; 0x200];
        rom[4..8].copy_from_slice(&0x00000100u32.to_be_bytes());
        rom[0x100..0x102].copy_from_slice(&0x4E71u16.to_be_bytes()); // NOP loop

        let cart = Cartridge::from_bytes(rom).expect("valid rom");
        let mut emulator = Emulator::new(cart);

        for _ in 0..64 {
            emulator.step();
        }

        assert!(emulator.pending_audio_samples() > 0);
        let drained = emulator.drain_audio_samples(64);
        assert!(!drained.is_empty());
    }
}
