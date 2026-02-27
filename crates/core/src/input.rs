#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    C,
    Start,
}

#[derive(Debug, Clone, Default)]
struct PadState {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    a: bool,
    b: bool,
    c: bool,
    start: bool,
}

impl PadState {
    fn set_button(&mut self, button: Button, pressed: bool) {
        match button {
            Button::Up => self.up = pressed,
            Button::Down => self.down = pressed,
            Button::Left => self.left = pressed,
            Button::Right => self.right = pressed,
            Button::A => self.a = pressed,
            Button::B => self.b = pressed,
            Button::C => self.c = pressed,
            Button::Start => self.start = pressed,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IoBus {
    version: u8,
    pad1: PadState,
    pad2: PadState,
    port1_data: u8,
    port1_ctrl: u8,
    port2_data: u8,
    port2_ctrl: u8,
}

impl Default for IoBus {
    fn default() -> Self {
        Self {
            // Default to JP/NTSC-compatible bits so JP-only ROMs can pass
            // early region checks during boot.
            version: 0x20,
            pad1: PadState::default(),
            pad2: PadState::default(),
            port1_data: 0x40,
            port1_ctrl: 0x40,
            port2_data: 0x40,
            port2_ctrl: 0x40,
        }
    }
}

impl IoBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_button_pressed(&mut self, button: Button, pressed: bool) {
        self.pad1.set_button(button, pressed);
    }

    pub fn set_button2_pressed(&mut self, button: Button, pressed: bool) {
        self.pad2.set_button(button, pressed);
    }

    pub fn read_version(&self) -> u8 {
        self.version
    }

    pub fn read_port1_data(&self) -> u8 {
        read_pad_data(&self.pad1, self.port1_data, self.port1_ctrl)
    }

    pub fn read_port2_data(&self) -> u8 {
        read_pad_data(&self.pad2, self.port2_data, self.port2_ctrl)
    }

    pub fn write_port1_data(&mut self, value: u8) {
        self.port1_data = value & 0x7F;
    }

    pub fn write_port2_data(&mut self, value: u8) {
        self.port2_data = value & 0x7F;
    }

    pub fn read_port1_ctrl(&self) -> u8 {
        self.port1_ctrl
    }

    pub fn read_port2_ctrl(&self) -> u8 {
        self.port2_ctrl
    }

    pub fn write_port1_ctrl(&mut self, value: u8) {
        self.port1_ctrl = value & 0x7F;
    }

    pub fn write_port2_ctrl(&mut self, value: u8) {
        self.port2_ctrl = value & 0x7F;
    }
}

fn read_pad_data(pad: &PadState, port_data: u8, port_ctrl: u8) -> u8 {
    // Start from output latch state. Inputs are then overlaid for bits configured
    // as input in the control register.
    let mut value = port_data & 0x7F;

    // TH is driven by the console only when configured as output; otherwise the
    // line is pulled high.
    let th_high = if (port_ctrl & 0x40) != 0 {
        (port_data & 0x40) != 0
    } else {
        true
    };

    let mut pad_input = if th_high {
        (active_low_bit(pad.up) << 0)
            | (active_low_bit(pad.down) << 1)
            | (active_low_bit(pad.left) << 2)
            | (active_low_bit(pad.right) << 3)
            | (active_low_bit(pad.b) << 4)
            | (active_low_bit(pad.c) << 5)
            | (1 << 6)
    } else {
        (active_low_bit(pad.up) << 0)
            | (active_low_bit(pad.down) << 1)
            | (0 << 2)
            | (0 << 3)
            | (active_low_bit(pad.a) << 4)
            | (active_low_bit(pad.start) << 5)
            | (0 << 6)
    };

    // If TH is configured as input, keep it high (pulled up).
    if (port_ctrl & 0x40) == 0 {
        pad_input |= 1 << 6;
    }

    // Bits set as input in control register are sourced from the controller.
    let input_mask = !port_ctrl & 0x7F;
    value = (value & !input_mask) | (pad_input & input_mask);
    value
}

fn active_low_bit(pressed: bool) -> u8 {
    if pressed { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::{Button, IoBus};

    #[test]
    fn reads_three_button_pad_with_th_high() {
        let mut io = IoBus::new();
        io.set_button_pressed(Button::Right, true);
        io.set_button_pressed(Button::B, true);

        assert_eq!(io.read_port1_data(), 0x67);
    }

    #[test]
    fn reads_start_and_a_with_th_low() {
        let mut io = IoBus::new();
        io.write_port1_data(0x00); // TH low
        io.set_button_pressed(Button::A, true);
        io.set_button_pressed(Button::Start, true);

        assert_eq!(io.read_port1_data(), 0x03);
    }

    #[test]
    fn reads_second_pad_independently() {
        let mut io = IoBus::new();
        io.set_button_pressed(Button::Right, true);
        io.set_button2_pressed(Button::Left, true);
        io.set_button2_pressed(Button::C, true);

        assert_eq!(io.read_port1_data(), 0x77);
        assert_eq!(io.read_port2_data(), 0x5B);
    }

    #[test]
    fn second_pad_th_low_exposes_a_and_start() {
        let mut io = IoBus::new();
        io.write_port2_data(0x00);
        io.set_button2_pressed(Button::A, true);
        io.set_button2_pressed(Button::Start, true);

        assert_eq!(io.read_port2_data(), 0x03);
    }

    #[test]
    fn control_register_keeps_output_bits_from_data_latch() {
        let mut io = IoBus::new();
        io.write_port1_ctrl(0x70);
        io.write_port1_data(0x10);
        io.set_button_pressed(Button::B, true);

        // Bit4 is configured as output, so the pressed-B input must not override it.
        assert_eq!(io.read_port1_data() & 0x10, 0x10);
    }

    #[test]
    fn version_register_is_exposed() {
        let io = IoBus::new();
        assert_eq!(io.read_version(), 0x20);
    }
}
