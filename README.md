# LocalPlayer

**Player de mídia 100% local e offline.** Interface egui minimalista em cima do mpv
(embedded por IPC — sem re-encode, sem navegador, sem telemetria). Gemeo do
[Local-Keys](https://github.com/Anon5T4R/Local-Keys), piloto da suíte Local em
egui/eframe.

## Download

Pegue o asset mais recente na página de
[Releases](https://github.com/Anon5T4R/Local-Player/releases):

| Asset | O que é |
| --- | --- |
| `local-player-setup.exe` | Instalador NSIS (atalhos no Desktop + Menu Iniciar) |
| `local-player-windows-x64.zip` | Portable (um `.exe` só) |
| `local-player-x86_64.AppImage` | Linux, AppImage |
| `local-player-linux-x86_64.tar.gz` | Linux, portable |

O app é midia-agnóstico: **tudo que o mpv toca, o LocalPlayer toca** (MKV/MP4/WebM/AVI/
MOV, MP3/FLAC/OGG/WAV...).

## mpv: baixado na primeira execução, do nosso espelho

O binário do mpv **não vem no instalador** (o app fica levinho). Na 1ª execução o
LocalPlayer baixa o mpv x86_64 (~32 MB) para `%LOCALAPPDATA%\LocalPlayer\mpv` e o
vínculo fica lá pra sempre. O download **não sai de CDN de terceiros**: vem do
espelho da própria suíte
([Anon5T4R/big-files-local](https://github.com/Anon5T4R/big-files-local/releases/tag/mpv-x86_64-20260809))
e o arquivo baixado é conferido por **SHA-256** na hora — se divergir, é descartado.
No Linux o mpv segue do caminho normal (`sudo apt install mpv`, ou o pacote da distro).

## Features

- **Playlist** persistente entre sessões: `+ arquivo` / `+ pasta` (multi-arquivo, roda
  fora da UI thread — pasta de rede não trava a janela) ou arraste os arquivos pra
  janela; "abrir com" no Explorer também funciona (caminhos via args).
- **Barra de progresso arrastável** — drag com dedupe (o slider não "briga" com o
  progresso do vídeo; seek só no `drag_stopped`).
- **Atalhos de teclado**:

| Tecla | Ação |
| --- | --- |
| `←` / `→` | voltar / avançar 10 s |
| `↑` / `↓` | volume ±5 |
| `M` | mute / desmute |
| `Espaço` | play / pause |
| `[` / `]` | girar o vídeo 90° outro sentido |
| `F` | fullscreen (Esc sai) |
| Clique no vídeo | mostra / esconde a interface |

- **Fullscreen "solo"**: só o vídeo; a interface some; **cursor some 3 s depois** de
  parar o mouse e volta a qualquer movimento.
- **Retoma de onde parou** (`resume.json`, cap LRU de 200): fecha o app, volta e o
  vídeo recomeça do ponto em que você saiu.
- **Volume/mute salvão**: os preferidos do player são lembrados
  entre sessões (`player.json`).
- **Janela no tamanho do vídeo** — quando a interface está escondida, a janela se
  acomoda nas dimensões do arquivo (com clamp na área útil do monitor).
- **Autoupdate**: check no GitHub da própria release (estojo Windows: substitui o
  `.exe` de verdade; Linux: AppImage/tar.gz também).
- **Temas light/dark e i18n PT/EN/ES** comuns da suíte (crate `lab-ui`).

## Build

```bash
cargo build --release
```

O `Cargo.lock` está commitado; `build.rs` embute o `icon.ico` no binário do Windows.

## Design / arquitetura (resumo)

- **Motor separado**: o mpv roda como processo próprio controlado por
  `--input-ipc-server` (JSON IPC). Nada de crate nativo de decode — o app nunca
  trava no decode pesado; tudo async por canais mpsc.
- **Windows**: vídeo embutido na janela do player via child window (`--wid`),
  cliques/cursor capturados por um `WndProc` próprio (subclasse do child).
- **Linux**: mesmo `--wid` em X11/XWayland (eventos via X11 direto); Wayland puro
  cai no mpv em janela própria gracefully.

Licença: GPL-3.0-or-later.
