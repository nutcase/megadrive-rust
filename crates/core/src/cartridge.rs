use std::fmt::{Display, Formatter};

const HEADER_MIN_SIZE: usize = 0x200;

#[derive(Debug, Clone)]
pub struct Cartridge {
    rom: Vec<u8>,
    header: RomHeader,
}

impl Cartridge {
    pub fn from_bytes(rom: Vec<u8>) -> Result<Self, CartridgeError> {
        if rom.len() < HEADER_MIN_SIZE {
            return Err(CartridgeError::RomTooSmall {
                size: rom.len(),
                min_size: HEADER_MIN_SIZE,
            });
        }

        let header = RomHeader::parse(&rom);
        Ok(Self { rom, header })
    }

    pub fn len(&self) -> usize {
        self.rom.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rom.is_empty()
    }

    pub fn header(&self) -> &RomHeader {
        &self.header
    }

    pub fn read_u8(&self, addr: u32) -> u8 {
        let len = self.rom.len();
        if len == 0 {
            return 0xFF;
        }
        let index = (addr as usize) % len;
        self.rom[index]
    }
}

#[derive(Debug, Clone)]
pub struct RomHeader {
    pub console_name: String,
    pub domestic_title: String,
    pub overseas_title: String,
    pub product_code: String,
    pub checksum: u16,
    pub io_support: String,
    pub rom_start: u32,
    pub rom_end: u32,
    pub ram_start: u32,
    pub ram_end: u32,
    pub region: String,
}

impl RomHeader {
    fn parse(rom: &[u8]) -> Self {
        Self {
            console_name: read_ascii(rom, 0x100, 0x110),
            domestic_title: read_ascii(rom, 0x120, 0x150),
            overseas_title: read_ascii(rom, 0x150, 0x180),
            product_code: read_ascii(rom, 0x180, 0x18E),
            checksum: read_u16_be(rom, 0x18E),
            io_support: read_ascii(rom, 0x190, 0x1A0),
            rom_start: read_u32_be(rom, 0x1A0),
            rom_end: read_u32_be(rom, 0x1A4),
            ram_start: read_u32_be(rom, 0x1A8),
            ram_end: read_u32_be(rom, 0x1AC),
            region: read_ascii(rom, 0x1F0, 0x200),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CartridgeError {
    RomTooSmall { size: usize, min_size: usize },
}

impl Display for CartridgeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RomTooSmall { size, min_size } => {
                write!(
                    f,
                    "ROM image is too small: {size} bytes (minimum required: {min_size})"
                )
            }
        }
    }
}

impl std::error::Error for CartridgeError {}

fn read_ascii(rom: &[u8], start: usize, end: usize) -> String {
    let end = end.min(rom.len());
    let start = start.min(end);
    let bytes = &rom[start..end];
    let mut text = String::with_capacity(bytes.len());

    for &b in bytes {
        let c = if b.is_ascii_graphic() || b == b' ' {
            b as char
        } else {
            ' '
        };
        text.push(c);
    }

    text.trim().to_string()
}

fn read_u16_be(rom: &[u8], offset: usize) -> u16 {
    if offset + 1 >= rom.len() {
        return 0;
    }
    u16::from_be_bytes([rom[offset], rom[offset + 1]])
}

fn read_u32_be(rom: &[u8], offset: usize) -> u32 {
    if offset + 3 >= rom.len() {
        return 0;
    }
    u32::from_be_bytes([
        rom[offset],
        rom[offset + 1],
        rom[offset + 2],
        rom[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::{Cartridge, CartridgeError};

    #[test]
    fn parses_header_fields() {
        let mut rom = vec![0u8; 0x400];
        rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
        rom[0x120..0x126].copy_from_slice(b"SONIC ");
        rom[0x180..0x188].copy_from_slice(b"GM 00001");
        rom[0x18E..0x190].copy_from_slice(&0x4E71u16.to_be_bytes());
        rom[0x1F0..0x1F3].copy_from_slice(b"JUE");

        let cart = Cartridge::from_bytes(rom).expect("valid rom");
        let header = cart.header();

        assert_eq!(header.console_name, "SEGA MEGA DRIVE");
        assert_eq!(header.domestic_title, "SONIC");
        assert_eq!(header.product_code, "GM 00001");
        assert_eq!(header.checksum, 0x4E71);
        assert_eq!(header.region, "JUE");
    }

    #[test]
    fn rejects_too_small_rom() {
        let rom = vec![0u8; 0x100];
        let err = Cartridge::from_bytes(rom).expect_err("must fail");
        assert_eq!(
            err,
            CartridgeError::RomTooSmall {
                size: 0x100,
                min_size: 0x200
            }
        );
    }
}
