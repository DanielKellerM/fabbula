# Quickstart (5 Minutes)

This guide gets you from image to GDS/merge/GUI fast.

## 1. Install

```bash
cargo install fabbula
```

## 2. Generate standalone artwork (SKY130)

```bash
fabbula generate -i mascot.png -o mascot.gds -p sky130 --svg mascot.svg --html mascot.html
```

## 3. Merge into an existing chip GDS

```bash
fabbula merge -i mascot.png --chip my_chip.gds -o my_chip_art.gds -p sky130 --exclusion-margin 20.0
```

## 4. Launch GUI

```bash
fabbula gui -i mascot.png -p sky130
```

In GUI:

- tune threshold/strategy/options
- click `Generate`
- click `Save GDS` for standalone artwork
- set `Chip GDS`, then use `Merge & Save Chip GDS` for merge output

## Known-Good Starter Commands

### SKY130

```bash
fabbula generate -i mascot.png -o sky130_art.gds -p sky130 --threshold auto --strategy greedy-merge
```

### GF180MCU

```bash
fabbula generate -i mascot.png -o gf180_art.gds -p gf180mcu --threshold auto --strategy greedy-merge
```

### IHP SG13G2

```bash
fabbula generate -i mascot.png -o ihp_art.gds -p ihp_sg13g2 --threshold auto --strategy greedy-merge
```

## Before Tapeout

- Run your external/foundry DRC deck on the generated/merged output.
- Do not rely only on internal DRC checks for signoff.
