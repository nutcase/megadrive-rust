pub mod cheat_model;
pub mod cheat_search;
pub mod gl_game;
pub mod hex_viewer;

use cheat_search::CheatSearchUi;
use hex_viewer::HexViewerState;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    HexViewer,
    CheatSearch,
}

pub struct CheatToolUi {
    pub active_tab: ActiveTab,
    pub hex_viewer: HexViewerState,
    pub cheat_search_ui: CheatSearchUi,
    pub panel_visible: bool,
    pub refresh_requested: bool,
    pub paused: bool,
    pub auto_refresh: bool,
    pub wram_snapshot: Vec<u8>,
}

impl CheatToolUi {
    pub fn new() -> Self {
        Self {
            active_tab: ActiveTab::HexViewer,
            hex_viewer: HexViewerState::new(),
            cheat_search_ui: CheatSearchUi::new(),
            panel_visible: false,
            refresh_requested: false,
            paused: false,
            auto_refresh: true,
            wram_snapshot: vec![0u8; cheat_model::WORK_RAM_SIZE],
        }
    }

    pub fn refresh(&mut self, wram: &[u8]) {
        let previous = self.wram_snapshot.clone();
        if self.wram_snapshot.len() != wram.len() {
            self.wram_snapshot.resize(wram.len(), 0);
        }
        self.wram_snapshot.copy_from_slice(wram);
        self.hex_viewer.update_prev(&previous);
    }

    pub fn show_panel(
        &mut self,
        ui: &mut egui::Ui,
        ram_writes: &mut Vec<(usize, u8)>,
        wram: &[u8],
        cheat_path: Option<&std::path::Path>,
    ) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.active_tab, ActiveTab::HexViewer, "Hex Viewer");
            ui.selectable_value(&mut self.active_tab, ActiveTab::CheatSearch, "Cheat Search");
            ui.separator();
            ui.checkbox(&mut self.paused, "Pause");
        });
        ui.separator();

        match self.active_tab {
            ActiveTab::HexViewer => {
                ui.horizontal(|ui| {
                    if ui.button("Refresh").clicked() {
                        self.refresh_requested = true;
                    }
                    ui.checkbox(&mut self.auto_refresh, "Auto");
                });
                ui.separator();
                if self.auto_refresh {
                    self.refresh_requested = true;
                }
                self.hex_viewer.show(ui, &self.wram_snapshot, ram_writes);
            }
            ActiveTab::CheatSearch => {
                self.cheat_search_ui.show(ui, wram, cheat_path);
            }
        }
    }
}
