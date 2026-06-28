# Lovely

A better way to publish LÖVE games.

Lovely is a GPL-3.0 Rust CLI for turning one LÖVE >= 11 source tree into web
and desktop/Steam distribution artifacts.

This repository currently contains the first implementation slice:

- `lovely.toml` project configuration via `lovely init`.
- `lovely.lock` reproducibility metadata pinned to the `love-11-plus` runtime channel.
- Runtime registry commands for installing local pinned runtimes into
  `~/.cache/lovely`.
- Deterministic `.love`/ZIP/TAR packaging with normalized file ordering and timestamps.
- Target adapter seams for web, Windows, macOS, and Linux.
- Web packaging that emits a small `lovely-web-shims.js` browser compatibility
  layer for responsive canvas, fullscreen, and mobile text-input hooks.
- Web launch argument configuration for demo/release variants such as
  `arguments = ["--demo-capture"]`.
- Compatibility diagnostics for web-native-module hazards and known love.js
  porting pitfalls.
- GitHub Actions workflow generation with `lovely ci github`.

The runtime-heavy work is intentionally separated from ordinary game builds.
Lovely is not intended to host its own runtime distribution service. It should
resolve official upstream or vendor-provided runtimes, cache them locally, and
verify their checksums. For now, `lovely runtime fetch` installs local runtime
files or directories into the same cache layout a future upstream URL resolver
will use. Web builds copy cached JavaScript/WASM runtime files into `dist/web`;
desktop builds include cached runtime content in their artifacts and Steam depot
directories.

See [docs/web-runtime.md](docs/web-runtime.md) for the love.js fork notes and
the runtime patch checklist Lovely is tracking.

## Commands

```sh
lovely init
lovely lock
lovely doctor [target]
lovely check [target...]
lovely runtime fetch <target> <local-path> [--channel love-11-plus] [--sha256 <hex>]
lovely runtime doctor [target|all]
lovely runtime list
lovely runtime cache-dir
lovely build [web|windows|macos|linux|desktop|all]
lovely publish itch
lovely ci github
```

JavaScript and TypeScript runtime/tooling work uses ESLint flat config with
`eslint-config-love`:

```sh
npm run lint:js
npm run typecheck:js
npm run check:js
```

Web builds prepend `./game.love` for love.js and append any configured
`targets.web.arguments`. Custom HTML templates can use `__WEB_ARGUMENTS__` and
`__WEB_MEMORY__` placeholders:

```toml
[targets.web]
html_template = "src/templates/index.html"
runtime_path = "runtimes/web"
arguments = ["--demo-capture"]
```

Use `runtime_path` for project-pinned web runtime artifacts, including patched
love.js forks. Lovely copies the runtime files into `dist/web` and the upload
ZIP during `lovely build web`, so game repositories do not need a separate
machine-local runtime cache step.

## Scope

Lovely targets LÖVE >= 11. Runtime-specific compatibility is handled through
pinned runtime artifacts and project diagnostics rather than assuming a single
engine generation.
