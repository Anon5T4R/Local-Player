# AGENTS.md — LocalPlayer

Player de mídia 100% local e offline, parte da suíte Local do João.
Rust puro + egui, app single-crate. Sem conta, sem telemetria, sem dependência de servidores de terceiros.

## Regras
- **Dependências:** toda dep que entra/sai/atualiza exige `Cargo.lock` commitado +
  linha no inventário do repo `Anon5T4R/local-deps` (regra completa em
  `local-deps/WORKFLOW.md`).
- **Sem fontes git:** nenhuma dependência pode apontar para `git+` (o CI valida).
  Tudo vem do crates.io (pinado pelo lock).
- **Commits:** português, sem Co-Authored-By.
- **Release:** tag `vX.Y.Z` → o Actions builda Windows (NSIS + zip) e Linux
  (AppImage + tar.gz), valida o PKGBUILD do Arch e publica. Mínimo local.
- **Disco local:** nunca deixe `target/` para trás — `cargo clean` após uso local;
  builds pesados são do CI.

## Estrutura
- `src/` — app single-crate
- `icons/` — ícones do app
- `packaging/windows/installer.nsi` — instalador NSIS (Program Files 64-bit)
- `packaging/linux/local-player.desktop` — desktop entry

## Build
```
cargo build --locked                       # debug
cargo test --locked                        # testes
makensis packaging/windows/installer.nsi   # instalador (Windows, precisa do build --release antes)
```
