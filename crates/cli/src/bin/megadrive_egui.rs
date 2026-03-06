#[path = "../egui_ui/mod.rs"]
mod egui_ui;
#[path = "../hud_toast.rs"]
mod hud_toast;

use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

use egui_sdl2_gl::DpiScaling;
use egui_sdl2_gl::ShaderVersion;
use egui_sdl2_gl::gl;
use egui_ui::CheatToolUi;
use egui_ui::gl_game::GlGameRenderer;
use hud_toast::{HudToast, draw_hud_toast_rgb24, show_hud_toast};
use megadrive_core::{Button, Cartridge, ControllerType, Emulator, FRAME_HEIGHT, FRAME_WIDTH};
use sdl2::audio::AudioSpecDesired;
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod, Scancode};

const SCALE: u32 = 3;
const PANEL_WIDTH_DEFAULT: f32 = 420.0;
const PANEL_WIDTH_MIN: f32 = 300.0;

#[derive(Debug, Clone, PartialEq, Eq)]
struct CliOptions {
    rom_path: String,
    boot_frames: Option<u64>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let options = parse_cli_options(std::env::args().skip(1))?;
    let boot_frames = options.boot_frames.or(env_boot_frames()?).unwrap_or(0);

    let rom_path = options.rom_path;
    let rom_bytes = std::fs::read(&rom_path)?;
    let cartridge = Cartridge::from_bytes(rom_bytes)?;
    let mut emulator = Emulator::new(cartridge);
    let header = emulator.header().clone();
    let pad1_type = env_controller_type("MEGADRIVE_PAD1")?.unwrap_or(ControllerType::ThreeButton);
    let pad2_type = env_controller_type("MEGADRIVE_PAD2")?.unwrap_or(ControllerType::ThreeButton);
    emulator.set_controller_type(1, pad1_type);
    emulator.set_controller_type(2, pad2_type);

    println!("Loaded ROM: {}", Path::new(&rom_path).display());
    println!("Console      : {}", header.console_name);
    println!("Domestic     : {}", header.domestic_title);
    println!("Overseas     : {}", header.overseas_title);
    println!("Product code : {}", header.product_code);
    println!("Checksum     : 0x{:04X}", header.checksum);
    println!("Region       : {}", header.region);
    println!("Controller 1 : {}", controller_type_label(pad1_type));
    println!("Controller 2 : {}", controller_type_label(pad2_type));
    println!("Cheat panel  : Tab");
    println!("State hotkey : Ctrl/Cmd+0..9 save(file+session) / 0..9 load(file+session)");
    if boot_frames > 0 {
        fast_forward_boot_frames(&mut emulator, boot_frames);
        println!("Boot skip    : {} frames", boot_frames);
    }

    run_window_loop(
        &header.domestic_title,
        &header.overseas_title,
        &rom_path,
        &mut emulator,
    )?;

    Ok(())
}

fn parse_cli_options<I>(args: I) -> Result<CliOptions, Box<dyn Error>>
where
    I: IntoIterator<Item = String>,
{
    let mut rom_path: Option<String> = None;
    let mut boot_frames: Option<u64> = None;
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            "--boot-frames" => {
                let value = iter.next().ok_or("--boot-frames requires a value")?;
                boot_frames = Some(value.parse::<u64>()?);
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}").into());
            }
            _ => {
                if rom_path.is_some() {
                    return Err("multiple ROM paths provided".into());
                }
                rom_path = Some(arg);
            }
        }
    }

    let rom_path =
        rom_path.ok_or("usage: megadrive-egui <path-to-rom.bin> [--boot-frames <frames>]")?;

    Ok(CliOptions {
        rom_path,
        boot_frames,
    })
}

fn print_usage() {
    println!("Usage: megadrive-egui <path-to-rom.bin> [--boot-frames <frames>]");
    println!("  --boot-frames <frames>  Fast-forward N video frames before opening the window");
    println!("  Environment fallback    MEGADRIVE_BOOT_FRAMES");
    println!("  Controller env          MEGADRIVE_PAD1=3|6, MEGADRIVE_PAD2=3|6");
    println!("  Cheat panel toggle      Tab");
}

fn env_boot_frames() -> Result<Option<u64>, Box<dyn Error>> {
    match std::env::var("MEGADRIVE_BOOT_FRAMES") {
        Ok(value) => Ok(Some(value.parse::<u64>()?)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(err) => Err(Box::new(err)),
    }
}

fn env_controller_type(var_name: &str) -> Result<Option<ControllerType>, Box<dyn Error>> {
    match std::env::var(var_name) {
        Ok(value) => parse_controller_type(&value)
            .map(Some)
            .ok_or_else(|| format!("invalid {var_name} value: {value} (expected 3 or 6)").into()),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(err) => Err(Box::new(err)),
    }
}

fn parse_controller_type(value: &str) -> Option<ControllerType> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "3" | "3b" | "3btn" | "3button" | "three" => Some(ControllerType::ThreeButton),
        "6" | "6b" | "6btn" | "6button" | "six" => Some(ControllerType::SixButton),
        _ => None,
    }
}

fn controller_type_label(controller_type: ControllerType) -> &'static str {
    match controller_type {
        ControllerType::ThreeButton => "3-button",
        ControllerType::SixButton => "6-button",
    }
}

fn fast_forward_boot_frames(emulator: &mut Emulator, frames: u64) {
    let mut advanced = 0u64;
    while advanced < frames {
        if emulator.step().frame_ready {
            advanced += 1;
        }
    }
}

fn run_window_loop(
    domestic_title: &str,
    overseas_title: &str,
    rom_path: &str,
    emulator: &mut Emulator,
) -> Result<(), Box<dyn Error>> {
    let sdl = sdl2::init().map_err(sdl_error)?;
    let video = sdl.video().map_err(|err| {
        io::Error::other(format!(
            "Failed to initialize SDL video subsystem: {err}. Run this command in a GUI session with an active display."
        ))
    })?;
    let displays = video.num_video_displays().map_err(|err| {
        io::Error::other(format!(
            "Failed to query SDL displays: {err}. Run this command in a GUI session with an active display."
        ))
    })?;
    if displays < 1 {
        return Err(io::Error::other(
            "SDL display is unavailable. Run this command in a GUI session with an active display.",
        )
        .into());
    }

    let rom_title = if !domestic_title.is_empty() {
        domestic_title
    } else if !overseas_title.is_empty() {
        overseas_title
    } else {
        ""
    };

    let title = if rom_title.is_empty() {
        "Mega Drive Emulator + Cheats".to_string()
    } else {
        format!("Mega Drive Emulator + Cheats - {rom_title}")
    };

    let game_h = (FRAME_HEIGHT as u32) * SCALE;
    let game_w = game_h * FRAME_WIDTH as u32 / FRAME_HEIGHT as u32;

    let gl_attr = video.gl_attr();
    gl_attr.set_context_profile(sdl2::video::GLProfile::Core);
    gl_attr.set_context_version(3, 2);
    gl_attr.set_double_buffer(true);
    gl_attr.set_multisample_samples(0);

    let mut panel_width_px = PANEL_WIDTH_DEFAULT as u32;
    let mut window = video
        .window(&title, game_w, game_h)
        .position_centered()
        .resizable()
        .opengl()
        .build()
        .map_err(|err| io::Error::other(err.to_string()))?;

    let gl_context = window
        .gl_create_context()
        .map_err(|err| io::Error::other(err.to_string()))?;
    window
        .gl_make_current(&gl_context)
        .map_err(|err| io::Error::other(err.to_string()))?;

    gl::load_with(|name| video.gl_get_proc_address(name) as *const _);
    let vsync_requested = std::env::var("MEGADRIVE_GL_VSYNC")
        .ok()
        .map(|v| !matches!(v.trim(), "0" | "false" | "off" | "no"))
        .unwrap_or(true);
    if vsync_requested {
        if let Err(vsync_err) = video.gl_set_swap_interval(sdl2::video::SwapInterval::VSync) {
            eprintln!(
                "warning: failed to enable VSync ({}), using immediate swap",
                vsync_err
            );
            let _ = video.gl_set_swap_interval(sdl2::video::SwapInterval::Immediate);
        }
        eprintln!("swap interval : {:?}", video.gl_get_swap_interval());
    } else {
        let _ = video.gl_set_swap_interval(sdl2::video::SwapInterval::Immediate);
        eprintln!("swap interval : {:?}", video.gl_get_swap_interval());
    }

    let (mut painter, mut egui_state) =
        egui_sdl2_gl::with_sdl2(&window, ShaderVersion::Default, DpiScaling::Default);
    let egui_ctx = egui::Context::default();

    let mut events = sdl.event_pump().map_err(sdl_error)?;
    let audio = sdl.audio().map_err(|err| {
        io::Error::other(format!(
            "Failed to initialize SDL audio subsystem: {err}. Ensure an audio output device is available."
        ))
    })?;
    let audio_spec = AudioSpecDesired {
        freq: Some(44_100),
        channels: Some(2),
        samples: Some(1_024),
    };
    let audio_queue = audio
        .open_queue::<i16, _>(None, &audio_spec)
        .map_err(|err| io::Error::other(err.to_string()))?;
    let obtained_audio_spec = audio_queue.spec();
    let output_sample_rate_hz = obtained_audio_spec.freq.max(8_000) as u32;
    emulator.set_audio_output_sample_rate_hz(output_sample_rate_hz);
    let emulator_channels = emulator.audio_output_channels().max(1);
    let output_channels = obtained_audio_spec.channels.max(1);
    println!(
        "Audio output : {} Hz, {} ch",
        obtained_audio_spec.freq, obtained_audio_spec.channels
    );
    let audio_queue_target_frames = ((output_sample_rate_hz as usize) / 10).clamp(2_048, 8_192);
    let audio_queue_feed_frames = (audio_queue_target_frames / 2).max(1_024);
    audio_queue.resume();

    let mut game_renderer = GlGameRenderer::new();
    let cheat_path = cheat_file_path(rom_path);
    let state_dir = state_dir_path();
    println!("Cheat file    : {}", cheat_path.display());
    println!("State dir     : {}", state_dir.display());
    let mut cheat_ui = CheatToolUi::new();
    let mut prev_panel_visible = false;
    let mut state_slots: [Option<Emulator>; 10] = std::array::from_fn(|_| None);
    let mut hud_toast: Option<HudToast> = None;

    let text_input = video.text_input();
    let mut text_input_active = false;
    text_input.stop();

    'running: loop {
        let should_enable_text_input = cheat_ui.panel_visible;
        if should_enable_text_input != text_input_active {
            if should_enable_text_input {
                text_input.start();
            } else {
                text_input.stop();
            }
            text_input_active = should_enable_text_input;
        }

        egui_state.input.time = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        );

        let egui_wants_keyboard = cheat_ui.panel_visible && egui_ctx.wants_keyboard_input();

        for event in events.poll_iter() {
            if cheat_ui.panel_visible {
                if let Some(filtered) = filter_event_for_ascii_text_input(&event) {
                    egui_state.process_input(&window, filtered, &mut painter);
                }
            }

            match event {
                Event::Quit { .. } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::Tab),
                    repeat: false,
                    ..
                } => {
                    cheat_ui.panel_visible = !cheat_ui.panel_visible;
                }
                Event::KeyDown {
                    keycode,
                    scancode,
                    keymod,
                    repeat: false,
                    ..
                } => {
                    if let Some(slot) = state_slot_from_input(keycode, scancode) {
                        let state_path = state_slot_path(rom_path, slot);
                        if keymod_has_state_save_modifier(keymod) {
                            state_slots[slot] = Some(emulator.clone());
                            match emulator.save_state_to_file(&state_path) {
                                Ok(()) => {
                                    println!(
                                        "Saved state slot {} -> {}",
                                        slot,
                                        state_path.display()
                                    );
                                    show_hud_toast(&mut hud_toast, format!("SAVE {slot} OK"));
                                }
                                Err(err) => {
                                    eprintln!(
                                        "failed to save state slot {} to {}: {}",
                                        slot,
                                        state_path.display(),
                                        err
                                    );
                                    show_hud_toast(&mut hud_toast, format!("SAVE {slot} ERR"));
                                }
                            }
                            continue;
                        }
                        if !egui_wants_keyboard {
                            if let Some(saved) = &state_slots[slot] {
                                *emulator = saved.clone();
                                emulator.set_audio_output_sample_rate_hz(output_sample_rate_hz);
                                audio_queue.clear();
                                println!("Loaded state slot {} (session)", slot);
                                show_hud_toast(&mut hud_toast, format!("LOAD {slot} OK"));
                            } else {
                                if state_path.exists() {
                                    match emulator.load_state_from_file(&state_path) {
                                        Ok(()) => {
                                            state_slots[slot] = Some(emulator.clone());
                                            emulator.set_audio_output_sample_rate_hz(
                                                output_sample_rate_hz,
                                            );
                                            audio_queue.clear();
                                            println!(
                                                "Loaded state slot {} <- {}",
                                                slot,
                                                state_path.display()
                                            );
                                            show_hud_toast(
                                                &mut hud_toast,
                                                format!("LOAD {slot} OK"),
                                            );
                                        }
                                        Err(err) => {
                                            eprintln!(
                                                "failed to load state slot {} from {}: {}",
                                                slot,
                                                state_path.display(),
                                                err
                                            );
                                            show_hud_toast(
                                                &mut hud_toast,
                                                format!("LOAD {slot} ERR"),
                                            );
                                        }
                                    }
                                } else {
                                    println!("State slot {} is empty", slot);
                                    show_hud_toast(&mut hud_toast, format!("SLOT {slot} EMPTY"));
                                }
                            }
                            continue;
                        } else {
                            println!(
                                "State load hotkey blocked while UI text input is focused (slot {})",
                                slot
                            );
                        }
                    }
                    if !egui_wants_keyboard {
                        if let Some(key) = keycode {
                            if let Some((player, button)) = map_keycode_to_player_button(key) {
                                set_button_state(emulator, player, button, true);
                            }
                        }
                    }
                }
                Event::KeyUp {
                    keycode,
                    repeat: false,
                    ..
                } => {
                    if !egui_wants_keyboard {
                        if let Some(key) = keycode {
                            if let Some((player, button)) = map_keycode_to_player_button(key) {
                                set_button_state(emulator, player, button, false);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if cheat_ui.panel_visible != prev_panel_visible {
            if cheat_ui.panel_visible {
                cheat_ui.refresh(emulator.work_ram());
                let _ = window.set_size(game_w + panel_width_px, game_h);
            } else {
                let _ = window.set_size(game_w, game_h);
            }
            prev_panel_visible = cheat_ui.panel_visible;
        }

        cheat_ui
            .cheat_search_ui
            .manager
            .apply_to_wram(emulator.work_ram_mut());

        if !cheat_ui.paused {
            loop {
                if emulator.step().frame_ready {
                    break;
                }
            }
        }

        cheat_ui
            .cheat_search_ui
            .manager
            .apply_to_wram(emulator.work_ram_mut());

        let queued_i16 = (audio_queue.size() as usize) / std::mem::size_of::<i16>();
        let queued_frames = queued_i16 / output_channels as usize;
        if queued_frames < audio_queue_target_frames && emulator.pending_audio_samples() > 0 {
            let samples =
                emulator.drain_audio_samples(audio_queue_feed_frames * emulator_channels as usize);
            if !samples.is_empty() {
                let queued = convert_audio_channels(&samples, emulator_channels, output_channels);
                audio_queue
                    .queue_audio(&queued)
                    .map_err(|err| io::Error::other(err.to_string()))?;
            }
        }

        if hud_toast.is_some() {
            let mut frame_buf = emulator.frame_buffer().to_vec();
            draw_hud_toast_rgb24(&mut frame_buf, FRAME_WIDTH, FRAME_HEIGHT, &mut hud_toast);
            game_renderer.upload_frame_rgb24(&frame_buf, FRAME_WIDTH, FRAME_HEIGHT);
        } else {
            game_renderer.upload_frame_rgb24(emulator.frame_buffer(), FRAME_WIDTH, FRAME_HEIGHT);
        }

        let (win_w, _) = window.size();
        let (drawable_w, drawable_h) = window.drawable_size();
        unsafe {
            gl::ClearColor(0.0, 0.0, 0.0, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }

        let panel_px_logical = if cheat_ui.panel_visible {
            panel_width_px
        } else {
            0
        };
        let panel_px = if win_w > 0 {
            (panel_px_logical as u64)
                .saturating_mul(drawable_w as u64)
                .saturating_div(win_w as u64) as u32
        } else {
            0
        };
        let game_vp_w = drawable_w.saturating_sub(panel_px);
        game_renderer.draw(0, 0, game_vp_w as i32, drawable_h as i32);

        if cheat_ui.panel_visible {
            painter.update_screen_rect((drawable_w, drawable_h));
            egui_state.input.screen_rect = Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(drawable_w as f32, drawable_h as f32),
            ));

            let mut ram_writes: Vec<(usize, u8)> = Vec::new();
            let wram = emulator.work_ram();
            let mut requested_panel_width = panel_width_px;
            let full_output = egui_ctx.run(egui_state.input.take(), |ctx| {
                let panel_resp = egui::SidePanel::right("md_cheat_panel")
                    .resizable(true)
                    .min_width(PANEL_WIDTH_MIN)
                    .default_width(PANEL_WIDTH_DEFAULT)
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                cheat_ui.show_panel(
                                    ui,
                                    &mut ram_writes,
                                    wram,
                                    Some(cheat_path.as_path()),
                                );
                            });
                    });

                requested_panel_width =
                    panel_resp.response.rect.width().max(PANEL_WIDTH_MIN) as u32;
            });

            if requested_panel_width != panel_width_px {
                panel_width_px = requested_panel_width;
                let _ = window.set_size(game_w + panel_width_px, game_h);
            }

            if cheat_ui.refresh_requested {
                cheat_ui.refresh(emulator.work_ram());
                cheat_ui.refresh_requested = false;
            }

            for (addr, value) in ram_writes {
                let wram_mut = emulator.work_ram_mut();
                if addr < wram_mut.len() {
                    wram_mut[addr] = value;
                }
            }

            let primitives = egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
            painter.paint_jobs(None, full_output.textures_delta, primitives);
            egui_state.process_output(&window, &full_output.platform_output);
        }

        window.gl_swap_window();
    }

    Ok(())
}

fn sdl_error(message: String) -> io::Error {
    io::Error::other(message)
}

fn convert_audio_channels(samples: &[i16], input_channels: u8, output_channels: u8) -> Vec<i16> {
    if input_channels == output_channels {
        return samples.to_vec();
    }

    match (input_channels, output_channels) {
        (2, 1) => samples
            .chunks_exact(2)
            .map(|pair| ((pair[0] as i32 + pair[1] as i32) / 2) as i16)
            .collect(),
        (1, 2) => {
            let mut out = Vec::with_capacity(samples.len() * 2);
            for &sample in samples {
                out.push(sample);
                out.push(sample);
            }
            out
        }
        _ => samples.to_vec(),
    }
}

fn set_button_state(emulator: &mut Emulator, player: u8, button: Button, pressed: bool) {
    match player {
        1 => emulator.set_button_pressed(button, pressed),
        2 => emulator.set_button2_pressed(button, pressed),
        _ => {}
    }
}

fn map_keycode_to_player_button(key: Keycode) -> Option<(u8, Button)> {
    match key {
        Keycode::Up => Some((1, Button::Up)),
        Keycode::Down => Some((1, Button::Down)),
        Keycode::Left => Some((1, Button::Left)),
        Keycode::Right => Some((1, Button::Right)),
        Keycode::A => Some((1, Button::A)),
        Keycode::Z => Some((1, Button::B)),
        Keycode::X => Some((1, Button::C)),
        Keycode::S => Some((1, Button::X)),
        Keycode::D => Some((1, Button::Y)),
        Keycode::F => Some((1, Button::Z)),
        Keycode::Q => Some((1, Button::Mode)),
        Keycode::Return => Some((1, Button::Start)),
        Keycode::I => Some((2, Button::Up)),
        Keycode::K => Some((2, Button::Down)),
        Keycode::J => Some((2, Button::Left)),
        Keycode::L => Some((2, Button::Right)),
        Keycode::R => Some((2, Button::A)),
        Keycode::T => Some((2, Button::B)),
        Keycode::Y => Some((2, Button::C)),
        Keycode::U => Some((2, Button::X)),
        Keycode::O => Some((2, Button::Y)),
        Keycode::P => Some((2, Button::Z)),
        Keycode::Slash => Some((2, Button::Mode)),
        Keycode::RShift => Some((2, Button::Start)),
        _ => None,
    }
}

fn state_slot_from_input(keycode: Option<Keycode>, scancode: Option<Scancode>) -> Option<usize> {
    keycode
        .and_then(state_slot_from_keycode)
        .or_else(|| scancode.and_then(state_slot_from_scancode))
}

fn state_slot_from_keycode(key: Keycode) -> Option<usize> {
    match key {
        Keycode::Num0 | Keycode::Kp0 => Some(0),
        Keycode::Num1 | Keycode::Kp1 => Some(1),
        Keycode::Num2 | Keycode::Kp2 => Some(2),
        Keycode::Num3 | Keycode::Kp3 => Some(3),
        Keycode::Num4 | Keycode::Kp4 => Some(4),
        Keycode::Num5 | Keycode::Kp5 => Some(5),
        Keycode::Num6 | Keycode::Kp6 => Some(6),
        Keycode::Num7 | Keycode::Kp7 => Some(7),
        Keycode::Num8 | Keycode::Kp8 => Some(8),
        Keycode::Num9 | Keycode::Kp9 => Some(9),
        _ => None,
    }
}

fn state_slot_from_scancode(key: Scancode) -> Option<usize> {
    match key {
        Scancode::Num0 | Scancode::Kp0 => Some(0),
        Scancode::Num1 | Scancode::Kp1 => Some(1),
        Scancode::Num2 | Scancode::Kp2 => Some(2),
        Scancode::Num3 | Scancode::Kp3 => Some(3),
        Scancode::Num4 | Scancode::Kp4 => Some(4),
        Scancode::Num5 | Scancode::Kp5 => Some(5),
        Scancode::Num6 | Scancode::Kp6 => Some(6),
        Scancode::Num7 | Scancode::Kp7 => Some(7),
        Scancode::Num8 | Scancode::Kp8 => Some(8),
        Scancode::Num9 | Scancode::Kp9 => Some(9),
        _ => None,
    }
}

fn keymod_has_state_save_modifier(keymod: Mod) -> bool {
    keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD | Mod::LGUIMOD | Mod::RGUIMOD)
}

fn state_slot_path(rom_path: &str, slot: usize) -> PathBuf {
    let stem = rom_stem(rom_path);
    state_dir_path().join(format!("{stem}.slot{slot}.mdst"))
}

fn state_dir_path() -> PathBuf {
    std::env::var_os("MEGADRIVE_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(default_state_dir)
}

fn default_state_dir() -> PathBuf {
    let candidate = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workspace_root = candidate.canonicalize().unwrap_or(candidate);
    workspace_root.join("states")
}

fn cheat_file_path(rom_path: &str) -> PathBuf {
    let stem = rom_stem(rom_path);

    let cheat_dir = std::env::var_os("MEGADRIVE_CHEAT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(default_cheat_dir);

    cheat_dir.join(format!("{stem}.json"))
}

fn rom_stem(rom_path: &str) -> String {
    Path::new(rom_path)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("game")
        .to_string()
}

fn default_cheat_dir() -> PathBuf {
    // Use workspace-root `cheats/` by default so save/load location is stable
    // regardless of the process current working directory.
    let candidate = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workspace_root = candidate.canonicalize().unwrap_or(candidate);
    workspace_root.join("cheats")
}

fn filter_event_for_ascii_text_input(event: &Event) -> Option<Event> {
    match event {
        Event::TextEditing { .. } => None,
        Event::TextInput {
            timestamp,
            window_id,
            text,
        } => {
            let ascii_text: String = text.chars().filter(|ch| ch.is_ascii()).collect();
            if ascii_text.is_empty() {
                None
            } else {
                Some(Event::TextInput {
                    timestamp: *timestamp,
                    window_id: *window_id,
                    text: ascii_text,
                })
            }
        }
        _ => Some(event.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::{keymod_has_state_save_modifier, state_slot_from_input, state_slot_path};
    use sdl2::keyboard::{Keycode, Mod, Scancode};

    #[test]
    fn state_slot_detects_keycode_and_scancode() {
        assert_eq!(state_slot_from_input(Some(Keycode::Num3), None), Some(3));
        assert_eq!(state_slot_from_input(None, Some(Scancode::Num7)), Some(7));
        assert_eq!(state_slot_from_input(Some(Keycode::Kp9), None), Some(9));
    }

    #[test]
    fn state_save_modifier_accepts_ctrl_and_cmd() {
        assert!(keymod_has_state_save_modifier(Mod::LCTRLMOD));
        assert!(keymod_has_state_save_modifier(Mod::RGUIMOD));
        assert!(!keymod_has_state_save_modifier(Mod::NOMOD));
    }

    #[test]
    fn state_slot_path_uses_slot_suffix() {
        let path = state_slot_path("roms/Sonic The Hedgehog 3.md", 4);
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        assert!(name.ends_with(".slot4.mdst"));
        assert!(name.starts_with("Sonic The Hedgehog 3"));
    }
}
