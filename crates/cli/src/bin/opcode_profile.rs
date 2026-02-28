use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::error::Error;

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
    let disable_sprites = env::var("DISABLE_SPRITES").is_ok();
    let force_window_off = env::var("FORCE_WINDOW_OFF").is_ok();
    let dump_line_state = env::var("DUMP_LINE_STATE").is_ok();
    let dump_frame_path = env::var("DUMP_FRAME").ok();
    let mut input_events = env::var("INPUT_SCRIPT")
        .ok()
        .map(|value| parse_input_script(&value))
        .unwrap_or_default();
    input_events.sort_by_key(|event| event.frame);
    let mut next_input_event = 0usize;

    let rom = std::fs::read(&rom_path)?;
    let cart = Cartridge::from_bytes(rom)?;
    let mut memory = MemoryMap::new(cart);
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

    for step_idx in 0..steps {
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
        }
        if force_window_off {
            memory.vdp_mut().write_control_port(0x9100);
            memory.vdp_mut().write_control_port(0x9200);
        }
        if disable_sprites {
            let sat_base = ((memory.vdp().register(5) as usize & 0x7F) << 9) & 0xFFFF;
            for offset in 0..(80 * 8) {
                memory
                    .vdp_mut()
                    .write_vram_u8((sat_base + offset) as u16, 0);
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
    println!("pc              : 0x{:08X}", cpu.pc());
    println!("cycles          : {}", cpu.cycles());
    println!("frames          : {}", memory.frame_count());
    println!("unknown total   : {}", cpu.unknown_opcode_total());
    println!("unknown distinct: {}", cpu.unknown_opcode_histogram().len());
    println!("exceptions      : {}", cpu.exception_histogram().len());
    println!("z80 cycles      : {}", memory.z80().cycles());
    println!("z80 unknown     : {}", memory.z80().unknown_opcode_total());
    println!(
        "z80 state       : pc=0x{:04X} halted={} reset={} busreq={} busack={}",
        memory.z80().pc(),
        memory.z80().halted(),
        memory.z80().reset_asserted(),
        memory.z80().bus_requested(),
        memory.z80().bus_granted()
    );
    println!("ym writes       : {}", memory.audio().ym_write_count());
    println!("psg writes      : {}", memory.audio().psg_write_count());
    println!(
        "ym active ch    : {}",
        memory.audio().ym2612().active_channels()
    );
    let pending_audio = memory.pending_audio_samples();
    let stale_audio = pending_audio.saturating_sub(4096);
    if stale_audio > 0 {
        let _ = memory.drain_audio_samples(stale_audio);
    }
    let audio_probe = memory.drain_audio_samples(4096);
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
        "audio samples   : pending={} probe={} nonzero={} peak={} rms={:.1}",
        pending_audio,
        audio_probe.len(),
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
    print_dma_trace(&memory);
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
    println!("line state      : y r1 r2 r3 r4 r11 r13 r16 vs0 vs1 hsA hsB");
    for &y in &[0usize, 16, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 223] {
        let hs = vdp.line_hscroll_words(y);
        println!(
            "                  {:03} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:04X} {:04X} {:04X} {:04X}",
            y,
            vdp.line_register(y, 1),
            vdp.line_register(y, 2),
            vdp.line_register(y, 3),
            vdp.line_register(y, 4),
            vdp.line_register(y, 11),
            vdp.line_register(y, 13),
            vdp.line_register(y, 16),
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
            || vs0 != prev_vs0
            || vs1 != prev_vs1
            || hs != prev_hs
        {
            if changes < 24 {
                println!(
                    "                  change@{:03}: r1 {:02X}->{:02X} r2 {:02X}->{:02X} r3 {:02X}->{:02X} r4 {:02X}->{:02X} r11 {:02X}->{:02X} r13 {:02X}->{:02X} r16 {:02X}->{:02X} vs0 {:04X}->{:04X} vs1 {:04X}->{:04X} hsA {:04X}->{:04X} hsB {:04X}->{:04X}",
                    y,
                    prev_r1, r1,
                    prev_r2, r2,
                    prev_r3, r3,
                    prev_r4, r4,
                    prev_r11, r11,
                    prev_r13, r13,
                    prev_r16, r16,
                    prev_vs0, vs0,
                    prev_vs1, vs1,
                    prev_hs[0], hs[0],
                    prev_hs[1], hs[1]
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
            prev_vs0 = vs0;
            prev_vs1 = vs1;
            prev_hs = hs;
        }
    }
    println!("line changes    : {}", changes);
}

fn print_dma_trace(memory: &MemoryMap) {
    let trace = memory.dma_trace();
    if trace.is_empty() {
        println!("dma trace       : (none)");
        return;
    }
    println!("dma trace       : {} entries (latest first)", trace.len());
    for entry in trace.iter().rev().take(8) {
        println!(
            "                  {:?} src=0x{:08X} words={} first={:04X} last={:04X}",
            entry.target, entry.source_addr, entry.words, entry.first_word, entry.last_word
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
    } else if value.eq_ignore_ascii_case("START") {
        Some(Button::Start)
    } else {
        None
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
