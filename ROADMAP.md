# ROADMAP - Local Player

What has been delivered lives in the release notes; here stay the open items, the debt and the
decisions that govern the app.

## Open phases

- **7b. macOS hwdec** — VideoToolbox is missing; Linux (VA-API/NVDEC) and Windows
  (Media Foundation/DXVA) are already in the fork's `crates/hwdec` (`hwdec/src/lib.rs`).
- **8. Remaining maturation** — VFR and extracting the engine as a crate.

## Known open items

- **4K HEVC hwdec** — MF/DXVA covers Main/Main 10 and VP9/AV1; real 4K HEVC on
  hardware is still to be validated (SW may not hold up alone, and the GPU→CPU readback grows
  with the resolution).
- **BT.2020→709 gamut** — the HDR→SDR tonemap (BT.2390, 10 bits up to the GPU) is done;
  the gamut conversion is still pending.
- **VFR** — the pipeline assumes the nominal rate the container declares.
- **ASS** — basic (styles/colors/\an/outline); `\pos`/`\move`/`\fad`/karaoke/`\t`/drawings
  ignored (warnings on stderr); no line collision and no automatic wrapping.
- **Audio device hot-swap** — changing the output requires reopening the track/app.
- **Gapless** — the playlist's auto-advance works, but without gapless outside MP3.
- **Embedded subtitles** — text is supported (SRT/VTT/ASS from MKV and tx3g from MP4). Bitmap
  tracks (PGS/VobSub) are listed by `--probe` and ignored by the player; `wvtt` in MP4 (samples
  in boxes) is not interpreted yet.
- **1:1 window and dynamic chrome** — the resize measures the chrome on opening and recomputes in
  up to 3 passes (the initial window is narrow and the bars wrap, the chrome changes after the
  resize). If the chrome changes LATER (the subtitle status line, a new item in the list), the
  video keeps a border of a few px until the next track — chasing the chrome per frame would make
  the window "breathe" on its own, so no.
- **Update TLS** — pure provider (vendored fork of `rustls-rustcrypto`) as the only path,
  with no fallback; `ring`/`aws-lc-sys` out of the graph; 2 advisories ignored in `deny.toml`
  (`RUSTSEC-2023-0071` in `rsa`, no patch even in 0.10-rc; `RUSTSEC-2026-0387` in
  `crypto-bigint 0.5`, which only goes away by migrating to rsa 0.10), with the reason there.
  Revisit when the `rustls-rustcrypto` upstream migrates off `rsa`.

## Technical debt

- A/V sync and GOP-aware seek are the hard work; the codecs themselves are not.
- 4K HEVC SW perf may require hwdec sooner than expected.

## Decisions

- **No mpv fallback** (João, 2026-11-07): the path is pure Rust; there is no escape
  valve to mpv.
- **No new PRs to filmcraft** (João, 2026-11-09): PRs #4494 and #4534 were
  closed without merging; the patches stay recorded in `docs/upstream/`.
