# Troubleshooting

## Install/build issues

### `cargo install fabbula` fails

- Check Rust toolchain:

```bash
rustc --version
cargo --version
```

- Ensure build essentials are installed for your platform.
- Retry with a clean cargo cache if needed.

## CLI/runtime issues

### Large image is slow or memory-heavy

- Resize input before generation.
- Use `--max-width` / `--max-height`.
- Prefer `--strategy row-merge` for speed on large bitmaps.

### Output has too many polygons

- Try `--strategy greedy-merge` or `--strategy histogram-merge`.
- Increase image downscaling (`--max-width`, `--max-height`).

### Merge result overlaps existing metal

- Use `--exclusion-margin`.
- If needed, target specific source metal with `--exclusion-layer`.

### DRC violations remain

- Run with default DRC enabled (avoid `--no-check-drc`).
- Keep density enforcement enabled (avoid `--no-density-enforce`).
- Verify PDK TOML values and layer assignments.
- Always run external/foundry DRC for final signoff.

## GUI issues

### GUI opens but generation does nothing

- Confirm image path exists and is readable.
- Check `status` bar for parse/PDK errors.

### Canvas/die area behavior is unexpected

- Non-zero canvas values are fixed.
- `0` canvas dimensions trigger auto-fit behavior.
- `Use die bounds` uses chip GDS bounds for placement canvas.

### Merge in GUI fails

- Ensure `Chip GDS` path is set.
- Generate full-resolution result first (not preview-only).
- Check write permissions for output path.

## Platform notes

- macOS: if pre-commit tooling crashes (for example Taplo), verify locally outside constrained/sandboxed shells.
- Linux/Windows: ensure required system libs for image/GUI stack are present.
