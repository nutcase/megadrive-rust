# Mega Drive Emulator (Rust)

Rustで実装している Mega Drive / Genesis エミュレーターです。
68000 + Z80 + VDP + YM2612/PSG を `crates/core` に集約し、`crates/cli` で SDL/egui フロントエンドを提供しています。

現在は「実機互換性を崩さずに動作範囲を広げる」ことを優先し、実 ROM での検証と回帰テストを併用して改善を進めています。

## Implemented
- 68000 コア（主要命令群、例外処理、割り込み、主要アドレッシングモード）
- Z80 コア（命令実行、BUSREQ/RESET、68k との RAM バス仲裁）
- VDP（VRAM/CRAM/VSRAM、plane/sprite 合成、スクロール、window、DMA、NTSC/PAL タイミング）
- 入力（1P/2P、3ボタン/6ボタン）
- 音源経路（YM2612/PSG のレジスタモデルと出力ミキシング）
- チート UI（egui）
  - Hex Viewer
  - Cheat Search（スナップショット比較）
  - Cheat の有効/無効、編集、JSON保存/読込
- 診断ツール
  - `opcode_profile`（未知 opcode/例外/VDP DMA 追跡）
  - `dump_frame.sh`（フレーム/ログ/各種 VDP 診断ダンプ）

## Quick Start
推奨（release + チート UI 有効）:
```bash
./run.sh roms/<game>.md
./run.sh roms/<game>.md --boot-frames 600
```

従来 SDL フロントエンド（egui なし）:
```bash
./run.sh --no-egui roms/<game>.md
```

直接起動:
```bash
cargo run -p megadrive-cli --bin megadrive-egui -- roms/<game>.md
cargo run -p megadrive-cli --bin megadrive-cli -- roms/<game>.md
```

補足:
- `run.sh` はデフォルトで `megadrive-egui` を起動します。
- 起動前スキップは `--boot-frames` か `MEGADRIVE_BOOT_FRAMES` を使用します。
- パッド種別は `MEGADRIVE_PAD1` / `MEGADRIVE_PAD2` に `3` または `6` を指定します。

## Controls
- 1P
  - 十字: `↑ ↓ ← →`
  - `A/B/C`: `A / Z / X`
  - `X/Y/Z`: `S / D / F`
  - `Start/Mode`: `Enter / Q`
- 2P
  - 十字: `I K J L`
  - `A/B/C`: `R / T / Y`
  - `X/Y/Z`: `U / O / P`
  - `Start/Mode`: `Right Shift / /`
- 共通
  - 終了: `Esc`
  - チートパネル表示切替（egui版）: `Tab`

## Cheat UI
- チートファイルは `cheats/<ROM名>.json` に保存されます。
- Hex Viewer から直接 WRAM を編集できます。
- Cheat Search はスナップショット比較で候補を絞り込み、候補をそのまま Cheat へ追加できます。

## Debugging Tools
フレームダンプ（`opcode_profile` ラッパー）:
```bash
./dump_frame.sh roms/<game>.md --steps 11000000 --out /tmp/frame.png
./dump_frame.sh roms/<game>.md --stop-frame 900 --dump-line-state
```

`opcode_profile` 直接実行:
```bash
cargo run --release -p megadrive-cli --bin opcode_profile -- roms/<game>.md 12000000
```

## Project Layout
- `crates/core`: CPU/Z80/VDP/Audio/MemoryMap/入力などエミュレータ本体
- `crates/cli`: 実行フロントエンド (`megadrive-cli`, `megadrive-egui`, `opcode_profile`)
- `run.sh`: release ランチャー（デフォルト egui）
- `dump_frame.sh`: フレーム/診断ダンプ用スクリプト
- `roms/`: 手動検証用 ROM（ローカル）
- `tests`/`crates/core/tests`: 回帰テスト群

## Development Commands
```bash
cargo fmt
cargo test -q
cargo test -q -p megadrive-core
cargo test -q -p megadrive-cli
cargo check -q
```

## Known Limitations
- 命令/タイミング/バス仲裁は継続実装中で、タイトル依存の描画/音声差異が残る場合があります。
- VDP の一部エッジケース（特殊スクロールや優先度競合）は改善途中です。
- YM2612/PSG の完全な実機一致（微細なエンベロープ/ミキシング差）は未完です。

## License

教育・研究目的の実装です。
