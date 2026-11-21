# Contribuições para o upstream `storytold/filmcraft`

Patches do fork (`libs/filmcraft`) prontos para PR, gerados contra o snapshot
`8fda2f2`. O detalhamento (método da comparação, caso de uso, texto de PR em
inglês) está no `fork-patches.md`.

| Arquivo | O que é |
| --- | --- |
| `0001-gop-host-frame-cache-budget.patch` | teto do cache de frames configurável pelo host (`crates/codecs/src/gop.rs`), com teste |
| `0002-mp4-leading-empty-edit.patch` | ignora o empty edit inicial que um corte por stream-copy deixa no `elst` (`crates/codecs/src/mp4.rs`), com teste |

O `Cargo.toml` do vendor (workspace recortado, deps internas pinadas) é
empacotamento deste repo e **não** deve ser submetido — ver divergência 2 do
`fork-patches.md`.

**Estado:** os dois patches viraram PR no upstream (#4494 e #4534) e foram
fechados sem merge em 2026-11-09. Decisão do João na mesma data: **sem novos
PRs ao filmcraft** — o que o fork carrega fica documentado no `fork-patches.md`.

Notas de aplicação (se um dia voltarem a valer):

- Os patches foram gerados contra o blob do `8fda2f2` e validados com
  `git apply --check`. Se o upstream já tiver avançado nos arquivos, rebase
  manual nos hunks.
- Checkout Windows com `core.autocrlf=true` recusa os patches por CRLF/LF:
  aplique em WSL, num checkout com `core.autocrlf=false` ou no Linux. O
  conteúdo dos patches é LF, como manda o repo deles.
