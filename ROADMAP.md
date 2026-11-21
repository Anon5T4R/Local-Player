# ROADMAP - Local Player

O que já foi entregue mora nas release notes; aqui ficam as pendências, a dívida
e as decisões que governam o app.

## Fases em aberto

- **7b. hwdec macOS** — falta VideoToolbox; o Linux (VA-API/NVDEC) e o Windows
  (Media Foundation/DXVA) já estão no `crates/hwdec` do fork (`hwdec/src/lib.rs`).
- **8. Maturação restante** — VFR e extração do motor como crate.

## Pendências conhecidas

- **hwdec 4K HEVC** — MF/DXVA cobre Main/Main 10 e VP9/AV1; falta validar 4K HEVC real em
  hardware (o SW pode não segurar sozinho, e o readback GPU→CPU cresce com a resolução).
- **Gamut BT.2020→709** — o tonemap HDR→SDR (BT.2390, 10 bits até a GPU) está feito;
  a conversão de gamut segue pendente.
- **VFR** — o pipeline assume a taxa nominal declarada pelo container.
- **ASS** — básico (styles/cores/\an/contorno); `\pos`/`\move`/`\fad`/karaokê/`\t`/desenho
  ignorados (avisos no stderr); sem colisão de linhas nem quebra automática.
- **Hot-swap de device de áudio** — trocar a saída exige reabrir faixa/app.
- **Gapless** — auto-avanço da playlist funciona, mas sem gapless fora de MP3.
- **Legenda embutida** — suporta texto (SRT/VTT/ASS do MKV e tx3g do MP4). Faixas bitmap
  (PGS/VobSub) são listadas no `--probe` e ignoradas no player; `wvtt` em MP4 (amostras em caixas)
  ainda não é interpretado.
- **Janela 1:1 e chrome dinâmico** — o resize mede o chrome na abertura e recalcula em até 3
  passadas (a janela inicial é estreita e as barras quebram linha, o chrome muda depois do
  resize). Se o chrome mudar DEPOIS (linha de status da legenda, item novo na lista), o vídeo fica
  com uma borda de poucos px até a próxima faixa — perseguir o chrome por frame faria a janela
  "respirar" sozinha, então não.
- **TLS do update** — provider puro (fork vendored do `rustls-rustcrypto`) como caminho único,
  sem fallback; `ring`/`aws-lc-sys` fora do grafo; 2 advisories ignoradas no `deny.toml`
  (`RUSTSEC-2023-0071` do `rsa`, sem patch nem no 0.10-rc; `RUSTSEC-2026-0387` do
  `crypto-bigint 0.5`, que só sai migrando para rsa 0.10), com o motivo lá. Revisitar quando o
  upstream do `rustls-rustcrypto` migrar de `rsa`.

## Dívida técnica

- Sync A/V e seek GOP-aware são o trabalho duro; os codecs em si não.
- Perf SW 4K HEVC pode exigir hwdec antes do esperado.

## Decisões

- **Sem fallback mpv** (João, 2026-11-07): o caminho é pure Rust; não existe válvula
  de escape para o mpv.
- **Sem novos PRs ao filmcraft** (João, 2026-11-09): os PRs #4494 e #4534 foram
  fechados sem merge; os patches ficam registrados em `docs/upstream/`.
