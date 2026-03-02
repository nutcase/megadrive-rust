use egui::text::LayoutJob;
use egui::{self, Color32, FontId};

const BYTES_PER_ROW: usize = 16;
const COLOR_ADDR: Color32 = Color32::from_rgb(0x88, 0x88, 0x88);
const COLOR_NORMAL: Color32 = Color32::from_rgb(0xCC, 0xCC, 0xCC);
const COLOR_CHANGED: Color32 = Color32::from_rgb(0xFF, 0x44, 0x44);
const COLOR_ASCII: Color32 = Color32::from_rgb(0x88, 0xAA, 0x88);

pub struct HexViewerState {
    prev_ram: Vec<u8>,
    goto_addr: String,
    scroll_to_row: Option<usize>,
    edit_addr: String,
    edit_val: String,
}

impl HexViewerState {
    pub fn new() -> Self {
        Self {
            prev_ram: Vec::new(),
            goto_addr: String::new(),
            scroll_to_row: None,
            edit_addr: String::new(),
            edit_val: String::new(),
        }
    }

    pub fn update_prev(&mut self, ram: &[u8]) {
        if self.prev_ram.len() != ram.len() {
            self.prev_ram.resize(ram.len(), 0);
        }
        self.prev_ram.copy_from_slice(ram);
    }

    pub fn show(&mut self, ui: &mut egui::Ui, ram: &[u8], ram_writes: &mut Vec<(usize, u8)>) {
        let total_rows = ram.len().div_ceil(BYTES_PER_ROW);
        let mono = FontId::monospace(12.0);

        ui.horizontal(|ui| {
            ui.label("Go to:");
            let goto_resp =
                ui.add(egui::TextEdit::singleline(&mut self.goto_addr).desired_width(60.0));
            if (goto_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                || ui.button("Go").clicked()
            {
                if let Ok(addr) = parse_hex(&self.goto_addr) {
                    self.scroll_to_row = Some(addr / BYTES_PER_ROW);
                }
            }

            ui.separator();
            ui.label("Edit:");
            ui.add(egui::TextEdit::singleline(&mut self.edit_addr).desired_width(60.0));
            ui.label("=");
            let val_resp =
                ui.add(egui::TextEdit::singleline(&mut self.edit_val).desired_width(32.0));
            if (val_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                || ui.button("Set").clicked()
            {
                if let (Ok(addr), Ok(value)) = (
                    parse_hex(&self.edit_addr),
                    u8::from_str_radix(self.edit_val.trim(), 16),
                ) {
                    if addr < ram.len() {
                        ram_writes.push((addr, value));
                    }
                }
            }
        });
        ui.separator();

        let row_height = 16.0;
        let mut scroll_area = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(ui.available_height());

        if let Some(row) = self.scroll_to_row.take() {
            scroll_area = scroll_area.vertical_scroll_offset(row as f32 * row_height);
        }

        scroll_area.show_rows(ui, row_height, total_rows, |ui, row_range| {
            for row_idx in row_range {
                let base_addr = row_idx * BYTES_PER_ROW;
                let mut job = LayoutJob::default();

                append_text(&mut job, &format!("{:04X}: ", base_addr), &mono, COLOR_ADDR);

                for col in 0..BYTES_PER_ROW {
                    let addr = base_addr + col;
                    if addr >= ram.len() {
                        append_text(&mut job, "   ", &mono, COLOR_NORMAL);
                        continue;
                    }

                    let byte = ram[addr];
                    let changed = addr < self.prev_ram.len() && byte != self.prev_ram[addr];
                    let color = if changed { COLOR_CHANGED } else { COLOR_NORMAL };
                    append_text(&mut job, &format!("{:02X} ", byte), &mono, color);
                }

                append_text(&mut job, " ", &mono, COLOR_ASCII);
                for col in 0..BYTES_PER_ROW {
                    let addr = base_addr + col;
                    if addr >= ram.len() {
                        append_text(&mut job, " ", &mono, COLOR_ASCII);
                        continue;
                    }
                    let byte = ram[addr];
                    let ch = if (0x20..=0x7E).contains(&byte) {
                        byte as char
                    } else {
                        '.'
                    };
                    let mut buf = [0u8; 4];
                    let text = ch.encode_utf8(&mut buf);
                    append_text(&mut job, text, &mono, COLOR_ASCII);
                }

                ui.label(job);
            }
        });
    }
}

fn append_text(job: &mut LayoutJob, text: &str, font: &FontId, color: Color32) {
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font.clone(),
            color,
            ..Default::default()
        },
    );
}

fn parse_hex(input: &str) -> Result<usize, std::num::ParseIntError> {
    let s = input
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .trim_start_matches('$');
    usize::from_str_radix(s, 16)
}
