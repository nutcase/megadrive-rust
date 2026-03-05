use egui::{self, Color32, RichText};

use super::cheat_model::{CheatManager, CheatSearch, SearchFilter};

#[derive(Clone, Copy, PartialEq, Eq)]
enum FilterKind {
    Equal,
    NotEqual,
    GreaterThan,
    LessThan,
    Increased,
    Decreased,
    Changed,
    Unchanged,
    IncreasedBy,
    DecreasedBy,
}

impl FilterKind {
    const ALL: [FilterKind; 10] = [
        FilterKind::Equal,
        FilterKind::NotEqual,
        FilterKind::GreaterThan,
        FilterKind::LessThan,
        FilterKind::Increased,
        FilterKind::Decreased,
        FilterKind::Changed,
        FilterKind::Unchanged,
        FilterKind::IncreasedBy,
        FilterKind::DecreasedBy,
    ];

    fn label(self) -> &'static str {
        match self {
            FilterKind::Equal => "Equal to",
            FilterKind::NotEqual => "Not equal to",
            FilterKind::GreaterThan => "Greater than",
            FilterKind::LessThan => "Less than",
            FilterKind::Increased => "Increased",
            FilterKind::Decreased => "Decreased",
            FilterKind::Changed => "Changed",
            FilterKind::Unchanged => "Unchanged",
            FilterKind::IncreasedBy => "Increased by",
            FilterKind::DecreasedBy => "Decreased by",
        }
    }

    fn needs_value(self) -> bool {
        matches!(
            self,
            FilterKind::Equal
                | FilterKind::NotEqual
                | FilterKind::GreaterThan
                | FilterKind::LessThan
                | FilterKind::IncreasedBy
                | FilterKind::DecreasedBy
        )
    }
}

fn parse_u8_value(input: &str) -> Option<u8> {
    let s = input.trim();
    s.parse::<u8>().ok().or_else(|| {
        let hex = s.trim_start_matches("0x").trim_start_matches("0X");
        u8::from_str_radix(hex, 16).ok()
    })
}

fn format_addr(addr: u32) -> String {
    format!("W:{addr:04X}")
}

fn parse_cheat_addr(input: &str, wram_size: usize) -> Option<u32> {
    let mut s = input.trim();

    if let Some(rest) = s.strip_prefix("W:").or_else(|| s.strip_prefix("w:")) {
        s = rest;
    }

    s = s.trim_start_matches('$');
    s = s.trim_start_matches("0x").trim_start_matches("0X");

    let raw = u32::from_str_radix(s, 16).ok()?;

    if (0xFF0000..=0xFFFFFF).contains(&raw) {
        let offset = (raw - 0xFF0000) as usize;
        return (offset < wram_size).then_some(offset as u32);
    }

    ((raw as usize) < wram_size).then_some(raw)
}

pub struct CheatSearchUi {
    pub search: CheatSearch,
    pub manager: CheatManager,
    filter_kind: FilterKind,
    filter_value: String,
    new_cheat_addr: String,
    new_cheat_value: String,
    cheats_loaded: bool,
}

impl CheatSearchUi {
    pub fn new() -> Self {
        Self {
            search: CheatSearch::new(),
            manager: CheatManager::new(),
            filter_kind: FilterKind::Equal,
            filter_value: String::new(),
            new_cheat_addr: String::new(),
            new_cheat_value: String::new(),
            cheats_loaded: false,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, wram: &[u8], cheat_path: Option<&std::path::Path>) {
        if !self.cheats_loaded {
            self.cheats_loaded = true;
            if let Some(path) = cheat_path {
                if path.exists() {
                    if let Err(err) = self.manager.load_from_file(path) {
                        eprintln!("failed to load cheats from {}: {err}", path.display());
                    }
                }
            }
        }

        ui.horizontal(|ui| {
            ui.heading("Cheat Search");
            ui.separator();
            ui.label(RichText::new(format!("WRAM:{}KB", wram.len() / 1024)).small());
        });
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("Snapshot").clicked() {
                self.search.snapshot(wram);
            }
            if ui.button("Reset").clicked() {
                self.search.reset();
            }
            ui.label(format!("Candidates: {}", self.search.candidate_count()));
            if self.search.has_snapshot() {
                ui.label(
                    RichText::new("(snapshot taken)").color(Color32::from_rgb(0x44, 0xCC, 0x44)),
                );
            }
        });

        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Filter:");
            egui::ComboBox::from_id_salt("md_cheat_filter_kind")
                .selected_text(self.filter_kind.label())
                .width(130.0)
                .show_ui(ui, |ui| {
                    for kind in FilterKind::ALL {
                        ui.selectable_value(&mut self.filter_kind, kind, kind.label());
                    }
                });

            if self.filter_kind.needs_value() {
                ui.label("Value:");
                ui.add(egui::TextEdit::singleline(&mut self.filter_value).desired_width(60.0));
            }

            if ui.button("Apply").clicked() {
                if let Some(filter) = self.build_filter() {
                    self.search.apply_filter(filter, wram);
                }
            }
        });

        ui.separator();

        ui.label(format!("Results: {}", self.search.candidate_count()));
        ui.horizontal(|ui| {
            ui.style_mut().override_font_id = Some(egui::FontId::monospace(12.0));
            ui.label("Addr");
            ui.label("Prev");
            ui.label("Cur");
            ui.label("");
        });

        let candidates = self.search.candidates();
        let snapshot = self.search.previous_snapshot();
        let row_height = (ui.text_style_height(&egui::TextStyle::Monospace) + 4.0).max(16.0);

        egui::ScrollArea::vertical()
            .id_salt("md_cheat_results")
            .max_height(180.0)
            .show_rows(ui, row_height, candidates.len(), |ui, row_range| {
                ui.style_mut().override_font_id = Some(egui::FontId::monospace(12.0));
                for row_idx in row_range {
                    let Some(&addr) = candidates.get(row_idx) else {
                        continue;
                    };
                    let current = wram.get(addr as usize).copied().unwrap_or(0);
                    let previous = snapshot.map(|snap| snap.get(addr)).unwrap_or(0);

                    ui.horizontal(|ui| {
                        ui.label(format_addr(addr));
                        ui.label(format!("{:02X}", previous));
                        ui.label(format!("{:02X}", current));
                        if ui.small_button("Add").clicked() {
                            self.manager.add(addr, current, format_addr(addr));
                        }
                    });
                }
            });

        ui.separator();

        ui.horizontal(|ui| {
            ui.heading("Active Cheats");
            ui.separator();
            if let Some(path) = cheat_path {
                if ui.button("Save").clicked() {
                    if let Err(err) = self.manager.save_to_file(path) {
                        eprintln!("failed to save cheats to {}: {err}", path.display());
                    } else {
                        eprintln!("saved cheats to {}", path.display());
                    }
                }
                if path.exists() && ui.button("Load").clicked() {
                    if let Err(err) = self.manager.load_from_file(path) {
                        eprintln!("failed to load cheats from {}: {err}", path.display());
                    } else {
                        eprintln!("loaded cheats from {}", path.display());
                    }
                }
            }
        });

        let mut remove_index = None;
        egui::ScrollArea::vertical()
            .id_salt("md_cheat_entries")
            .max_height(140.0)
            .show(ui, |ui| {
                ui.style_mut().override_font_id = Some(egui::FontId::monospace(12.0));
                for (index, entry) in self.manager.entries.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut entry.enabled, "");
                        ui.label(format_addr(entry.address));
                        ui.label("=");
                        let mut value = format!("{:02X}", entry.value);
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut value)
                                .desired_width(32.0)
                                .clip_text(false),
                        );
                        if response.changed() {
                            if let Some(parsed) = parse_u8_value(&value) {
                                entry.value = parsed;
                            }
                        }
                        ui.text_edit_singleline(&mut entry.label);
                        if ui.small_button("X").clicked() {
                            remove_index = Some(index);
                        }
                    });
                }
            });

        if let Some(index) = remove_index {
            self.manager.remove(index);
        }

        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Add:");
            ui.add(
                egui::TextEdit::singleline(&mut self.new_cheat_addr)
                    .desired_width(80.0)
                    .hint_text("FF1234"),
            );
            ui.label("=");
            ui.add(
                egui::TextEdit::singleline(&mut self.new_cheat_value)
                    .desired_width(40.0)
                    .hint_text("xx"),
            );

            if ui.button("Add").clicked() {
                if let (Some(addr), Some(value)) = (
                    parse_cheat_addr(&self.new_cheat_addr, wram.len()),
                    parse_u8_value(&self.new_cheat_value),
                ) {
                    self.manager.add(addr, value, format_addr(addr));
                    self.new_cheat_addr.clear();
                    self.new_cheat_value.clear();
                }
            }
        });
    }

    fn build_filter(&self) -> Option<SearchFilter> {
        match self.filter_kind {
            FilterKind::Equal => parse_u8_value(&self.filter_value).map(SearchFilter::Equal),
            FilterKind::NotEqual => parse_u8_value(&self.filter_value).map(SearchFilter::NotEqual),
            FilterKind::GreaterThan => {
                parse_u8_value(&self.filter_value).map(SearchFilter::GreaterThan)
            }
            FilterKind::LessThan => parse_u8_value(&self.filter_value).map(SearchFilter::LessThan),
            FilterKind::Increased => Some(SearchFilter::Increased),
            FilterKind::Decreased => Some(SearchFilter::Decreased),
            FilterKind::Changed => Some(SearchFilter::Changed),
            FilterKind::Unchanged => Some(SearchFilter::Unchanged),
            FilterKind::IncreasedBy => {
                parse_u8_value(&self.filter_value).map(SearchFilter::IncreasedBy)
            }
            FilterKind::DecreasedBy => {
                parse_u8_value(&self.filter_value).map(SearchFilter::DecreasedBy)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cheat_addr;

    #[test]
    fn parses_absolute_wram_address() {
        assert_eq!(parse_cheat_addr("FF1234", 0x10000), Some(0x1234));
        assert_eq!(parse_cheat_addr("0xFF0000", 0x10000), Some(0x0000));
    }

    #[test]
    fn parses_offset_address() {
        assert_eq!(parse_cheat_addr("W:00A0", 0x10000), Some(0x00A0));
        assert_eq!(parse_cheat_addr("00A0", 0x10000), Some(0x00A0));
    }

    #[test]
    fn rejects_out_of_range_address() {
        assert_eq!(parse_cheat_addr("FFFFFF", 0x1000), None);
        assert_eq!(parse_cheat_addr("1000", 0x1000), None);
    }
}
