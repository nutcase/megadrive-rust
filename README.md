# Mega Drive Emulator (Rust)

Rustでメガドライブ（Genesis）エミュレーターを実装するための初期ワークスペースです。

## 構成

- `crates/core`:
  - エミュレーター本体の土台
  - `Cartridge`（ROM読み込み/ヘッダ解析）
  - `MemoryMap`（ROM + Work RAM + VDP + I/Oポート + Z80制御 + Z80 RAM窓口 + YM2612/PSGポート）
  - `M68k`（最小命令: `NOP` / `BRA` / `BSR` / `Bcc`（`BEQ/BNE/BCC/BCS/BPL/BMI/...`）/ `CMPI` / `CMP` / `TST` / `CLR` / `ORI` / `ANDI` / `EORI` / `ADDI` / `SUBI` / `ADDQ` / `SUBQ` / `Scc` / `DBcc` / `MOVEQ` / `MOVE`一部（`MOVE.B/W/L` + `MOVE to/from SR`）/ `MOVEM.W/L`（主要EA）/ `MOVEA` / `ADDA` / `MULU.W` / `MULS.W` / `DIVU.W` / `DIVS.W` / `AND` / `OR` / `EOR` / `ADD` / `SUB` / `SWAP` / `EXT.W/L` / `LINK` / `UNLK` / `LEA` / `PEA` / `BTST/BCHG/BCLR/BSET`（即値/動的）/ `JSR` / `JMP` / `RTS` / `TRAP` / `RTE` / `ILLEGAL` + VDP VBlank割り込みオートベクタ）
  - アドレッシング（実装済み範囲）: `Dn` / `(An)` / `(An)+` / `-(An)` / `(d16,An)` / `(d8,An,Xn)` / `(abs.w)` / `(abs.l)` / `(d16,PC)` / `(d8,PC,Xn)` / 一部 `#imm`
  - `Vdp`（VRAM/CRAM/VSRAM + 最小タイル描画 + Windowプレーン（簡易） + Plane A全画面H/Vスクロール（簡易） + 最小スプライト描画（SAT参照） + Plane/Sprite優先度（簡易） + DMA fill/copy + 68k->VDP DMA（簡易） + data/control/HV-counterポート副作用 + auto-increment + 背景色レジスタ反映 + register副作用 + ROMリージョン連動のNTSC/PALタイミング切替）
  - `IoBus`（3ボタン/6ボタンパッド入力、1P/2P）
  - `Z80`（BUSREQ/RESETレジスタ、BUSREQ→BUSACK遅延、Z80 RAM、サイクル進行スタブ）
  - `AudioBus`（YM2612レジスタ書き込み/PSG書き込み + 簡易モノラルサンプル生成スタブ）
  - `Emulator`（stepループ）
- `crates/cli`:
  - ROMを読み込んでヘッダ情報を表示
  - SDL2ウィンドウを開いてタイトルを表示
  - VDPのVRAM/CRAMから生成したフレームバッファを描画
  - `AudioBus` のサンプルを SDL2 `AudioQueue` へ送って再生
  - キー入力
    - 1P: 十字キー + `A`/`Z`/`X`/`Enter`（`A/B/C/Start`）、`S`/`D`/`F`/`Q`（`X/Y/Z/Mode`）
    - 2P: `I`/`J`/`K`/`L` + `R`/`T`/`Y`/`Right Shift`（`A/B/C/Start`）、`U`/`O`/`P`/`/`（`X/Y/Z/Mode`）

## 使い方

```bash
cargo test
cargo run -p megadrive-cli -- path/to/rom.bin
```

`megadrive-cli` は SDL2 ウィンドウを開き、ROM の国内向けタイトルをウィンドウタイトルに表示します。  
終了はウィンドウを閉じるか `Esc` キーです。

`run.sh` で release 実行できます。

```bash
./run.sh path/to/rom.bin
./run.sh path/to/rom.bin --boot-frames 600
```

起動前の高速スキップは `--boot-frames` を優先し、未指定時は環境変数 `MEGADRIVE_BOOT_FRAMES` を参照します。  
コントローラ種別は `MEGADRIVE_PAD1` / `MEGADRIVE_PAD2` (`3` または `6`) で切り替えできます（既定は `3`）。

## 次に実装する候補

1. 68000命令の拡張（`MOVE.B` / `ORI/ANDI` / 例外命令）
2. VDP描画精度向上（H/Vセルスクロール・優先度・sprite制限）とDMA精密化
3. Z80 RAM・68k/Z80バス仲裁の詳細化（現在は簡易BUSACK遅延モデル）
4. YM2612/PSGの実音生成とミキサー
