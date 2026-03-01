use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::error::Error;

use megadrive_core::ControllerType;
use megadrive_core::cartridge::Cartridge;
use megadrive_core::cpu::M68k;
use megadrive_core::input::Button;
use megadrive_core::memory::MemoryMap;

#[derive(Debug, Clone, Copy)]
struct InputEvent {
    frame: u64,
    player: u8,
    button: Button,
    pressed: bool,
}

#[derive(Debug, Clone)]
struct CliArgs {
    rom_path: String,
    steps: usize,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let cli = parse_cli_args(env::args().skip(1))?;
    let rom_path = cli.rom_path;
    let steps = cli.steps;
    let stop_on_unknown = env::var("STOP_ON_UNKNOWN").is_ok();
    let stop_on_bad_pc = env::var("STOP_ON_BAD_PC").is_ok();
    let stop_on_pc = env::var("STOP_ON_PC")
        .ok()
        .and_then(|value| parse_u32_value(&value));
    let watch_pc = env::var("WATCH_PC")
        .ok()
        .and_then(|value| parse_u32_value(&value));
    let watch_addrs = env::var("WATCH_ADDRS")
        .ok()
        .map(|value| parse_addr_list(&value))
        .unwrap_or_default();
    let watch_trace = env::var("WATCH_TRACE").is_ok();
    let trace_vdp_r1 = env::var("TRACE_VDP_R1").is_ok();
    let hold_start = env::var("HOLD_START").is_ok();
    let hold_a = env::var("HOLD_A").is_ok();
    let force_b094 = env::var("FORCE_B094").is_ok();
    let force_b094_sticky = env::var("FORCE_B094_STICKY").is_ok();
    let dump_line_state = env::var("DUMP_LINE_STATE").is_ok();
    let dump_plane_state = env::var("DUMP_PLANE_STATE").is_ok();
    let dump_sprite_state = env::var("DUMP_SPRITE_STATE").is_ok();
    let stop_frame = env::var("STOP_FRAME")
        .ok()
        .and_then(|v| v.parse::<u64>().ok());
    let dma_trace_limit = env::var("DMA_TRACE_LIMIT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(8)
        .clamp(1, 128);
    let dump_frame_path = env::var("DUMP_FRAME").ok();
    let dump_frames: BTreeSet<u64> = env::var("DUMP_FRAMES")
        .ok()
        .map(|value| parse_frame_list(&value))
        .unwrap_or_default();
    let dump_frame_prefix = env::var("DUMP_FRAME_PREFIX").ok();
    let mut dumped_frames = BTreeSet::new();
    let mut input_events = env::var("INPUT_SCRIPT")
        .ok()
        .map(|value| parse_input_script(&value))
        .unwrap_or_default();
    input_events.sort_by_key(|event| event.frame);
    let mut next_input_event = 0usize;

    let rom = std::fs::read(&rom_path)?;
    let cart = Cartridge::from_bytes(rom)?;
    let mut memory = MemoryMap::new(cart);
    if let Some(controller_type) = env::var("MEGADRIVE_PAD1")
        .ok()
        .and_then(|value| parse_controller_type(&value))
    {
        memory.set_controller_type(1, controller_type);
    }
    if let Some(controller_type) = env::var("MEGADRIVE_PAD2")
        .ok()
        .and_then(|value| parse_controller_type(&value))
    {
        memory.set_controller_type(2, controller_type);
    }
    let mut cpu = M68k::new();
    if hold_start {
        memory.set_button_pressed(Button::Start, true);
    }
    if hold_a {
        memory.set_button_pressed(Button::A, true);
    }
    cpu.reset(&mut memory);
    if force_b094 {
        memory.write_u32(0xFFFF_B094, 0x0000_50B2);
    }
    let initial_hash = framebuffer_hash(memory.frame_buffer());
    let mut pc_histogram: BTreeMap<u32, u64> = BTreeMap::new();
    let mut watch_return_histogram: BTreeMap<u32, u64> = BTreeMap::new();
    let mut watch_write_histogram: BTreeMap<u32, BTreeMap<u32, u64>> = BTreeMap::new();
    let mut trace: VecDeque<(usize, u32, u16, u32)> = VecDeque::new();
    let mut last_r1 = memory.vdp().register(1);
    let mut r1_trace_lines = 0usize;
    let mut watch_trace_lines = 0usize;
    let mut steps_executed = 0usize;

    for step_idx in 0..steps {
        steps_executed = step_idx + 1;
        while next_input_event < input_events.len()
            && input_events[next_input_event].frame <= memory.frame_count()
        {
            let event = input_events[next_input_event];
            if event.player == 2 {
                memory.set_button2_pressed(event.button, event.pressed);
            } else {
                memory.set_button_pressed(event.button, event.pressed);
            }
            next_input_event += 1;
        }

        let pc_before = cpu.pc();
        if force_b094_sticky {
            memory.write_u32(0xFFFF_B094, 0x0000_50B2);
        }
        let unknown_before = cpu.unknown_opcode_total();
        let opcode_before = read_opcode_at_pc(&memory, pc_before);
        trace.push_back((step_idx + 1, pc_before, opcode_before, cpu.a7()));
        if trace.len() > 256 {
            trace.pop_front();
        }
        if let Some(watched) = watch_pc {
            if pc_before == watched {
                let return_addr = memory.read_u32(cpu.a7());
                *watch_return_histogram.entry(return_addr).or_insert(0) += 1;
            }
        }
        let watch_before = if watch_addrs.is_empty() {
            Vec::new()
        } else {
            watch_addrs
                .iter()
                .map(|&addr| memory.read_u8(addr))
                .collect::<Vec<u8>>()
        };
        *pc_histogram.entry(pc_before).or_insert(0) += 1;
        let cycles = cpu.step(&mut memory);
        memory.step_subsystems(cycles);
        if memory.step_vdp(cycles) {
            memory.request_z80_interrupt();
            let frame = memory.frame_count();
            if !dump_frames.is_empty()
                && dump_frames.contains(&frame)
                && dumped_frames.insert(frame)
                && let Some(prefix) = dump_frame_prefix.as_deref()
            {
                let path = format!("{prefix}_{frame:06}.ppm");
                dump_frame_ppm(&path, memory.frame_buffer())?;
                println!("dump frame seq   : frame={} path={}", frame, path);
            }
            if let Some(target) = stop_frame
                && frame >= target
            {
                break;
            }
        }
        if !watch_addrs.is_empty() {
            for (idx, &addr) in watch_addrs.iter().enumerate() {
                let after = memory.read_u8(addr);
                let before = watch_before[idx];
                if after != before {
                    let by_pc = watch_write_histogram.entry(addr).or_default();
                    *by_pc.entry(pc_before).or_insert(0) += 1;
                    if watch_trace && watch_trace_lines < 256 {
                        println!(
                            "watch write step {}: pc=0x{:08X} opcode={:04X} addr=0x{:08X} {:02X}->{:02X}",
                            step_idx + 1,
                            pc_before,
                            opcode_before,
                            addr,
                            before,
                            after
                        );
                        watch_trace_lines += 1;
                    }
                }
            }
        }
        if trace_vdp_r1 {
            let r1 = memory.vdp().register(1);
            if r1 != last_r1 && r1_trace_lines < 128 {
                println!(
                    "r1 change step {}: pc=0x{:08X} {:02X}->{:02X}",
                    step_idx + 1,
                    cpu.pc(),
                    last_r1,
                    r1
                );
                r1_trace_lines += 1;
            }
            last_r1 = r1;
        }

        if stop_on_unknown && cpu.unknown_opcode_total() > unknown_before {
            let opcode = read_opcode_at_pc(&memory, pc_before);
            println!(
                "first unknown at step {}: pc=0x{:08X} opcode={:04X} next_pc=0x{:08X}",
                step_idx + 1,
                pc_before,
                opcode,
                cpu.pc()
            );
            print_recent_trace(&trace);
            break;
        }
        if stop_on_bad_pc && (cpu.pc() > 0x3F_FFFF || (cpu.pc() & 1) != 0) {
            println!(
                "bad pc at step {}: pc=0x{:08X} (prev_pc=0x{:08X} opcode={:04X})",
                step_idx + 1,
                cpu.pc(),
                pc_before,
                opcode_before
            );
            print_recent_trace(&trace);
            break;
        }
        if let Some(target_pc) = stop_on_pc {
            if cpu.pc() == target_pc {
                let sp = cpu.a7();
                let stack_top = memory.read_u32(sp);
                println!(
                    "hit target pc at step {}: pc=0x{:08X} (prev_pc=0x{:08X} opcode={:04X}) a7=0x{:08X} [a7]=0x{:08X}",
                    step_idx + 1,
                    cpu.pc(),
                    pc_before,
                    opcode_before,
                    sp,
                    stack_top
                );
                print_register_snapshot(&cpu);
                print_recent_trace(&trace);
                break;
            }
        }
    }

    println!("ROM             : {rom_path}");
    println!("steps           : {steps}");
    println!("steps executed  : {steps_executed}");
    println!("pc              : 0x{:08X}", cpu.pc());
    println!("cycles          : {}", cpu.cycles());
    println!("frames          : {}", memory.frame_count());
    println!("unknown total   : {}", cpu.unknown_opcode_total());
    println!("unknown distinct: {}", cpu.unknown_opcode_histogram().len());
    println!("exceptions      : {}", cpu.exception_histogram().len());
    println!("z80 cycles      : {}", memory.z80().cycles());
    println!("z80 unknown     : {}", memory.z80().unknown_opcode_total());
    println!(
        "z80 state       : pc=0x{:04X} sp=0x{:04X} a={:02X} f={:02X} bc=0x{:04X} de=0x{:04X} hl=0x{:04X} halted={} reset={} busreq={} busack={}",
        memory.z80().pc(),
        memory.z80().sp(),
        memory.z80().a(),
        memory.z80().f(),
        memory.z80().bc_reg(),
        memory.z80().de_reg(),
        memory.z80().hl_reg(),
        memory.z80().halted(),
        memory.z80().reset_asserted(),
        memory.z80().bus_requested(),
        memory.z80().bus_granted()
    );
    println!("ym writes       : {}", memory.audio().ym_write_count());
    println!("ym dac writes   : {}", memory.audio().ym_dac_write_count());
    println!(
        "ym src          : 68k={} z80={}",
        memory.audio().ym_writes_from_68k(),
        memory.audio().ym_writes_from_z80()
    );
    println!("psg writes      : {}", memory.audio().psg_write_count());
    println!(
        "psg src         : 68k={} z80={}",
        memory.audio().psg_writes_from_68k(),
        memory.audio().psg_writes_from_z80()
    );
    println!(
        "ym active ch    : {}",
        memory.audio().ym2612().active_channels()
    );
    println!(
        "ym regs         : 24={:02X} 25={:02X} 26={:02X} 27={:02X} 2A={:02X} 2B={:02X}",
        memory.audio().ym2612().register(0, 0x24),
        memory.audio().ym2612().register(0, 0x25),
        memory.audio().ym2612().register(0, 0x26),
        memory.audio().ym2612().register(0, 0x27),
        memory.audio().ym2612().register(0, 0x2A),
        memory.audio().ym2612().register(0, 0x2B)
    );
    let audio_channels = memory.audio().output_channels().max(1) as usize;
    let pending_audio = memory.pending_audio_samples();
    let pending_audio_frames = pending_audio / audio_channels;
    let probe_frames = 4096usize;
    let probe_samples = probe_frames * audio_channels;
    let stale_audio = pending_audio.saturating_sub(probe_samples);
    if stale_audio > 0 {
        let _ = memory.drain_audio_samples(stale_audio);
    }
    let audio_probe = memory.drain_audio_samples(probe_samples);
    let audio_nonzero = audio_probe.iter().filter(|&&sample| sample != 0).count();
    let audio_peak = audio_probe
        .iter()
        .map(|&sample| i32::from(sample).unsigned_abs() as i32)
        .max()
        .unwrap_or(0);
    let audio_rms = if audio_probe.is_empty() {
        0.0
    } else {
        let sum_sq: f64 = audio_probe
            .iter()
            .map(|&sample| {
                let s = f64::from(sample);
                s * s
            })
            .sum();
        (sum_sq / audio_probe.len() as f64).sqrt()
    };
    println!(
        "audio samples   : pending={} ({} frames @ {} ch) probe={} ({} frames) nonzero={} peak={} rms={:.1}",
        pending_audio,
        pending_audio_frames,
        audio_channels,
        audio_probe.len(),
        audio_probe.len() / audio_channels,
        audio_nonzero,
        audio_peak,
        audio_rms
    );
    let z80_boot: Vec<u8> = (0..16).map(|addr| memory.z80().read_ram_u8(addr)).collect();
    println!(
        "z80 ram[0..16]  : {}",
        z80_boot
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<String>>()
            .join(" ")
    );
    println!(
        "z80 ram[1B20..] : {:02X} {:02X} {:02X} {:02X}",
        memory.z80().read_ram_u8(0x1B20),
        memory.z80().read_ram_u8(0x1B21),
        memory.z80().read_ram_u8(0x1B22),
        memory.z80().read_ram_u8(0x1B23),
    );
    println!(
        "z80 ram[03B0..] : {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
        memory.z80().read_ram_u8(0x03B0),
        memory.z80().read_ram_u8(0x03B1),
        memory.z80().read_ram_u8(0x03B2),
        memory.z80().read_ram_u8(0x03B3),
        memory.z80().read_ram_u8(0x03B4),
        memory.z80().read_ram_u8(0x03B5),
        memory.z80().read_ram_u8(0x03B6),
        memory.z80().read_ram_u8(0x03B7),
    );
    print_register_snapshot(&cpu);
    let final_hash = framebuffer_hash(memory.frame_buffer());
    println!("frame hash init : 0x{initial_hash:016X}");
    println!("frame hash final: 0x{final_hash:016X}");
    println!(
        "frame colors    : {}",
        framebuffer_unique_colors(memory.frame_buffer())
    );
    print_vdp_snapshot(&memory);
    println!(
        "vdp writes      : data byte={} word={} control byte={} word={} fill={} copy={}",
        memory.vdp_data_byte_writes(),
        memory.vdp_data_word_writes(),
        memory.vdp_control_byte_writes(),
        memory.vdp_control_word_writes(),
        memory.vdp().dma_fill_ops(),
        memory.vdp().dma_copy_ops()
    );
    print_video_memory_snapshot(&memory);
    if dump_line_state {
        print_vdp_line_state_snapshot(&memory);
    }
    if dump_plane_state {
        print_vdp_plane_state_snapshot(&memory);
    }
    if dump_sprite_state {
        print_vdp_sprite_state_snapshot(&memory);
    }
    print_dma_trace(&memory, dma_trace_limit);
    print_ram_snapshot(&mut memory);

    for (idx, (opcode, count)) in cpu.unknown_opcode_histogram().iter().take(32).enumerate() {
        println!("{:2}. {:04X}  {}", idx + 1, opcode, count);
    }
    if cpu.unknown_opcode_total() > 0 {
        println!("unknown PCs:");
        for (idx, (pc, count)) in cpu
            .unknown_opcode_pc_histogram()
            .iter()
            .take(16)
            .enumerate()
        {
            let opcode = read_opcode_at_pc(&memory, *pc);
            println!("{:2}. {:08X}  {:04X}  {}", idx + 1, pc, opcode, count);
        }
    }
    let z80_unknown = memory.z80().unknown_opcode_histogram();
    if !z80_unknown.is_empty() {
        println!("z80 unknown opcodes:");
        for (idx, (opcode, count)) in z80_unknown.into_iter().take(16).enumerate() {
            println!("{:2}. {:02X}  {}", idx + 1, opcode, count);
        }
        println!("z80 unknown PCs:");
        for (idx, (pc, count)) in memory
            .z80()
            .unknown_opcode_pc_histogram()
            .into_iter()
            .take(16)
            .enumerate()
        {
            let b0 = memory.z80().read_ram_u8(pc);
            let b1 = memory.z80().read_ram_u8(pc.wrapping_add(1));
            let b2 = memory.z80().read_ram_u8(pc.wrapping_add(2));
            let b3 = memory.z80().read_ram_u8(pc.wrapping_add(3));
            println!(
                "{:2}. {:04X}  {}  [{:02X} {:02X} {:02X} {:02X}]",
                idx + 1,
                pc,
                count,
                b0,
                b1,
                b2,
                b3
            );
        }
    }
    if !cpu.exception_histogram().is_empty() {
        println!("exception vectors:");
        for (idx, (vector, count)) in cpu.exception_histogram().iter().take(16).enumerate() {
            println!("{:2}. {:>3}  {}", idx + 1, vector, count);
        }
    }
    println!("hot PCs:");
    for (idx, (pc, count)) in top_pc_entries(&pc_histogram)
        .into_iter()
        .take(16)
        .enumerate()
    {
        let opcode = read_opcode_at_pc(&memory, pc);
        println!("{:2}. {:08X}  {:04X}  {}", idx + 1, pc, opcode, count);
    }
    if let Some(watched) = watch_pc {
        println!("watch pc        : 0x{watched:08X}");
        println!("watch return PCs:");
        for (idx, (ret_pc, count)) in top_pc_entries(&watch_return_histogram)
            .into_iter()
            .take(16)
            .enumerate()
        {
            let opcode = read_opcode_at_pc(&memory, ret_pc);
            println!("{:2}. {:08X}  {:04X}  {}", idx + 1, ret_pc, opcode, count);
        }
    }
    if !watch_write_histogram.is_empty() {
        println!("watch address writes:");
        for (addr, by_pc) in &watch_write_histogram {
            println!("  addr 0x{addr:08X}:");
            for (idx, (pc, count)) in top_pc_entries(by_pc).into_iter().take(12).enumerate() {
                let opcode = read_opcode_at_pc(&memory, pc);
                println!("    {:2}. {:08X}  {:04X}  {}", idx + 1, pc, opcode, count);
            }
        }
    }

    if let Some(path) = dump_frame_path.as_deref() {
        dump_frame_ppm(path, memory.frame_buffer())?;
        println!("dump frame path  : {path}");
    }

    Ok(())
}

fn parse_cli_args(args: impl IntoIterator<Item = String>) -> Result<CliArgs, Box<dyn Error>> {
    let mut rom_path: Option<String> = None;
    let mut steps: Option<usize> = None;
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--steps" | "-s" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for --steps".to_string())?;
                let parsed = value.parse::<usize>()?;
                steps = Some(parsed);
            }
            "--help" | "-h" => {
                eprintln!(
                    "usage: cargo run -p megadrive-cli --bin opcode_profile -- <rom-path> [steps]\n       cargo run -p megadrive-cli --bin opcode_profile -- <rom-path> --steps <n>"
                );
                std::process::exit(0);
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}").into());
            }
            _ => {
                if rom_path.is_none() {
                    rom_path = Some(arg);
                } else if steps.is_none() {
                    steps = Some(arg.parse::<usize>()?);
                } else {
                    return Err(format!("unexpected positional argument: {arg}").into());
                }
            }
        }
    }

    let rom_path = rom_path.ok_or_else(|| {
        "usage: cargo run -p megadrive-cli --bin opcode_profile -- <rom-path> [steps]".to_string()
    })?;

    Ok(CliArgs {
        rom_path,
        steps: steps.unwrap_or(2_000_000),
    })
}

fn framebuffer_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

fn framebuffer_unique_colors(data: &[u8]) -> usize {
    let mut colors = BTreeSet::new();
    for pixel in data.chunks_exact(3) {
        let packed = ((pixel[0] as u32) << 16) | ((pixel[1] as u32) << 8) | pixel[2] as u32;
        colors.insert(packed);
    }
    colors.len()
}

fn top_pc_entries(pc_histogram: &BTreeMap<u32, u64>) -> Vec<(u32, u64)> {
    let mut entries: Vec<(u32, u64)> = pc_histogram
        .iter()
        .map(|(pc, count)| (*pc, *count))
        .collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    entries
}

fn read_opcode_at_pc(memory: &MemoryMap, pc: u32) -> u16 {
    if pc > 0x3F_FFFF {
        return 0;
    }
    let hi = memory.cartridge().read_u8(pc);
    let lo = memory.cartridge().read_u8(pc.wrapping_add(1));
    u16::from_be_bytes([hi, lo])
}

fn print_recent_trace(trace: &VecDeque<(usize, u32, u16, u32)>) {
    println!("recent trace:");
    for (step, pc, opcode, a7) in trace {
        println!(
            "  step {:>8}: pc=0x{:08X} opcode={:04X} a7=0x{:08X}",
            step, pc, opcode, a7
        );
    }
}

fn print_register_snapshot(cpu: &M68k) {
    println!(
        "regs            : D0={:08X} D1={:08X} D2={:08X} D3={:08X}",
        cpu.d_reg(0),
        cpu.d_reg(1),
        cpu.d_reg(2),
        cpu.d_reg(3)
    );
    println!(
        "                  D4={:08X} D5={:08X} D6={:08X} D7={:08X}",
        cpu.d_reg(4),
        cpu.d_reg(5),
        cpu.d_reg(6),
        cpu.d_reg(7)
    );
    println!(
        "                  A0={:08X} A1={:08X} A2={:08X} A3={:08X}",
        cpu.a_reg(0),
        cpu.a_reg(1),
        cpu.a_reg(2),
        cpu.a_reg(3)
    );
    println!(
        "                  A4={:08X} A5={:08X} A6={:08X} A7={:08X} SR={:04X}",
        cpu.a_reg(4),
        cpu.a_reg(5),
        cpu.a_reg(6),
        cpu.a_reg(7),
        cpu.sr_raw()
    );
}

fn print_vdp_snapshot(memory: &MemoryMap) {
    let vdp = memory.vdp();
    println!(
        "vdp regs        : r0={:02X} r1={:02X} r2={:02X} r3={:02X} r4={:02X} r5={:02X}",
        vdp.register(0),
        vdp.register(1),
        vdp.register(2),
        vdp.register(3),
        vdp.register(4),
        vdp.register(5)
    );
    println!(
        "                  r7={:02X} r10={:02X} r11={:02X} r12={:02X} r13={:02X} r15={:02X} r16={:02X} r17={:02X} r18={:02X}",
        vdp.register(7),
        vdp.register(10),
        vdp.register(11),
        vdp.register(12),
        vdp.register(13),
        vdp.register(15),
        vdp.register(16),
        vdp.register(17),
        vdp.register(18)
    );
}

fn print_video_memory_snapshot(memory: &MemoryMap) {
    let vdp = memory.vdp();
    let mut cram_nonzero = 0usize;
    let mut cram_preview = [0u16; 8];
    for i in 0..64u8 {
        let value = vdp.read_cram_u16(i);
        if (i as usize) < cram_preview.len() {
            cram_preview[i as usize] = value;
        }
        if value != 0 {
            cram_nonzero += 1;
        }
    }

    let mut vram_nonzero = 0usize;
    for addr in 0..0x1_0000u32 {
        if vdp.read_vram_u8(addr as u16) != 0 {
            vram_nonzero += 1;
        }
    }

    println!(
        "video snapshot   : cram_nonzero={}/64 vram_nonzero={}/65536",
        cram_nonzero, vram_nonzero
    );
    println!(
        "cram[0..8]       : {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
        cram_preview[0],
        cram_preview[1],
        cram_preview[2],
        cram_preview[3],
        cram_preview[4],
        cram_preview[5],
        cram_preview[6],
        cram_preview[7]
    );

    let hscroll_base = ((vdp.register(13) as usize & 0x3F) << 10) & 0xFFFF;
    let mut hscroll_words = [0u16; 8];
    let mut hscroll_nonzero = 0usize;
    for i in 0..(megadrive_core::FRAME_HEIGHT * 2) {
        let addr = hscroll_base + i * 2;
        let hi = vdp.read_vram_u8(addr as u16) as u16;
        let lo = vdp.read_vram_u8((addr + 1) as u16) as u16;
        let word = (hi << 8) | lo;
        if word != 0 {
            hscroll_nonzero += 1;
        }
    }
    for (i, slot) in hscroll_words.iter_mut().enumerate() {
        let addr = hscroll_base + i * 2;
        let hi = vdp.read_vram_u8(addr as u16) as u16;
        let lo = vdp.read_vram_u8((addr + 1) as u16) as u16;
        *slot = (hi << 8) | lo;
    }
    println!(
        "hscroll[0..8]    : {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} (nonzero {}/{})",
        hscroll_words[0],
        hscroll_words[1],
        hscroll_words[2],
        hscroll_words[3],
        hscroll_words[4],
        hscroll_words[5],
        hscroll_words[6],
        hscroll_words[7],
        hscroll_nonzero,
        megadrive_core::FRAME_HEIGHT * 2
    );
}

fn print_vdp_line_state_snapshot(memory: &MemoryMap) {
    let vdp = memory.vdp();
    println!("line state      : y r1 r2 r3 r4 r11 r13 r16 r17 r18 vs0 vs1 hsA hsB");
    for &y in &[
        0usize, 16, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 223,
    ] {
        let hs = vdp.line_hscroll_words(y);
        println!(
            "                  {:03} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:04X} {:04X} {:04X} {:04X}",
            y,
            vdp.line_register(y, 1),
            vdp.line_register(y, 2),
            vdp.line_register(y, 3),
            vdp.line_register(y, 4),
            vdp.line_register(y, 11),
            vdp.line_register(y, 13),
            vdp.line_register(y, 16),
            vdp.line_register(y, 17),
            vdp.line_register(y, 18),
            vdp.line_vsram_u16(y, 0),
            vdp.line_vsram_u16(y, 1),
            hs[0],
            hs[1]
        );
    }

    let mut changes = 0usize;
    let mut prev_r1 = vdp.line_register(0, 1);
    let mut prev_r2 = vdp.line_register(0, 2);
    let mut prev_r3 = vdp.line_register(0, 3);
    let mut prev_r4 = vdp.line_register(0, 4);
    let mut prev_r11 = vdp.line_register(0, 11);
    let mut prev_r13 = vdp.line_register(0, 13);
    let mut prev_r16 = vdp.line_register(0, 16);
    let mut prev_r17 = vdp.line_register(0, 17);
    let mut prev_r18 = vdp.line_register(0, 18);
    let mut prev_vs0 = vdp.line_vsram_u16(0, 0);
    let mut prev_vs1 = vdp.line_vsram_u16(0, 1);
    let mut prev_hs = vdp.line_hscroll_words(0);
    for y in 1..megadrive_core::FRAME_HEIGHT {
        let r1 = vdp.line_register(y, 1);
        let r2 = vdp.line_register(y, 2);
        let r3 = vdp.line_register(y, 3);
        let r4 = vdp.line_register(y, 4);
        let r11 = vdp.line_register(y, 11);
        let r13 = vdp.line_register(y, 13);
        let r16 = vdp.line_register(y, 16);
        let r17 = vdp.line_register(y, 17);
        let r18 = vdp.line_register(y, 18);
        let vs0 = vdp.line_vsram_u16(y, 0);
        let vs1 = vdp.line_vsram_u16(y, 1);
        let hs = vdp.line_hscroll_words(y);
        if r1 != prev_r1
            || r2 != prev_r2
            || r3 != prev_r3
            || r4 != prev_r4
            || r11 != prev_r11
            || r13 != prev_r13
            || r16 != prev_r16
            || r17 != prev_r17
            || r18 != prev_r18
            || vs0 != prev_vs0
            || vs1 != prev_vs1
            || hs != prev_hs
        {
            if changes < 24 {
                println!(
                    "                  change@{:03}: r1 {:02X}->{:02X} r2 {:02X}->{:02X} r3 {:02X}->{:02X} r4 {:02X}->{:02X} r11 {:02X}->{:02X} r13 {:02X}->{:02X} r16 {:02X}->{:02X} r17 {:02X}->{:02X} r18 {:02X}->{:02X} vs0 {:04X}->{:04X} vs1 {:04X}->{:04X} hsA {:04X}->{:04X} hsB {:04X}->{:04X}",
                    y,
                    prev_r1,
                    r1,
                    prev_r2,
                    r2,
                    prev_r3,
                    r3,
                    prev_r4,
                    r4,
                    prev_r11,
                    r11,
                    prev_r13,
                    r13,
                    prev_r16,
                    r16,
                    prev_r17,
                    r17,
                    prev_r18,
                    r18,
                    prev_vs0,
                    vs0,
                    prev_vs1,
                    vs1,
                    prev_hs[0],
                    hs[0],
                    prev_hs[1],
                    hs[1]
                );
            }
            changes += 1;
            prev_r1 = r1;
            prev_r2 = r2;
            prev_r3 = r3;
            prev_r4 = r4;
            prev_r11 = r11;
            prev_r13 = r13;
            prev_r16 = r16;
            prev_r17 = r17;
            prev_r18 = r18;
            prev_vs0 = vs0;
            prev_vs1 = vs1;
            prev_hs = hs;
        }
    }
    println!("line changes    : {}", changes);
}

fn print_vdp_plane_state_snapshot(memory: &MemoryMap) {
    fn plane_size_code_to_tiles(code: u8) -> usize {
        match code & 0x3 {
            0x0 => 32,
            0x1 => 64,
            0x3 => 128,
            _ => 32,
        }
    }

    fn read_name_entry(
        vdp: &megadrive_core::vdp::Vdp,
        base: usize,
        width: usize,
        x: usize,
        y: usize,
    ) -> u16 {
        let addr = (base + (y * width + x) * 2) & 0xFFFF;
        let hi = vdp.read_vram_u8(addr as u16) as u16;
        let lo = vdp.read_vram_u8(((addr + 1) & 0xFFFF) as u16) as u16;
        (hi << 8) | lo
    }

    let vdp = memory.vdp();
    let reg16 = vdp.register(16);
    let width = plane_size_code_to_tiles(reg16 & 0x03);
    let height = plane_size_code_to_tiles((reg16 >> 4) & 0x03);
    let plane_a_base = ((vdp.register(2) as usize & 0x38) << 10) & 0xFFFF;
    let plane_b_base = ((vdp.register(4) as usize & 0x07) << 13) & 0xFFFF;

    let mut a_nonzero = 0usize;
    let mut b_nonzero = 0usize;
    let mut a_priority = 0usize;
    let mut b_priority = 0usize;
    let mut a_row_nonzero = vec![0usize; height];
    let mut b_row_nonzero = vec![0usize; height];
    for y in 0..height {
        for x in 0..width {
            let a = read_name_entry(vdp, plane_a_base, width, x, y);
            let b = read_name_entry(vdp, plane_b_base, width, x, y);
            if a != 0 {
                a_nonzero += 1;
                a_row_nonzero[y] += 1;
            }
            if b != 0 {
                b_nonzero += 1;
                b_row_nonzero[y] += 1;
            }
            if (a & 0x8000) != 0 {
                a_priority += 1;
            }
            if (b & 0x8000) != 0 {
                b_priority += 1;
            }
        }
    }

    println!(
        "plane snapshot  : size={}x{} A_base={:04X} B_base={:04X}",
        width, height, plane_a_base, plane_b_base
    );
    println!(
        "                 A_nonzero={}/{} A_prio={} B_nonzero={}/{} B_prio={}",
        a_nonzero,
        width * height,
        a_priority,
        b_nonzero,
        width * height,
        b_priority
    );

    let mut rows_to_print = vec![0usize, 1, 2, 8, 16, 24, 31];
    if let Ok(range) = std::env::var("DUMP_PLANE_ROW_RANGE") {
        if let Some((start, end)) = range.split_once(':') {
            if let (Ok(start), Ok(end)) =
                (start.trim().parse::<usize>(), end.trim().parse::<usize>())
            {
                rows_to_print.clear();
                for y in start..=end {
                    if y < height {
                        rows_to_print.push(y);
                    }
                }
            }
        }
    }
    let row_cols = std::env::var("DUMP_PLANE_ROW_COLS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(3)
        .clamp(1, width.max(1));
    println!(
        "                 row_detail rows={} cols={}",
        rows_to_print.len(),
        row_cols
    );
    for y in rows_to_print {
        if y >= height {
            continue;
        }
        let mut a_vals = Vec::with_capacity(row_cols);
        let mut b_vals = Vec::with_capacity(row_cols);
        for x in 0..row_cols {
            a_vals.push(format!(
                "{:04X}",
                read_name_entry(vdp, plane_a_base, width, x, y)
            ));
            b_vals.push(format!(
                "{:04X}",
                read_name_entry(vdp, plane_b_base, width, x, y)
            ));
        }
        println!(
            "                 row{:02} A:{}  B:{}  nzA={:02}/{:02} nzB={:02}/{:02}",
            y,
            a_vals.join(" "),
            b_vals.join(" "),
            a_row_nonzero[y],
            width,
            b_row_nonzero[y],
            width
        );
    }
}

fn print_vdp_sprite_state_snapshot(memory: &MemoryMap) {
    let vdp = memory.vdp();
    let h40 = (vdp.register(12) & 0x01) != 0;
    let sat_mask = if h40 { 0x7E } else { 0x7F };
    let sat_base = ((vdp.register(5) as usize & sat_mask) << 9) & 0xFFFF;
    let read_word = |addr: usize| -> u16 {
        let hi = vdp.read_vram_u8((addr & 0xFFFF) as u16) as u16;
        let lo = vdp.read_vram_u8(((addr + 1) & 0xFFFF) as u16) as u16;
        (hi << 8) | lo
    };

    println!(
        "sprite snapshot : sat_base={:04X} mode={} (showing up to 40 linked entries)",
        sat_base,
        if h40 { "H40" } else { "H32" }
    );
    println!("                  idx link x y size tile attr pal prio hflip vflip mask nz c14 c15");

    let mut idx = 0usize;
    let mut visited = [false; 80];
    let mut traversed = 0usize;
    for _ in 0..80 {
        if idx >= 80 || visited[idx] {
            break;
        }
        visited[idx] = true;
        let entry = sat_base + idx * 8;
        let y_word = read_word(entry);
        let size_link = read_word(entry + 2);
        let attr = read_word(entry + 4);
        let x_word = read_word(entry + 6);

        let link = (size_link & 0x007F) as usize;
        let width_tiles = ((size_link >> 10) & 0x3) as usize + 1;
        let height_tiles = ((size_link >> 8) & 0x3) as usize + 1;
        let x = (x_word & 0x01FF) as i32 - 128;
        let y = (y_word & 0x03FF) as i32 - 128;
        let tile_start = (attr & 0x07FF) as usize;
        let pal = ((attr >> 13) & 0x3) as usize;
        let prio = (attr & 0x8000) != 0;
        let hflip = (attr & 0x0800) != 0;
        let vflip = (attr & 0x1000) != 0;
        let is_mask = (x_word & 0x01FF) == 0;
        let mut nonzero = 0usize;
        let mut control14 = 0usize;
        let mut control15 = 0usize;
        let tile_count = width_tiles * height_tiles;
        for tile_offset in 0..tile_count {
            let base = (tile_start + tile_offset) * 32;
            for row in 0..8usize {
                for col in 0..8usize {
                    let addr = (base + row * 4 + col / 2) & 0xFFFF;
                    let byte = vdp.read_vram_u8(addr as u16);
                    let px = if (col & 1) == 0 {
                        byte >> 4
                    } else {
                        byte & 0x0F
                    };
                    if px != 0 {
                        nonzero += 1;
                        if px == 14 {
                            control14 += 1;
                        } else if px == 15 {
                            control15 += 1;
                        }
                    }
                }
            }
        }
        let control14_ratio = if nonzero == 0 {
            0usize
        } else {
            (control14 * 100) / nonzero
        };
        let control15_ratio = if nonzero == 0 {
            0usize
        } else {
            (control15 * 100) / nonzero
        };

        if traversed < 40 {
            println!(
                "                  {:02}  {:02}  {:4} {:4} {}x{} {:04X} {:04X}  {}    {}     {}     {}    {}  {:4} {:3}% {:3}%",
                idx,
                link,
                x,
                y,
                width_tiles,
                height_tiles,
                tile_start,
                attr,
                pal,
                u8::from(prio),
                u8::from(hflip),
                u8::from(vflip),
                u8::from(is_mask),
                nonzero,
                control14_ratio,
                control15_ratio,
            );
        }
        traversed += 1;

        if link == 0 || link == idx {
            break;
        }
        idx = link;
    }
    println!("sprite entries  : {traversed}");
}

fn print_dma_trace(memory: &MemoryMap, limit: usize) {
    let trace = memory.dma_trace();
    if trace.is_empty() {
        println!("dma trace       : (none)");
        return;
    }
    println!(
        "dma trace       : {} entries (latest first, showing {})",
        trace.len(),
        limit.min(trace.len())
    );
    for entry in trace.iter().rev().take(limit) {
        let last_dest = entry.dest_addr.wrapping_add(
            entry
                .auto_increment
                .saturating_mul(entry.words.saturating_sub(1) as u16),
        );
        println!(
            "                  {:?} src=0x{:08X} dst=0x{:04X}..0x{:04X} inc={} words={} first={:04X} last={:04X}",
            entry.target,
            entry.source_addr,
            entry.dest_addr,
            last_dest,
            entry.auto_increment,
            entry.words,
            entry.first_word,
            entry.last_word
        );
    }
}

fn print_ram_snapshot(memory: &mut MemoryMap) {
    let f62a = memory.read_u8(0xFFFF_F62A);
    let f614 = memory.read_u8(0xFFFF_F614);
    let ffd8 = memory.read_u16(0xFFFF_FFD8);
    let ef3a = memory.read_u16(0xFFFF_EF3A);
    let b000 = memory.read_u32(0xFFFF_B000);
    let b004 = memory.read_u32(0xFFFF_B004);
    let b094 = memory.read_u32(0xFFFF_B094);
    println!(
        "ram snapshot     : [F62A]={:02X} [F614]={:02X} [FFD8]={:04X}",
        f62a, f614, ffd8
    );
    println!(
        "                  [EF3A]={:04X} [B000]={:08X} [B004]={:08X} [B094]={:08X}",
        ef3a, b000, b004, b094
    );
}

fn parse_u32_value(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse::<u32>().ok()
    }
}

fn parse_frame_list(value: &str) -> BTreeSet<u64> {
    let mut out = BTreeSet::new();
    for token in value.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if let Some((start, end)) = token.split_once('-') {
            let start = start.trim().parse::<u64>().ok();
            let end = end.trim().parse::<u64>().ok();
            if let (Some(start), Some(end)) = (start, end) {
                let (lo, hi) = if start <= end {
                    (start, end)
                } else {
                    (end, start)
                };
                for frame in lo..=hi {
                    out.insert(frame);
                }
                continue;
            }
        }
        if let Ok(frame) = token.parse::<u64>() {
            out.insert(frame);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::parse_cli_args;

    #[test]
    fn parse_cli_args_accepts_positional_steps() {
        let args = vec!["roms/Sonic.md".to_string(), "123".to_string()];
        let parsed = parse_cli_args(args).expect("must parse");
        assert_eq!(parsed.rom_path, "roms/Sonic.md");
        assert_eq!(parsed.steps, 123);
    }

    #[test]
    fn parse_cli_args_accepts_flag_steps() {
        let args = vec![
            "roms/Sonic.md".to_string(),
            "--steps".to_string(),
            "456".to_string(),
        ];
        let parsed = parse_cli_args(args).expect("must parse");
        assert_eq!(parsed.rom_path, "roms/Sonic.md");
        assert_eq!(parsed.steps, 456);
    }

    #[test]
    fn parse_cli_args_rejects_unknown_option() {
        let args = vec!["roms/Sonic.md".to_string(), "--bad".to_string()];
        let err = parse_cli_args(args).expect_err("must fail");
        assert!(err.to_string().contains("unknown option"));
    }

    #[test]
    fn parse_frame_list_accepts_single_values_and_ranges() {
        let frames = super::parse_frame_list("1,3-5,8");
        assert_eq!(
            frames.into_iter().collect::<Vec<u64>>(),
            vec![1, 3, 4, 5, 8]
        );
    }

    #[test]
    fn parse_frame_list_ignores_invalid_tokens() {
        let frames = super::parse_frame_list("2,foo,9-bar,7");
        assert_eq!(frames.into_iter().collect::<Vec<u64>>(), vec![2, 7]);
    }
}

fn parse_addr_list(value: &str) -> Vec<u32> {
    let mut addrs = BTreeSet::new();
    for part in value.split(',') {
        if let Some(addr) = parse_u32_value(part) {
            addrs.insert(addr & 0x00FF_FFFF);
        }
    }
    addrs.into_iter().collect()
}

fn parse_input_script(value: &str) -> Vec<InputEvent> {
    let mut events = Vec::new();
    for raw in value.split(';') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let parts: Vec<&str> = token.split(',').map(|part| part.trim()).collect();
        if parts.len() != 4 {
            continue;
        }

        let frame = match parts[0].parse::<u64>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let player = if parts[1].eq_ignore_ascii_case("P2") || parts[1] == "2" {
            2
        } else {
            1
        };
        let button = match parse_button(parts[2]) {
            Some(button) => button,
            None => continue,
        };
        let pressed = match parts[3] {
            "1" | "on" | "ON" | "down" | "DOWN" => true,
            "0" | "off" | "OFF" | "up" | "UP" => false,
            _ => continue,
        };
        events.push(InputEvent {
            frame,
            player,
            button,
            pressed,
        });
    }
    events
}

fn parse_button(value: &str) -> Option<Button> {
    if value.eq_ignore_ascii_case("UP") {
        Some(Button::Up)
    } else if value.eq_ignore_ascii_case("DOWN") {
        Some(Button::Down)
    } else if value.eq_ignore_ascii_case("LEFT") {
        Some(Button::Left)
    } else if value.eq_ignore_ascii_case("RIGHT") {
        Some(Button::Right)
    } else if value.eq_ignore_ascii_case("A") {
        Some(Button::A)
    } else if value.eq_ignore_ascii_case("B") {
        Some(Button::B)
    } else if value.eq_ignore_ascii_case("C") {
        Some(Button::C)
    } else if value.eq_ignore_ascii_case("X") {
        Some(Button::X)
    } else if value.eq_ignore_ascii_case("Y") {
        Some(Button::Y)
    } else if value.eq_ignore_ascii_case("Z") {
        Some(Button::Z)
    } else if value.eq_ignore_ascii_case("MODE") {
        Some(Button::Mode)
    } else if value.eq_ignore_ascii_case("START") {
        Some(Button::Start)
    } else {
        None
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

fn dump_frame_ppm(path: &str, rgb: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut file = std::fs::File::create(path)?;
    use std::io::Write as _;
    writeln!(
        file,
        "P6\n{} {}\n255",
        megadrive_core::FRAME_WIDTH,
        megadrive_core::FRAME_HEIGHT
    )?;
    file.write_all(rgb)?;
    Ok(())
}
