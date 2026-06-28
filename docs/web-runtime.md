# Lovely Web Runtime Notes

Lovely targets LÖVE >= 11 web builds. The current strategy is to keep the CLI
runtime-agnostic while collecting proven love.js fixes into a managed web
runtime profile.

## Findings from love.js forks

### pkhead/love.js `lua52` porting notes

Useful compatibility patches:

- Prefer `table.unpack` over `unpack` for Lua 5.2+.
- Provide a `bit` compatibility alias backed by `bit32` when available.
- Choose a supported canvas format before calling `love.graphics.newCanvas`.
- Hoist user-defined vertex shader `varying` declarations before generated
  shader code for WebGL implementations that reject late declarations.
- Avoid old `love.audio.play/stop/pause` module functions on affected
  Emscripten builds; prefer `Source:play/stop/pause`.

Lovely-side implications:

- Add future diagnostics for `unpack(`, `require("bit")`, module-level audio
  calls, and shader code with custom varyings.
- Consider an optional Lua web shim injected into `.love` archives only when a
  project opts in.

### dev10/wasm-love.js commits

Linked commits grouped by runtime concern:

- `c85e151`: Emscripten 2.0.34 support, mobile keyboard hooks, `Module._malloc`
  instead of deprecated `Module.getMemory`, IDBFS safety patches, generated
  worker/runtime refresh.
- `8e98713`: partial canvas resize support through CSS sizing, resize events,
  `Module.setCanvasSize` when available, and documented limitations.
- `b92f5e1`: fullscreen API corrections plus mobile CSS fullscreen fallback.
- `c76c5f5`: responsive mobile layout refinements, loading canvas sizing, and
  better fullscreen styling.

Lovely-side implications:

- Keep the generated shell responsive and mobile-aware.
- Emit `lovely-web-shims.js` beside web builds so cached runtimes and custom
  templates can share a small browser compatibility layer.
- Treat mobile keyboard support as two phase: runtime/native hook calls a global
  JavaScript function; JavaScript focuses a hidden input during the next touch
  gesture so mobile browsers allow the keyboard.
- Do not blindly vendor generated `.wasm` or generated Emscripten JavaScript
  from forks. Track the source-level fixes separately from generated artifacts.
- If Lovely eventually owns a runtime fork, patch IDBFS at build time or disable
  it by configuration rather than manually editing generated output.

## Current Lovely implementation

Web builds now emit:

- `index.html`
- `lovely-web-shims.js`
- `game.love`
- `lovely-runtime.txt`

`targets.web.arguments` configures game launch arguments for love.js builds.
Lovely writes `Module.arguments` as `["./game.love", ...arguments]`, so a demo
web deployment can set `arguments = ["--demo-capture"]`. Custom web templates
can use the `__WEB_ARGUMENTS__` placeholder to receive that JavaScript array and
`__WEB_MEMORY__` to receive the configured memory size.

For patched or project-pinned web runtimes, set `targets.web.runtime_path` to a
project-relative directory or file. This lets `lovely build web` include the
runtime directly from the game repository or restored release artifact without a
separate `lovely runtime fetch` setup step. The global runtime cache remains
useful for shared machine-local tooling, but release-oriented game repos should
prefer explicit runtime pins.

The shim currently provides:

- responsive canvas resize helper,
- desktop fullscreen helper,
- mobile CSS fullscreen fallback,
- mobile text-input hooks via `SDL_StartTextInput` and `SDL_StopTextInput`,
- hidden-input forwarding for simple text entry.

## Runtime Fork Checklist

When Lovely starts building or distributing a managed love.js-derived runtime,
fold in these items in order:

1. Source-level Emscripten compatibility:
   - `Module.getMemory` to `Module._malloc`
   - CMake policy/version updates needed by the selected Emscripten release
2. Browser shell improvements:
   - responsive canvas sizing
   - fullscreen API fallback matrix
   - mobile CSS fullscreen fallback
   - mobile keyboard bridge
3. Filesystem policy:
   - decide whether IDBFS is enabled by default,
   - if enabled, include safety checks for undefined file contents,
   - expose persistence as an explicit Lovely config option.
4. Lua compatibility shims:
   - `bit`/`bit32`
   - canvas pixel format fallback
   - shader varying hoist
   - Lua 5.1/5.2 library differences
5. Test matrix:
   - desktop Chrome/Firefox/Safari,
   - iOS Safari,
   - Android Chrome,
   - Itch iframe/embedded page,
   - threaded builds with cross-origin isolation headers.
