#!/usr/bin/env bash
# Generate preview gallery for all input images across PDK/mode/size combos.
# Outputs SVG + HTML to docs/previews/ with consistent naming.
#
# Usage: ./scripts/generate_gallery.sh [--dry-run]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
INPUT_DIR="$PROJECT_DIR/media/input"
OUTPUT_DIR="$PROJECT_DIR/docs/previews"
FABBULA="$PROJECT_DIR/target/release/fabbula"

DRY_RUN=""
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN="--dry-run"
fi

# Build release binary
echo "Building fabbula (release)..."
cargo build --release --bin fabbula --manifest-path "$PROJECT_DIR/Cargo.toml"

mkdir -p "$OUTPUT_DIR"

GENERATED=0
FAILED=0
SKIPPED=0

# Generate a single preview.
# Args: input_image output_slug pdk [extra_flags...]
generate() {
    local input="$1"
    local slug="$2"
    local pdk="$3"
    shift 3
    local extra_flags=("$@")

    local svg="$OUTPUT_DIR/${slug}.svg"
    local html="$OUTPUT_DIR/${slug}.html"
    local gds="/tmp/fabbula_gallery_${slug}.gds"

    if [[ -f "$svg" && -f "$html" && -z "$DRY_RUN" ]]; then
        echo "  SKIP  $slug (already exists)"
        ((SKIPPED++)) || true
        return 0
    fi

    echo "  GEN   $slug ($pdk)"
    if "$FABBULA" generate \
        -i "$input" \
        -o "$gds" \
        -p "$pdk" \
        --svg "$svg" \
        --html "$html" \
        --no-check-drc \
        --force \
        ${extra_flags[@]+"${extra_flags[@]}"} \
        $DRY_RUN \
        2>&1 | grep -E "(INFO|WARN|ERROR)" | tail -5; then
        ((GENERATED++)) || true
    else
        echo "  FAIL  $slug"
        ((FAILED++)) || true
    fi

    # Clean up temp GDS
    rm -f "$gds"
}

echo ""
echo "=== Generating gallery previews ==="
echo ""

# ---- Existing previews (re-generate for consistency) ----

echo "-- Re-generating existing previews --"
generate "$INPUT_DIR/fabbula.png"            "fabbula_sky130_multi_inv"   sky130     --color-mode palette --invert
generate "$INPUT_DIR/alpine_serenity.png"    "alpine_sky130_multi_inv"    sky130     --color-mode palette --invert
generate "$INPUT_DIR/futuristic_zurich.png"  "zurich_fabbula2_multi_inv"  fabbula2   --color-mode palette --invert
generate "$INPUT_DIR/futuristic_zurich.png"  "zurich_fabbula2"            fabbula2
generate "$INPUT_DIR/volcano_park.png"       "volcano_gf180_multi_inv"    gf180mcu   --color-mode palette --invert
generate "$INPUT_DIR/volcano_park.png"       "volcano_gf180"              gf180mcu
generate "$INPUT_DIR/bear.png"               "bear_gf180"                 gf180mcu
generate "$INPUT_DIR/cybernetic_owl.png"     "owl_freepdk45"              freepdk45
generate "$INPUT_DIR/ballroomm_jellyfish.png" "jellyfish_ihp_multi_inv"   ihp_sg13g2 --color-mode palette --invert
generate "$INPUT_DIR/fox_tea.png"            "fox_asap7_multi_inv"        asap7      --color-mode palette --invert

echo ""
echo "-- New previews: Geometric / Abstract --"
generate "$INPUT_DIR/mandala.png"            "mandala_sky130"             sky130
generate "$INPUT_DIR/mandala.png"            "mandala_gf180_multi_inv"    gf180mcu   --color-mode palette --invert

echo ""
echo "-- New previews: Portraits --"
generate "$INPUT_DIR/portrait.png"           "portrait_ihp"               ihp_sg13g2
generate "$INPUT_DIR/portrait.png"           "portrait_sky130_dither"     sky130     --dither

echo ""
echo "-- New previews: Technical / Engineering --"
generate "$INPUT_DIR/vacuum_tube.png"        "vacuum_tube_sky130"         sky130

echo ""
echo "-- New previews: Cultural / Heraldic --"
generate "$INPUT_DIR/crane_mon.png"          "crane_mon_gf180"            gf180mcu

echo ""
echo "-- New previews: Architecture --"
generate "$INPUT_DIR/golden_gate.png"        "golden_gate_fabbula2_large" fabbula2   --size-um 5000x5000

echo ""
echo "-- New previews: Space / Astronomy --"
generate "$INPUT_DIR/astronaut.png"          "astronaut_sky130_multi_inv" sky130     --color-mode palette --invert

echo ""
echo "-- New previews: Botanical --"
generate "$INPUT_DIR/lotus.png"              "lotus_ihp_multi_inv"        ihp_sg13g2 --color-mode palette --invert

echo ""
echo "-- New previews: Pixel Art / Retro --"
generate "$INPUT_DIR/pixel_ship.png"         "pixel_ship_freepdk45_small" freepdk45  --size-um 200x200
generate "$INPUT_DIR/pixel_ship.png"         "pixel_ship_sky130"          sky130

echo ""
echo "-- New previews: Logo --"
generate "$INPUT_DIR/shield_logo.png"        "shield_logo_gf180_small"    gf180mcu   --size-um 500x500

echo ""
echo "-- New previews: Fantasy --"
generate "$INPUT_DIR/dragon.png"             "dragon_asap7_multi_inv"     asap7      --color-mode palette --invert
generate "$INPUT_DIR/dragon.png"             "dragon_sky130"              sky130

echo ""
echo "-- New previews: Landscapes (new PDK combos) --"
generate "$INPUT_DIR/bear.png"               "bear_sky130"                sky130
generate "$INPUT_DIR/alpine_serenity.png"    "alpine_gf180_multi_inv"     gf180mcu   --color-mode palette --invert

echo ""
echo "=== Summary ==="
echo "  Generated: $GENERATED"
echo "  Skipped:   $SKIPPED"
echo "  Failed:    $FAILED"
echo "  Total:     $((GENERATED + SKIPPED + FAILED))"
echo ""
echo "Previews in: $OUTPUT_DIR"
