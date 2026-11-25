# Upstream

## Base

- Upstream: `storytold/filmcraft` **`8fda2f2`** (2026-10-25), só playback/decode, vendored em `libs/filmcraft`.
  As divergências do vendor (e a comparação que prova a base) vivem em `docs/upstream/fork-patches.md`.
- `craft-update` é espelho do local-libs (repo `Styx-Studios/local-libs`, crate `crates/update`): nunca edite a cópia
  daqui — bugfix/feature vão na fonte e o `sync.ps1` replica para os apps.
- Menções a `storytold/*` são referência histórica e obrigação de licença; nada é lido de `<root>/storytold` em build,
  teste ou release.
