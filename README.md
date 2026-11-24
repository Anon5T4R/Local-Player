<p align="center">
  <img src="assets/app-icon/hicolor/128x128/apps/local-player-rs.png" width="96" height="96" alt="Local Player icon">
</p>

<h1 align="center">Local Player</h1>

<p align="center">
  A video and audio player for Windows and Linux whose file readers and decoders are written in Rust.<br>
  <a href="https://github.com/Anon5T4R/Local-Player/releases/latest">Download</a> ·
  <a href="ROADMAP.md">Roadmap and status</a> ·
  <a href="#where-it-comes-from">Where it comes from</a> ·
  <a href="#how-it-is-made">How it is made</a>
</p>

<!-- README-SHOT: a film playing with subtitles, controls visible; dark and light -->
<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/readme/main-dark.png">
    <img src="docs/images/readme/main-light.png" width="820" alt="Local Player playing a video with subtitles">
  </picture>
</p>

## What is different about it

Most players, including mpv and VLC, decode through FFmpeg or GStreamer, which are large C libraries.
Local Player reads the containers and decodes the video and audio with Rust code: the clean-room
decoders of [FilmCraft](https://github.com/storytold/filmcraft), plus
[symphonia](https://github.com/pdeljanov/Symphonia) for part of the audio formats. When the system offers hardware
decoding (Media Foundation/DXVA on Windows, VA-API/NVDEC on Linux), it is used first, and the Rust
decoder takes over when it is not available.

What that buys: no C code reads the file you open, and one toolchain builds the whole player. What
it costs: fewer formats than FFmpeg, and software decoding that is slower than FFmpeg's hand-tuned
assembly, so very large files (4K HEVC, for example) depend on hardware decoding.

## What it plays

- **Video containers:** MP4, M4V, MOV, MKV, WebM, AVI, MPEG-TS (`.ts`, `.m2ts`).
- **Video codecs:** H.264, HEVC (Main and Main 10), AV1, VP8, VP9.
- **HDR:** HEVC Main 10 with PQ or HLG, kept at 10 or 12 bits all the way to the GPU and tone-mapped
  to SDR with ITU-R BT.2390 (the peak comes from the file's MaxCLL or mastering metadata).
- **Audio files:** MP3, FLAC, Opus, Vorbis, WAV, AIFF, M4A (AAC, ALAC).
- **Audio inside video:** AAC, MP3, Opus, AC-3, E-AC-3, FLAC, Vorbis, PCM.
- **Subtitles:** external `.srt`, `.vtt`, `.ass` and `.ssa`, found next to the video automatically
  (`film.srt`, `film.pt-br.srt`, …); text tracks inside MKV and MP4 (`tx3g`), picked by language.
- **Playlist** kept between sessions; open files or a whole folder, or drop them on the window.

What is missing or partial (bitmap subtitles such as PGS, gapless playback, variable frame rate, the
BT.2020 color gamut, advanced ASS effects, older codecs like MPEG-4 Part 2 or VC-1) is tracked in the
[roadmap](ROADMAP.md). It plays local files only: no network streams, discs or online services.

## Download

Windows 10/11 (x64): `.msi` installer or portable `.zip`. Linux (x86_64): `.AppImage` or `.tar.gz`.
All in [Releases](https://github.com/Anon5T4R/Local-Player/releases/latest). macOS is not published.

Each file is signed with [minisign](https://jedisct1.github.io/minisign/), the same key as every
Local app:

```sh
minisign -Vm <file> -P RWTqN+hTTCFbDCGCA1cEJRLp2FSluxIUiJfqdoAYwkwNNVH5MTYH8qSN
```

## Network and privacy

- The player never sends anything about what you watch.
- On start it asks GitHub if a newer version exists, and downloads it only if you accept. There is
  no setting to turn the check off yet.
- No account, analytics or crash reporting. The diagnostic logs below stay in your user folder.

Security notes, all checkable in this repository:

- The container readers and the video decoders (H.264, HEVC, AV1, VP8, VP9) contain no `unsafe`
  code. In the player itself, `unsafe` appears only in calls to the Windows API (console, thread
  priority, opening a link); the hardware decoding layer uses it to talk to the OS video APIs.
- TLS for the update check is pure Rust (rustls with RustCrypto); no `ring`, `aws-lc` or OpenSSL.
- Releases are built by GitHub Actions from a tagged commit with locked dependencies, checked by
  `cargo-deny`.

## Where it comes from

Local Player is our own app, not a fork of another player. The decoding side is vendored from
[FilmCraft](https://github.com/storytold/filmcraft), an open-source video editor by
[ArtCraft](https://getartcraft.com) (MIT or Apache-2.0), at commit `8fda2f2`: only the parts a player
needs (containers, decoders, hardware decoding), without the editor, effects or encoders. The
decoders are their work.

We keep the vendored copy close to upstream and record every change we make to it in
[docs/upstream/fork-patches.md](docs/upstream/fork-patches.md). Fixes to the decoders that apply
upstream should go to FilmCraft as issues or pull requests rather than live only here.

What is ours: the playback engine (audio as the master clock, frame queue, seeking), the GPU
presentation (YUV to RGB in a WGSL shader, the 10-bit path and the HDR tone mapping), the subtitle
parsers and renderer, the playlist, the diagnostics, and the shell shared by all Local apps.

## How the app is built

```
UI (egui)
 └─ engine thread: read container → decode (FilmCraft codecs, or hardware) → frame queue
      ├─ video: YUV → wgpu texture → shader to RGB, tone mapping for HDR
      ├─ subtitles: the cue active at the master clock, drawn over the video
      └─ audio: cpal output fed from a ring buffer; audio is the master clock
```

## Diagnostics

Every run writes a session log (the last 10 are kept) to `%APPDATA%\local-player-rs\logs\` on Windows
or `~/.local/share/local-player-rs/logs/` on Linux: one line per file played and one per anomaly
(stalls, dropped frames, slow uploads, high memory). Press `D` for a live overlay with decode and
display rates, drops, delay and memory. From the command line:

```sh
local-player-rs --probe film.mkv                 # container, codecs, size, frame rate, bitrate
local-player-rs --bench film.mkv --seconds 120   # decode speed without audio, GPU or UI
```

Optional environment variables (the defaults are the recommended values):

| Variable | Effect |
|---|---|
| `LOCALPLAYER_STATS` | `<file.csv>` (or `1` for a file in the temp folder): one row per second; the most useful thing to attach when reporting a stutter |
| `LOCALPLAYER_DECODE_THREADS` | maximum decoder threads (default `0` = all cores) |
| `LOCALPLAYER_FRAME_CACHE_MB` | cap on the decoded-frame cache (default 128) |
| `LOCALPLAYER_HDR_PEAK` | peak brightness in nits when the file does not say (default 1000) |
| `LOCALPLAYER_HDR_TONEMAP` | `bt2390` (default) or `hable` |

On Windows the release `.exe` is a GUI program, so PowerShell does not wait for `--probe` or `--bench`
to finish; use `cmd /c "local-player-rs --bench film.mkv"` in scripts.

## Keyboard shortcuts

| Key | Action |
|---|---|
| `Space` | play / pause |
| `←` / `→` | back / forward 5 s |
| `F`, `Esc` | full screen / leave full screen |
| `R` / `Shift+R` | rotate 90° clockwise / counterclockwise |
| `M` | mute |
| `S` / `C` | subtitles on/off / next embedded subtitle track |
| `Z` / `X` | subtitle delay −0.5 s / +0.5 s |
| `D` | statistics overlay |

## Build from source

```sh
git clone https://github.com/Anon5T4R/Local-Player
cd Local-Player
cargo build --release
```

## How it is made

Local Player follows the same rules as every Local app: local files and no account, Rust only
(including the UI and the cryptography), one self-contained repository per app, governed
dependencies, signed releases built from a tag, and MCP so agents can use the app. Most of the code
is written by AI coding agents (Claude Code and OpenCode) from written plans, reviewed by the
maintainer and held to the same checks and CI. The reasons for each decision:
[How we make the Local apps](https://github.com/Styx-Studios/.github/blob/main/profile/README.md#how-we-make-the-local-apps).

## License and credits

MIT or Apache-2.0, at your option ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
The vendored FilmCraft crates keep their authors' licenses and notices in `libs/filmcraft`.
Other Local apps: [the full list](https://github.com/Styx-Studios).
