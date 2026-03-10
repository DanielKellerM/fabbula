# `fabbula package-macro` Specification

Status: Draft (v0.1)
Owner: fabbula maintainers
Scope: Rust-only helper for producing a ready-to-drop macro artifact bundle.

## Goal

Provide a single CLI command that packages fabbula outputs into a deterministic folder layout
for downstream physical-design flows, without runtime coupling to OpenLane/OpenROAD or any
non-Rust tooling.

This command is an artifact packager, not a flow runner.

## Non-Goals

- No invocation of OpenLane/OpenROAD/KLayout from `fabbula`.
- No generation of Tcl/Python wrappers that execute external tools.
- No process-specific magic defaults that hide required physical constraints.

## Command Summary

```bash
fabbula package-macro \
  --input <image> \
  --pdk <pdk-name-or-toml> \
  --out-dir <dir> \
  --macro-name <name> \
  [generation options...]
```

The command internally performs generation and exports:

- GDS (`<macro-name>.gds`)
- LEF (`<macro-name>.lef`)
- Manifest JSON (`manifest.json`)
- Human README (`README.txt`)
- Optional previews (`preview.svg`, `preview.html`)

## CLI Contract

### Required Arguments

- `--input <path>`
  - Source artwork (PNG/JPG/BMP/GIF/SVG)
- `--pdk <id-or-path>`
  - Built-in PDK id or custom TOML path
- `--out-dir <path>`
  - Output bundle directory (created if missing)
- `--macro-name <string>`
  - Macro/cell name used for exported files and metadata

### Optional Arguments

- `--library-name <string>` (default: `fabbula`)
- `--threshold <0-255|otsu|auto|alpha>` (default: `128`)
- `--strategy <pixel-rects|row-merge|greedy-merge|histogram-merge>` (default: `greedy-merge`)
- `--separated`
- `--invert`
- `--rotate <0|90|180|270>` (default: `0`)
- `--flip <horizontal|vertical>`
- `--max-width <u32>`
- `--max-height <u32>`
- `--size-um <WxH>`
- `--dither`
- `--no-check-drc`
- `--no-density-enforce`
- `--force`
- `--with-svg`
- `--with-html`
- `--metadata-only`
  - Validate inputs and write manifest template without generating geometry

### Exit Semantics

- Exit `0`: packaging succeeded.
- Exit non-zero: any generation/export/validation failure.
- Error output should be concise and include the failing stage (`load`, `generate`, `write_gds`, etc.).

## Output Layout

Given `--out-dir build/macro_bundle --macro-name logo_macro`:

```text
build/macro_bundle/
  logo_macro.gds
  logo_macro.lef
  manifest.json
  README.txt
  preview.svg          # optional
  preview.html         # optional
```

File naming must be deterministic and derived from `--macro-name`.

## `manifest.json` Schema (v1)

```json
{
  "schema_version": "1.0",
  "tool": {
    "name": "fabbula",
    "version": "0.1.0"
  },
  "macro": {
    "name": "logo_macro",
    "library_name": "fabbula",
    "gds_file": "logo_macro.gds",
    "lef_file": "logo_macro.lef"
  },
  "input": {
    "path": "relative/original/input.png",
    "hash_sha256": "..."
  },
  "pdk": {
    "id_or_path": "sky130",
    "resolved_name": "sky130",
    "db_units_per_um": 1000
  },
  "generation": {
    "threshold": "128",
    "strategy": "greedy-merge",
    "separated": false,
    "invert": false,
    "rotate": 0,
    "flip": null,
    "dither": false,
    "density_enforced": true,
    "drc_checked": true
  },
  "results": {
    "polygon_count": 0,
    "bounds_dbu": { "x0": 0, "y0": 0, "x1": 0, "y1": 0 },
    "bounds_um": { "width": 0.0, "height": 0.0 },
    "drc_violation_count": 0
  },
  "artifacts": {
    "files": [
      { "name": "logo_macro.gds", "sha256": "...", "size_bytes": 0 },
      { "name": "logo_macro.lef", "sha256": "...", "size_bytes": 0 },
      { "name": "manifest.json", "sha256": "...", "size_bytes": 0 },
      { "name": "README.txt", "sha256": "...", "size_bytes": 0 }
    ]
  },
  "created_at_utc": "2026-03-10T00:00:00Z"
}
```

Notes:

- `schema_version` allows future evolution.
- All file references should be bundle-relative paths.
- Hashes are SHA-256 lowercase hex.

## `README.txt` Contents

Human-readable summary for handoff:

- What was generated and with which settings.
- Macro name/library name.
- PDK and units.
- Polygon count and bounds.
- DRC summary (`clean` or count).
- Explicit disclaimer:
  - compatibility artifact bundle, not a signoff replacement.
  - run project/foundry flow checks externally.

## Determinism Requirements

- Same inputs + same command options -> byte-identical manifest ordering and file naming.
- Timestamps only in `created_at_utc`; optionally allow `--fixed-timestamp` for reproducible CI.
- JSON keys emitted in stable order.

## Validation Rules

Before writing outputs:

- `--macro-name` must be non-empty and safe for file names.
- `--out-dir` writable.
- Input file exists and is supported.
- PDK resolves and validates.

After generation:

- GDS and LEF files exist and are non-empty.
- Manifest references all emitted artifacts.
- If DRC check is enabled and violations exist, fail unless explicitly overridden by existing CLI semantics.

## Implementation Plan (Rust-Only)

1. Add `PackageMacro` subcommand to CLI.
2. Reuse existing generate path (single source of truth for geometry).
3. Emit bundle files through existing writers (`write_gds_multi`, `write_lef_multi`, preview writers).
4. Add manifest builder struct + serde serialization.
5. Add hashing utility for emitted files.
6. Add integration tests for:
   - success path
   - deterministic naming/layout
   - manifest completeness
   - invalid macro name / bad pdk / missing input

## Acceptance Criteria

- A user can run one command and get a complete bundle in a fresh folder.
- Bundle can be consumed manually by downstream flows without additional generated scripts.
- All output metadata is auditable from `manifest.json`.
- Command works without OpenLane/OpenROAD/KLayout installed.
