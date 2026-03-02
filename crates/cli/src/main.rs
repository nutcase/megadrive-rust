use std::error::Error;
use std::io;
use std::path::Path;

use megadrive_core::{Button, Cartridge, ControllerType, Emulator, FRAME_HEIGHT, FRAME_WIDTH};
use sdl2::audio::AudioSpecDesired;
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};
use sdl2::pixels::{Color, PixelFormatEnum};

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
    println!("State hotkey : Ctrl+0..9 save / 0..9 load");
    if boot_frames > 0 {
        fast_forward_boot_frames(&mut emulator, boot_frames);
        println!("Boot skip    : {} frames", boot_frames);
    }

    run_window_loop(
        &header.domestic_title,
        &header.overseas_title,
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
        rom_path.ok_or("usage: megadrive-cli <path-to-rom.bin> [--boot-frames <frames>]")?;

    Ok(CliOptions {
        rom_path,
        boot_frames,
    })
}

fn print_usage() {
    println!("Usage: megadrive-cli <path-to-rom.bin> [--boot-frames <frames>]");
    println!("  --boot-frames <frames>  Fast-forward N video frames before opening the window");
    println!("  Environment fallback    MEGADRIVE_BOOT_FRAMES");
    println!("  Controller env          MEGADRIVE_PAD1=3|6, MEGADRIVE_PAD2=3|6");
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
        "Mega Drive Emulator".to_string()
    } else {
        format!("Mega Drive Emulator - {rom_title}")
    };

    let window = video
        .window(&title, 960, 672)
        .position_centered()
        .resizable()
        .build()
        .map_err(|err| io::Error::other(err.to_string()))?;

    let mut canvas = window
        .into_canvas()
        .accelerated()
        .present_vsync()
        .build()
        .map_err(|err| io::Error::other(err.to_string()))?;

    let texture_creator = canvas.texture_creator();
    let mut frame_texture = texture_creator
        .create_texture_streaming(
            PixelFormatEnum::RGB24,
            FRAME_WIDTH as u32,
            FRAME_HEIGHT as u32,
        )
        .map_err(|err| io::Error::other(err.to_string()))?;

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
    let mut state_slots: [Option<Emulator>; 10] = std::array::from_fn(|_| None);

    'running: loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    repeat: false,
                    ..
                } => {
                    if let Some(slot) = state_slot_from_keycode(key) {
                        if keymod_has_ctrl(keymod) {
                            state_slots[slot] = Some(emulator.clone());
                            println!("Saved state slot {}", slot);
                        } else if let Some(saved) = &state_slots[slot] {
                            *emulator = saved.clone();
                            emulator.set_audio_output_sample_rate_hz(output_sample_rate_hz);
                            audio_queue.clear();
                            println!("Loaded state slot {}", slot);
                        } else {
                            println!("State slot {} is empty", slot);
                        }
                        continue;
                    }
                    if let Some((player, button)) = map_keycode_to_player_button(key) {
                        set_button_state(emulator, player, button, true);
                    }
                }
                Event::KeyUp {
                    keycode: Some(key),
                    repeat: false,
                    ..
                } => {
                    if let Some((player, button)) = map_keycode_to_player_button(key) {
                        set_button_state(emulator, player, button, false);
                    }
                }
                _ => {}
            }
        }

        // Run emulation until one video frame boundary.
        loop {
            if emulator.step().frame_ready {
                break;
            }
        }

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

        frame_texture
            .update(None, emulator.frame_buffer(), FRAME_WIDTH * 3)
            .map_err(|err| io::Error::other(err.to_string()))?;

        canvas.set_draw_color(Color::RGB(0, 0, 0));
        canvas.clear();
        canvas
            .copy(&frame_texture, None, None)
            .map_err(|err| io::Error::other(err.to_string()))?;
        canvas.present();
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
        // Player 1
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
        // Player 2
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

fn keymod_has_ctrl(keymod: Mod) -> bool {
    keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD)
}

#[cfg(test)]
mod tests {
    use super::{
        CliOptions, convert_audio_channels, keymod_has_ctrl, parse_cli_options,
        state_slot_from_keycode,
    };
    use sdl2::keyboard::{Keycode, Mod};

    #[test]
    fn parses_rom_only() {
        let options = parse_cli_options(vec!["roms/sonic.md".to_string()]).expect("valid args");
        assert_eq!(
            options,
            CliOptions {
                rom_path: "roms/sonic.md".to_string(),
                boot_frames: None,
            }
        );
    }

    #[test]
    fn parses_boot_frames_option() {
        let options = parse_cli_options(vec![
            "--boot-frames".to_string(),
            "600".to_string(),
            "roms/sonic.md".to_string(),
        ])
        .expect("valid args");
        assert_eq!(
            options,
            CliOptions {
                rom_path: "roms/sonic.md".to_string(),
                boot_frames: Some(600),
            }
        );
    }

    #[test]
    fn rejects_unknown_option() {
        let err = parse_cli_options(vec![
            "--bad-option".to_string(),
            "roms/sonic.md".to_string(),
        ])
        .expect_err("must fail");
        assert!(err.to_string().contains("unknown option"));
    }

    #[test]
    fn rejects_missing_boot_frames_value() {
        let err = parse_cli_options(vec![
            "roms/sonic.md".to_string(),
            "--boot-frames".to_string(),
        ])
        .expect_err("must fail");
        assert!(err.to_string().contains("--boot-frames requires a value"));
    }

    #[test]
    fn downmixes_stereo_to_mono() {
        let out = convert_audio_channels(&[100, -100, 300, 100], 2, 1);
        assert_eq!(out, vec![0, 200]);
    }

    #[test]
    fn duplicates_mono_to_stereo() {
        let out = convert_audio_channels(&[10, -20, 30], 1, 2);
        assert_eq!(out, vec![10, 10, -20, -20, 30, 30]);
    }

    #[test]
    fn maps_number_keys_to_state_slots() {
        assert_eq!(state_slot_from_keycode(Keycode::Num0), Some(0));
        assert_eq!(state_slot_from_keycode(Keycode::Num5), Some(5));
        assert_eq!(state_slot_from_keycode(Keycode::Num9), Some(9));
        assert_eq!(state_slot_from_keycode(Keycode::Kp3), Some(3));
        assert_eq!(state_slot_from_keycode(Keycode::A), None);
    }

    #[test]
    fn detects_ctrl_modifier() {
        assert!(keymod_has_ctrl(Mod::LCTRLMOD));
        assert!(keymod_has_ctrl(Mod::RCTRLMOD));
        assert!(!keymod_has_ctrl(Mod::NOMOD));
    }
}
