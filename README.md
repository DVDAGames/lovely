# Lovely

A better way to publish LÖVE 12 games.

Lovely is a GPL-3.0 Rust CLI for turning one LÖVE source tree into web,
desktop/Steam, and Nintendo Switch homebrew distribution artifacts.

This repository currently contains the first implementation slice:

- `lovely.toml` project configuration via `lovely init`.
- `lovely.lock` reproducibility metadata pinned to the `12-preview` runtime channel.
- Runtime registry commands for installing local pinned runtimes into
  `~/.cache/lovely`.
- Deterministic `.love`/ZIP/TAR packaging with normalized file ordering and timestamps.
- Target adapter seams for web, Windows, macOS, Linux, and Switch.
- Switch `bundler` mode that creates NACP metadata with `nacptool`, creates a
  metadata-bearing NRO with `elf2nro`, then fuses `game.love`.
- Compatibility diagnostics for web-native-module hazards and LÖVE Potion caveats.
- GitHub Actions workflow generation with `lovely ci github`.

The runtime-heavy work is intentionally separated from ordinary game builds.
Lovely is not intended to host its own runtime distribution service. It should
resolve official upstream or vendor-provided runtimes, cache them locally, and
verify their checksums. For now, `lovely runtime fetch` installs local runtime
files or directories into the same cache layout a future upstream URL resolver
will use. Web builds copy cached JavaScript/WASM runtime files into `dist/web`;
desktop builds include cached runtime content in their artifacts and Steam depot
directories. Switch release builds require a real LÖVE Potion `.elf` configured
in `targets.switch.lovepotion_elf` or cached with
`lovely runtime fetch switch <path>`, plus devkitPro `switch-tools` on `PATH`.
The older direct `.nro` concatenation path remains available as
`targets.switch.mode = "fuse"` for local development, but it preserves base LÖVE
Potion metadata instead of applying custom title/icon metadata.

## Commands

```sh
lovely init
lovely lock
lovely doctor [target]
lovely check [target...]
lovely runtime fetch <target> <local-path> [--channel 12-preview] [--sha256 <hex>]
lovely runtime doctor [target|all]
lovely runtime list
lovely runtime cache-dir
lovely build [web|windows|macos|linux|switch|desktop|all]
lovely publish itch
lovely ci github
```

## Scope

Lovely targets LÖVE 12 only. LÖVE 11.5 tooling remains outside this project.
Nintendo Switch support means homebrew through LÖVE Potion; NSP, eShop,
Nintendo SDK, and licensed commercial publishing support are explicitly out of
scope.
