use serde::{Deserialize, Serialize};

pub const WORK_RAM_SIZE: usize = 0x10000;

#[derive(Clone)]
pub struct RamSnapshot {
    data: Vec<u8>,
}

impl RamSnapshot {
    pub fn capture(ram: &[u8]) -> Self {
        Self { data: ram.to_vec() }
    }

    pub fn get(&self, addr: u32) -> u8 {
        self.data.get(addr as usize).copied().unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchFilter {
    Equal(u8),
    NotEqual(u8),
    GreaterThan(u8),
    LessThan(u8),
    Increased,
    Decreased,
    Changed,
    Unchanged,
    IncreasedBy(u8),
    DecreasedBy(u8),
}

impl SearchFilter {
    pub fn needs_snapshot(&self) -> bool {
        matches!(
            self,
            Self::Increased
                | Self::Decreased
                | Self::Changed
                | Self::Unchanged
                | Self::IncreasedBy(_)
                | Self::DecreasedBy(_)
        )
    }
}

pub struct CheatSearch {
    snapshot: Option<RamSnapshot>,
    candidates: Vec<u32>,
    ram_size: usize,
}

impl CheatSearch {
    pub fn new() -> Self {
        Self {
            snapshot: None,
            candidates: (0..WORK_RAM_SIZE as u32).collect(),
            ram_size: WORK_RAM_SIZE,
        }
    }

    pub fn resize(&mut self, size: usize) {
        if size != self.ram_size {
            self.ram_size = size;
            self.snapshot = None;
            self.candidates = (0..size as u32).collect();
        }
    }

    pub fn snapshot(&mut self, ram: &[u8]) {
        if ram.len() != self.ram_size {
            self.resize(ram.len());
        }
        self.snapshot = Some(RamSnapshot::capture(ram));
    }

    pub fn has_snapshot(&self) -> bool {
        self.snapshot.is_some()
    }

    pub fn previous_snapshot(&self) -> Option<&RamSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn apply_filter(&mut self, filter: SearchFilter, current_ram: &[u8]) {
        if current_ram.len() != self.ram_size {
            self.resize(current_ram.len());
        }

        let snap = match &self.snapshot {
            Some(s) if filter.needs_snapshot() => s,
            _ if filter.needs_snapshot() => return,
            _ => {
                self.candidates.retain(|&addr| {
                    let cur = current_ram.get(addr as usize).copied().unwrap_or(0);
                    match filter {
                        SearchFilter::Equal(v) => cur == v,
                        SearchFilter::NotEqual(v) => cur != v,
                        SearchFilter::GreaterThan(v) => cur > v,
                        SearchFilter::LessThan(v) => cur < v,
                        _ => unreachable!(),
                    }
                });
                self.snapshot = Some(RamSnapshot::capture(current_ram));
                return;
            }
        };

        let snap_clone = snap.clone();
        self.candidates.retain(|&addr| {
            let cur = current_ram.get(addr as usize).copied().unwrap_or(0);
            let prev = snap_clone.get(addr);
            match filter {
                SearchFilter::Increased => cur > prev,
                SearchFilter::Decreased => cur < prev,
                SearchFilter::Changed => cur != prev,
                SearchFilter::Unchanged => cur == prev,
                SearchFilter::IncreasedBy(delta) => cur == prev.wrapping_add(delta),
                SearchFilter::DecreasedBy(delta) => cur == prev.wrapping_sub(delta),
                _ => unreachable!(),
            }
        });
        self.snapshot = Some(RamSnapshot::capture(current_ram));
    }

    pub fn reset(&mut self) {
        self.snapshot = None;
        self.candidates = (0..self.ram_size as u32).collect();
    }

    pub fn candidates(&self) -> &[u32] {
        &self.candidates
    }

    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CheatEntry {
    pub address: u32,
    pub value: u8,
    pub enabled: bool,
    pub label: String,
}

pub struct CheatManager {
    pub entries: Vec<CheatEntry>,
}

impl CheatManager {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, address: u32, value: u8, label: String) {
        self.entries.push(CheatEntry {
            address,
            value,
            enabled: true,
            label,
        });
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
        }
    }

    pub fn apply_to_wram(&self, wram: &mut [u8]) {
        for entry in &self.entries {
            if !entry.enabled {
                continue;
            }
            let addr = entry.address as usize;
            if addr < wram.len() {
                wram[addr] = entry.value;
            }
        }
    }

    pub fn save_to_file(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let json = serde_json::to_string_pretty(&self.entries).map_err(|err| err.to_string())?;
        std::fs::write(path, json).map_err(|err| err.to_string())
    }

    pub fn load_from_file(&mut self, path: &std::path::Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
        let entries: Vec<CheatEntry> =
            serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
        self.entries = entries;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_equal_values() {
        let mut wram = vec![0u8; WORK_RAM_SIZE];
        wram[0x10] = 0x44;
        wram[0x20] = 0x44;
        wram[0x30] = 0x12;

        let mut search = CheatSearch::new();
        search.apply_filter(SearchFilter::Equal(0x44), &wram);

        assert_eq!(search.candidate_count(), 2);
        assert!(search.candidates().contains(&0x10));
        assert!(search.candidates().contains(&0x20));
    }

    #[test]
    fn filters_increased_values_after_snapshot() {
        let mut wram = vec![0u8; WORK_RAM_SIZE];
        wram[0x100] = 10;
        wram[0x101] = 20;

        let mut search = CheatSearch::new();
        search.snapshot(&wram);

        wram[0x100] = 11;
        wram[0x101] = 20;
        search.apply_filter(SearchFilter::Increased, &wram);

        assert_eq!(search.candidates(), &[0x100]);
    }

    #[test]
    fn applies_only_enabled_entries() {
        let mut manager = CheatManager::new();
        manager.add(0x0010, 0xAA, "A".to_string());
        manager.add(0x0011, 0xBB, "B".to_string());
        manager.entries[1].enabled = false;

        let mut wram = vec![0u8; WORK_RAM_SIZE];
        manager.apply_to_wram(&mut wram);

        assert_eq!(wram[0x10], 0xAA);
        assert_eq!(wram[0x11], 0x00);
    }
}
