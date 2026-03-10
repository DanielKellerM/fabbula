# Validation Artifacts

This directory is reserved for external validation evidence.

Goal: publish reproducible, tool-agnostic proof beyond internal `check_drc`.

## Planned Contents

```text
validation/
  README.md
  scripts/
    run_klayout_drc.sh
    collect_metrics.sh
  reports/
    sky130/
    gf180mcu/
    ihp_sg13g2/
  case_studies/
    merge_real_chip/
      input/
      output/
      logs/
      screenshots/
```

## Minimum Evidence Set

For each target PDK:

- command used to generate GDS
- command used to run external DRC
- raw log output
- summary (`0 violations` or exact rule failures)
- artifact hashes for traceability

## Reproducibility Rules

- Keep scripts deterministic and parameterized.
- Record tool versions (fabbula, klayout/deck revision, OS).
- Store paths in a repo-relative way where possible.

## Signoff Disclaimer

These artifacts improve confidence but do not replace official foundry signoff requirements.
