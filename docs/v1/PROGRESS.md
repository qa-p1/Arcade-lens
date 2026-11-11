# Lens 1.0 progress (newest first)

## 2026-11-11 - main builds carry the plain version (D6)
- `ci.yml`: builds from main and pull requests are labelled with the Cargo version, not `-dev.<sha>`, so Link's
  ecosystem e2e can publish them as the stable release Tools installs (three Lens checks failed on this). Checked by
  reading the workflow; CI on the PR runs it.
- Open: `release.yml` labels nightlies `<version>-dev.<sha>` while the fleet's nightly tags are
  `<version>-nightly.<date>.<n>` (Tools `source.rs`, Link D4); to check against Tools' nightly channel in step 4.

## Step 0 - brief
- docs/v1/SPEC.md, docs/v1/audit/ (text only) and DECISIONS.md added. Not code; nothing to run.

Open: everything in steps 1-7 (see the git log for what has landed). Link needs: none yet.
