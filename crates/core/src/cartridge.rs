use std::fmt::{Display, Formatter};

const HEADER_MIN_SIZE: usize = 0x200;

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct Cartridge {
    rom: Vec<u8>,
    header: RomHeader,
    save_ram: Option<SaveRam>,
}

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
struct SaveRam {
    start: u32,
    end: u32,
    lane: SaveRamLane,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, bincode::Encode, bincode::Decode)]
enum SaveRamLane {
    Both,
    Even,
    Odd,
}

impl SaveRam {
    fn parse_from_header(rom: &[u8]) -> Option<Self> {
        if rom.len() < 0x1BC || &rom[0x1B0..0x1B2] != b"RA" {
            return None;
        }

        let start = read_u32_be(rom, 0x1B4) & 0x00FF_FFFF;
        let end = read_u32_be(rom, 0x1B8) & 0x00FF_FFFF;
        if end < start {
            return None;
        }

        let lane = if (start & 1) == 1 && (end & 1) == 1 {
            SaveRamLane::Odd
        } else if (start & 1) == 0 && (end & 1) == 0 {
            SaveRamLane::Even
        } else {
            SaveRamLane::Both
        };

        let len = match lane {
            SaveRamLane::Both => end.wrapping_sub(start).wrapping_add(1) as usize,
            SaveRamLane::Even | SaveRamLane::Odd => {
                end.wrapping_sub(start).wrapping_div(2).wrapping_add(1) as usize
            }
        };
        if len == 0 {
            return None;
        }

        Some(Self {
            start,
            end,
            lane,
            // Cartridge save RAM powers up to erased state.
            data: vec![0xFF; len],
        })
    }

    fn contains(&self, addr: u32) -> bool {
        addr >= self.start && addr <= self.end
    }

    fn offset_for_addr(&self, addr: u32) -> Option<usize> {
        if !self.contains(addr) {
            return None;
        }
        match self.lane {
            SaveRamLane::Both => Some((addr - self.start) as usize),
            SaveRamLane::Even => {
                if (addr & 1) == 0 {
                    Some(((addr - self.start) >> 1) as usize)
                } else {
                    None
                }
            }
            SaveRamLane::Odd => {
                if (addr & 1) == 1 {
                    Some(((addr - self.start) >> 1) as usize)
                } else {
                    None
                }
            }
        }
    }
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
        let save_ram = SaveRam::parse_from_header(&rom);
        Ok(Self {
            rom,
            header,
            save_ram,
        })
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

    pub fn has_save_ram(&self) -> bool {
        self.save_ram.is_some()
    }

    pub fn read_save_ram_u8(&self, addr: u32) -> Option<u8> {
        let save_ram = self.save_ram.as_ref()?;
        if !save_ram.contains(addr) {
            return None;
        }
        Some(
            save_ram
                .offset_for_addr(addr)
                .and_then(|idx| save_ram.data.get(idx).copied())
                .unwrap_or(0xFF),
        )
    }

    pub fn write_save_ram_u8(&mut self, addr: u32, value: u8) -> bool {
        let Some(save_ram) = self.save_ram.as_mut() else {
            return false;
        };
        if !save_ram.contains(addr) {
            return false;
        }
        if let Some(idx) = save_ram.offset_for_addr(addr)
            && let Some(slot) = save_ram.data.get_mut(idx)
        {
            *slot = value;
        }
        true
    }
}

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
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

#[derive(Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode)]
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

    #[test]
    fn parses_backup_ram_header_and_maps_odd_lane() {
        let mut rom = vec![0u8; 0x400];
        rom[0x1B0..0x1B2].copy_from_slice(b"RA");
        rom[0x1B4..0x1B8].copy_from_slice(&0x0020_0001u32.to_be_bytes());
        rom[0x1B8..0x1BC].copy_from_slice(&0x0020_0007u32.to_be_bytes());

        let mut cart = Cartridge::from_bytes(rom).expect("valid rom");
        assert!(cart.has_save_ram());
        assert_eq!(cart.read_save_ram_u8(0x0020_0001), Some(0xFF));
        assert_eq!(cart.read_save_ram_u8(0x0020_0000), None);
        assert_eq!(cart.read_save_ram_u8(0x0020_0002), Some(0xFF));

        assert!(cart.write_save_ram_u8(0x0020_0001, 0x12));
        assert_eq!(cart.read_save_ram_u8(0x0020_0001), Some(0x12));
        // Even lane is not writable in odd-lane SRAM range.
        assert!(cart.write_save_ram_u8(0x0020_0002, 0x34));
        assert_eq!(cart.read_save_ram_u8(0x0020_0001), Some(0x12));
        assert_eq!(cart.read_save_ram_u8(0x0020_0002), Some(0xFF));
    }
}
