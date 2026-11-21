# AGENTS.md - Local Player

Player de mídia 100% local e offline em Rust puro — daily driver do João (o
antecessor `local-player` usava mpv + IPC). Rust puro + egui, app single-crate.
Sem conta, sem telemetria, sem dependência de servidores de terceiros, sem
processo externo de playback.

## Regras
- **Dependências:** toda dep que entra/sai/atualiza exige `Cargo.lock` commitado +
  linha no inventário do repo `Anon5T4R/local-deps` (regra completa em
  `local-deps/WORKFLOW.md`).
- **Sem fontes git:** nenhuma dependência pode apontar para `git+` (o CI valida).
  Tudo vem do crates.io (pinado pelo lock) ou de `libs/` (vendored).
- **Commits:** português, sem Co-Authored-By.
- **Release:** tag `vX.Y.Z` via `release.sh`; fluxo, gate local e checklist no
  `docs/git-protocol.md` do dev-notes.
- **Disco:** no workspace do João vale a política de `target/` do dev-notes (`MEMORY.md` §4 — limpa o que
  está frio ou feito, mantém durante iteração ativa); fora dele, `cargo clean` ao terminar. Builds pesados são do CI.

## Estrutura
- `src/` — app single-crate (`i18n.rs`, `prefs.rs`, `theme.rs`: idioma, preferências e tema do app); `src/engine/` é o motor de playback (threads
  próprias: decode symphonia/filmcraft, ring SPSC e saída cpal); `src/pickers.rs`:
  seletor nativo async via rfd + pollster (portal XDG no Linux)
- Casca da suíte = `src/shell.rs` + craft-icons + craft-palette + craft-desktop-theme
  (espelhos do local-libs)
- `icons/` — ícones do app (só `icon.png` 128×128; o PNG mestre maior ainda está pendente)
- `assets/app-icon/hicolor/` — árvore de ícones commitada (gerada por `packaging/icons.sh`)
- `libs/` — vendored, `exclude` do workspace do app:
  - `egui/` + `winit/` — forks do padrão craft
  - `craft-libs/` — desktop-theme, icons, palette (a casca), i18n, update e
    `craft-media`/`craft-docpath`
  - `filmcraft/` — workspace próprio (`libs/filmcraft/Cargo.toml`) com os crates
    de container/codec do FilmCraft usados pelo motor; `crates/hwdec`
    (`filmcraft-hwdec`) é o recorte do `filmcraft-platform` do upstream: decoders de
    hardware do Windows (Media Foundation/DXVA) e do Linux (VA-API/NVDEC) com
    fallback SW transparente
- `packaging/windows/package.ps1` + `local-player-rs.wxs` — MSI per-machine (WiX v5,
  Program Files\Local Player RS) e portable zip
- `packaging/linux/` — `package.sh` (AppImage + tar FHS + install.sh + deb/rpm),
  `local-player-rs.desktop`, `local-player-rs.metainfo.xml.in`, `nfpm.yaml`, `postinst.sh`
- `packaging/arch/PKGBUILD` — pacote Arch a partir do tarball FHS da release

## Build
```
cargo check                                # motor + shell (sem target/ depois: cargo clean)
cargo build --locked                       # debug
cargo test --locked                        # testes
cargo xtask ci                             # o gate local: o mesmo que o CI (fmt, clippy, build, testes, cargo-deny,
                                           # espelhos do local-libs)
cargo xtask version set X.Y.Z              # bump (Cargo.toml + lock)
cargo xtask check                          # confere as versões sincronizadas
```

## Upstream e a pasta `<root>/storytold`

Menções a `storytold/*` em código, docs e atribuições (NOTICE/ATTRIBUTION) são referência histórica e
obrigação de licença — nada é lido dessa pasta em build, teste ou release.

Divergências entre o craft-libs vendored e o que o FilmCraft espera se resolvem
**no fork** (`libs/filmcraft`), nunca em `libs/craft-libs`.
