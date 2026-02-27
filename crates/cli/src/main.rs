use std::error::Error;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use megadrive_core::{Button, Cartridge, Emulator, FRAME_HEIGHT, FRAME_WIDTH};
use sdl2::audio::AudioSpecDesired;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
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

    println!("Loaded ROM: {}", Path::new(&rom_path).display());
    println!("Console      : {}", header.console_name);
    println!("Domestic     : {}", header.domestic_title);
    println!("Overseas     : {}", header.overseas_title);
    println!("Product code : {}", header.product_code);
    println!("Checksum     : 0x{:04X}", header.checksum);
    println!("Region       : {}", header.region);
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
}

fn env_boot_frames() -> Result<Option<u64>, Box<dyn Error>> {
    match std::env::var("MEGADRIVE_BOOT_FRAMES") {
        Ok(value) => Ok(Some(value.parse::<u64>()?)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(err) => Err(Box::new(err)),
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
    const AUDIO_QUEUE_TARGET_SAMPLES: usize = 4_096;
    const AUDIO_QUEUE_FEED_SAMPLES: usize = 2_048;

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
        channels: Some(1),
        samples: Some(1_024),
    };
    let audio_queue = audio
        .open_queue::<i16, _>(None, &audio_spec)
        .map_err(|err| io::Error::other(err.to_string()))?;
    audio_queue.resume();

    let frame_budget = Duration::from_nanos(16_666_667);

    'running: loop {
        let frame_start = Instant::now();

        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                Event::KeyDown {
                    keycode: Some(key),
                    repeat: false,
                    ..
                } => {
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

        let queued_samples = (audio_queue.size() as usize) / std::mem::size_of::<i16>();
        if queued_samples < AUDIO_QUEUE_TARGET_SAMPLES && emulator.pending_audio_samples() > 0 {
            let samples = emulator.drain_audio_samples(AUDIO_QUEUE_FEED_SAMPLES);
            if !samples.is_empty() {
                audio_queue
                    .queue_audio(&samples)
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

        let elapsed = frame_start.elapsed();
        if elapsed < frame_budget {
            std::thread::sleep(frame_budget - elapsed);
        }
    }

    Ok(())
}

fn sdl_error(message: String) -> io::Error {
    io::Error::other(message)
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
        Keycode::Return => Some((1, Button::Start)),
        // Player 2
        Keycode::I => Some((2, Button::Up)),
        Keycode::K => Some((2, Button::Down)),
        Keycode::J => Some((2, Button::Left)),
        Keycode::L => Some((2, Button::Right)),
        Keycode::R => Some((2, Button::A)),
        Keycode::T => Some((2, Button::B)),
        Keycode::Y => Some((2, Button::C)),
        Keycode::RShift => Some((2, Button::Start)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{CliOptions, parse_cli_options};

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
}
