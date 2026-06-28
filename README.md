# Lovely

A better way to publish LÖVE games.

Lovely is a GPL-3.0 Rust CLI for turning one LÖVE >= 11 source tree into web
and desktop/Steam distribution artifacts.

---

**Note:**: This is a work in progress and is 95% vibe coded to get the basics working and validate the idea. Once parity with existing [`love.js`](https://github.com/Davidobot/love.js) builds is achieved, I'll start focusing on patching existing bugs, pulling in outstanding Pull Requests and Issues, and identifying fixes that were added to forks of `love.js` that should be upstreamed into a canonical runtime.

---

This repository currently contains the first implementation slice:

- `lovely.toml` project configuration via `lovely init`.
- `lovely.lock` reproducibility metadata pinned to the `love-11-plus` runtime channel.
- Runtime registry commands for installing local pinned runtimes into
  `~/.cache/lovely`.
- Deterministic `.love`/ZIP/TAR packaging with normalized file ordering and timestamps.
- Target adapter seams for web, Windows, macOS, and Linux.
- Web packaging that consumes Lovely.js runtime bundles, including
  `lovely-game-loader.js`, `lovely-web-shims.js`, runtime JavaScript/WASM, and
  the bundle-owned default `index.html` template.
- Web launch argument configuration for demo/release variants such as
  `arguments = ["--demo-capture"]`.
- Itch.io publishing through Butler with explicit staging/release targets.
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

Install the CLI from crates.io with the `lovely-packager` package. The installed
binary is still named `lovely`:

```sh
cargo install lovely-packager --locked
```

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
lovely publish itch [staging|release]
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
`targets.web.arguments`. When no project template is configured, Lovely uses
the `html` template declared by the selected Lovely.js `lovely-runtime.json`.
Templates can use `__GAME_TITLE__`, `__WEB_ARGUMENTS__`, and `__WEB_MEMORY__`
placeholders. Use `html_assets` for files referenced by a custom template, such
as logos or fonts; files are copied beside `index.html`, and directories are
copied under their directory name:

```toml
[targets.web]
html_template = "src/templates/index.html"
html_assets = ["src/templates/logo.png", "src/templates/fonts"]
runtime_path = "runtimes/web"
arguments = ["--demo-capture"]
```

Lovely resolves the managed Lovely.js runtime bundle from `targets.web.variant`
when `runtime_path` is unset. Use `runtime_path` only to override that default
with a project-pinned bundle. Lovely copies runtime files into `dist/web` and
the upload ZIP during `lovely build web`, while rendering `index.html` from
either the project template or the bundle default. If `runtime_path` points at a
missing `lovely.js/dist/<variant>` checkout, Lovely restores the Lovely.js
repository, runs its build, and uses the generated bundle. Set `LOVELY_JS_PATH`
to use an existing local Lovely.js checkout, or `LOVELY_JS_REF` to pin the
restored checkout ref in CI.

Itch.io publishes use Butler. Set `[itch].project` to your Itch project
slug, then run `lovely publish itch staging` for `prerelease_channel` or
`lovely publish itch release` for `release_channel`. If Butler is not already
on `PATH`, Lovely downloads the official Butler archive into its cache first.

## Scope

Lovely targets LÖVE >= 11. Runtime-specific compatibility is handled through
pinned runtime artifacts and project diagnostics rather than assuming a single
engine generation.
