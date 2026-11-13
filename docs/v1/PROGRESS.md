# Lens 1.0 progress (newest first)

## 2026-11-13 - steps 1 and 3: every P0 and P1 fixed (PR #17)
- `11a9332` behaviour locks; each fix below flips its lock or adds a test that failed before.
- P0: `34e97e3` + `0a8d14b` LN-1 (`--status`/`--hide` skip the install dialog; its fonts load before the first
  frame), LN-8 (a secret's history thumbnail is a blank tile), RD-LENS-1 (Ctrl+C arrives as `Event::Copy` on
  Wayland and now copies in the overlay and the annotator).
- P1: LN-3/BP-6 (window processes reaped on every command and a 2 s tick, D4), `671b5a9` LN-4 (focus loss never
  closes the overlay, D5), `a195404` LN-5 + RD-LENS-2/3 (Escape ladders), `8fd66d0` LN-2 (a copy that would vanish
  is an error naming xclip/wl-clipboard), `cadf34b` LN-6 (ffmpeg stopped on drop; no Record Window on Wayland),
  `707a5c8` LN-7 (solo pins keep their source area, so X11 pins offer Live).
- Tested: `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`. Not tested by hand: the
  fresh-profile AppImage dialog (LN-1) and the X11 recorder/pin paths; the owner checks those.
- P2: `3037cc3` LN-13 (IBANs are not phones), `e2aac59` LN-9 (colour codes in text), `ae30ff4` LN-12 (OCR lines
  under 0.2 confidence dropped), `ff19f8e` RD-LENS-6 (a table needs aligned columns), `1243784` LN-11 (`--status`
  counts Link-opened overlays). Each with a unit test that fails without it.
- Open: LN-10 (the loupe reads the same frozen image as the click, so the wrong colour is likely a hover-coordinate
  mismatch; needs the harness to reproduce) and RD-LENS-4 (pin menu clipped by the pin window); then steps 2, 4-8.

## 2026-11-11 - main builds carry the plain version (D6)
- `ci.yml`: builds from main and pull requests are labelled with the Cargo version, not `-dev.<sha>`, so Link's
  ecosystem e2e can publish them as the stable release Tools installs (three Lens checks failed on this). Checked by
  reading the workflow; CI on the PR runs it.
- Open: `release.yml` labels nightlies `<version>-dev.<sha>` while the fleet's nightly tags are
  `<version>-nightly.<date>.<n>` (Tools `source.rs`, Link D4); to check against Tools' nightly channel in step 4.

## Step 0 - brief
- docs/v1/SPEC.md, docs/v1/audit/ (text only) and DECISIONS.md added. Not code; nothing to run.

Open: everything in steps 1-7 (see the git log for what has landed). Link needs: none yet.
