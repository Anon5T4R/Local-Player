# Upstream

## Base

- Upstream: `storytold/filmcraft` **`8fda2f2`** (2026-10-25), playback/decode only, vendored in `libs/filmcraft`.
  The vendor's divergences (and the comparison that proves the base) live in `docs/upstream/fork-patches.md`.
- `craft-update` is a mirror of local-libs (repo `Styx-Studios/local-libs`, crate `crates/update`): never edit the copy
  here — bugfix/feature go into the source and `sync.ps1` replicates them to the apps.
- Mentions of `storytold/*` are historical reference and license obligation; nothing is read from `<root>/storytold`
  in build, test or release.
