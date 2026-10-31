# ROADMAP — LocalPlayer

Backlog vivo. Itens confirmados pelo João em 2026-10-30.

## Bugs abertos

(nenhum — leva de 2026-10-30 fechada; ver "Feitos")

## Feitos

- [x] **mpv não fecha as instâncias direito** — o mecanismo Job Object
      (KILL_ON_JOB_CLOSE) foi provado em runtime (harness dedicado); o relato
      era da era pré-0.9.9. Reabrir se reaparecer.
- [x] **Vídeo invisível (tela azul) em janela e fullscreen** — v0.10.0: o
      renderer glow (OpenGL) do eframe cobria as child windows do mpv. Troca
      para wgpu/DX12 no Windows (molde local-office/graphics.rs).
- [x] **Comandos de teclado mortos** — v0.10.0: bindings Space/setas (seek,
      volume) no egui + F/Esc/Tab; com o foco na janela do mpv, os bindings
      nativos dele valem.
- [x] **OSD/controles não apareciam; cursor não escondia** — v0.10.0: a
      janela interna do mpv nasce WS_DISABLED (o Windows pula no hit-test,
      ninguém recebia o mouse); `enable_mpv_window` limpa o estilo → OSC
      nativo + cursor-autohide do mpv funcionam.
- [x] **Fullscreen bugado** — v0.10.0: `place()` usa `inner_rect` no
      fullscreen e reposiciona a janela do mpv junto (ela cobre o vídeo).
- [x] Instalador NSIS para `Program Files` 64-bit — v0.9.10+.

## Dívida técnica

(nenhuma)

## Feitos (migração craft)

- [x] **Migração do UI para o stack craft** — v0.11.0 (2026-10-31): egui
      forkado 0.37 vendored em `libs/` (revs do keys), craft-i18n TSV
      (pt/en/es, 149 chaves), tema Tokens System/Light/Dark, wgpu 30/DX12
      nativo. Comportamentos preservados 1:1; embed/mpv/job object intactos.

