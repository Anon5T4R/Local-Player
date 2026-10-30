# ROADMAP — LocalPlayer

Backlog vivo. Itens confirmados pelo João em 2026-10-30.

## Bugs abertos

- [ ] **mpv não fecha as instâncias direito** — processos órfãos ao fechar o app
      (investigar: teardown do processo filho no exit; `mpv_job.rs`/`mpv_setup.rs`).
- [ ] **OSD/controles não aparecem sobre o vídeo** ao passar o mouse ou clicar
      (OSC embutido + cursor-hide: `embed.rs`, hook de mouse).
- [ ] **Comandos de teclado pararam de funcionar** (setas, espaço etc.) —
      foco/WndProc do embed: `embed.rs` (`ESC_FLAG`, `OLD_PROC`).
- [ ] **Fullscreen bugado** — fullscreen da janela vs fullscreen do próprio mpv
      (`Cmd::Fullscreen` / `Event::Fullscreen`), estado dessincronizado.
      Ver também v0.9.9 "Esc garantido via flag do child".

## Feitos (aguardando release)

- [x] Instalador NSIS para `Program Files` 64-bit (`$PROGRAMFILES64`) — commit
      2621386; o release da tag que continha o fix falhou no job do NSIS
      (caminho relativo do `File`), vai sair na próxima tag.

## Dívida técnica

- [ ] Migração do UI do lab-ui para o stack craft (padrão storytold,
      craft-ui/craft-i18n + egui vendored) — decisão do João em 2026-10-30:
      UI completa no padrão office/pdf. Fazer DEPOIS dos bugs acima.
