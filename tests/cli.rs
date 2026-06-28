use std::fs;
use std::path::Path;
use std::process::Command;

fn binary() -> String {
    env!("CARGO_BIN_EXE_lovely").to_string()
}

fn copy_fixture(to: &Path) {
    fs::create_dir_all(to).unwrap();
    fs::copy("tests/fixtures/minimal-game/main.lua", to.join("main.lua")).unwrap();
}

#[test]
fn init_creates_config_and_lock() {
    let root = tempfile_dir("init");
    copy_fixture(&root);

    let output = Command::new(binary())
        .arg("init")
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("lovely.toml").is_file());
    assert!(root.join("lovely.lock").is_file());
}

#[test]
fn builds_web_package() {
    let root = tempfile_dir("web");
    let lovely_js = fake_lovely_js_runtime(&root);
    copy_fixture(&root);
    assert!(
        Command::new(binary())
            .arg("init")
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let output = Command::new(binary())
        .args(["build", "web"])
        .env("LOVELY_JS_PATH", &lovely_js)
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("dist/web/game.love").is_file());
    assert!(root.join("dist/web/index.html").is_file());
    assert!(root.join("dist/web/lovely-web-shims.js").is_file());
    let manifest = fs::read_to_string(root.join("dist/web/lovely-runtime.txt")).unwrap();
    assert!(manifest.contains("runtime_channel=love-11-plus"));
    assert!(fs::read_dir(root.join("dist")).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with("-web.zip")
    }));
}

#[test]
fn web_build_renders_configured_arguments_into_template() {
    let root = tempfile_dir("web-arguments");
    let lovely_js = fake_lovely_js_runtime(&root);
    copy_fixture(&root);
    fs::create_dir_all(root.join("templates")).unwrap();
    fs::write(
        root.join("templates/index.html"),
        r#"<!doctype html>
<script>
var Module = {
  arguments: __WEB_ARGUMENTS__,
  INITIAL_MEMORY: __WEB_MEMORY__
};
</script>
"#,
    )
    .unwrap();

    assert!(
        Command::new(binary())
            .arg("init")
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let config = fs::read_to_string(root.join("lovely.toml")).unwrap();
    fs::write(
        root.join("lovely.toml"),
        config
            .replace(
                "html_template = \"\"",
                "html_template = \"templates/index.html\"",
            )
            .replace(
                "arguments = []",
                "arguments = [\"--demo-capture\", \"--seed=harbor\"]",
            ),
    )
    .unwrap();

    let output = Command::new(binary())
        .args(["build", "web"])
        .env("LOVELY_JS_PATH", &lovely_js)
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let index = fs::read_to_string(root.join("dist/web/index.html")).unwrap();
    assert!(index.contains(r#"arguments: ["./game.love", "--demo-capture", "--seed=harbor"]"#));
    assert!(index.contains("INITIAL_MEMORY: 67108864"));

    let manifest = fs::read_to_string(root.join("dist/web/lovely-runtime.txt")).unwrap();
    assert!(manifest.contains(r#"arguments=["./game.love", "--demo-capture", "--seed=harbor"]"#));
}

#[test]
fn web_build_copies_configured_html_assets() {
    let root = tempfile_dir("web-html-assets");
    let lovely_js = fake_lovely_js_runtime(&root);
    copy_fixture(&root);
    fs::create_dir_all(root.join("templates")).unwrap();
    fs::write(
        root.join("templates/index.html"),
        r#"<!doctype html>
<img src="logo.png" alt="Game logo">
<script>var Module = { arguments: __WEB_ARGUMENTS__ };</script>
"#,
    )
    .unwrap();
    fs::write(root.join("templates/logo.png"), b"logo-bytes").unwrap();

    assert!(
        Command::new(binary())
            .arg("init")
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let config = fs::read_to_string(root.join("lovely.toml")).unwrap();
    fs::write(
        root.join("lovely.toml"),
        config
            .replace(
                "html_template = \"\"",
                "html_template = \"templates/index.html\"",
            )
            .replace("html_assets = []", "html_assets = [\"templates/logo.png\"]"),
    )
    .unwrap();

    let output = Command::new(binary())
        .args(["doctor", "web"])
        .env("LOVELY_JS_PATH", &lovely_js)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new(binary())
        .args(["build", "web"])
        .env("LOVELY_JS_PATH", &lovely_js)
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(root.join("dist/web/logo.png")).unwrap(),
        b"logo-bytes"
    );
    let zip = fs::read(find_web_zip(&root)).unwrap();
    assert!(contains_bytes(&zip, b"logo.png"));
    assert!(contains_bytes(&zip, b"logo-bytes"));
}

#[test]
fn web_build_uses_configured_runtime_path_without_cache_setup() {
    let root = tempfile_dir("web-runtime-path");
    copy_fixture(&root);
    write_fake_web_runtime(&root.join("runtimes/web"));

    assert!(
        Command::new(binary())
            .arg("init")
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let config = fs::read_to_string(root.join("lovely.toml")).unwrap();
    fs::write(
        root.join("lovely.toml"),
        config.replace("runtime_path = \"\"", "runtime_path = \"runtimes/web\""),
    )
    .unwrap();

    let output = Command::new(binary())
        .args(["doctor", "web"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("runtime.missing"));
    assert!(!stdout.contains("lock.unresolved"));

    let output = Command::new(binary())
        .args(["build", "web"])
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("dist/web/love.js").is_file());
    assert!(root.join("dist/web/love.wasm").is_file());
    assert!(root.join("dist/web/lovely-game-loader.js").is_file());
    assert!(root.join("dist/web/lovely-web-shims.js").is_file());
    assert!(root.join("dist/web/lovely-runtime.json").is_file());

    let index = fs::read_to_string(root.join("dist/web/index.html")).unwrap();
    assert!(index.contains("<title>Lovely Test Web Runtime Path"));
    assert!(index.contains("args=[\"./game.love\"]"));
    assert!(index.contains("memory=67108864"));
    assert!(!index.contains("__GAME_TITLE__"));
    assert!(!index.contains("__WEB_ARGUMENTS__"));
    assert!(!index.contains("__WEB_MEMORY__"));

    let zip = fs::read(find_web_zip(&root)).unwrap();
    assert!(contains_bytes(&zip, b"lovely-game-loader.js"));
    assert!(contains_bytes(&zip, b"lovely-web-shims.js"));
    assert!(contains_bytes(&zip, b"lovely-runtime.json"));
}

#[test]
fn web_build_restores_missing_lovely_js_runtime_path_from_override() {
    let root = tempfile_dir("web-managed-lovely-js");
    copy_fixture(&root);
    let lovely_js = root.join("fake-lovely-js");
    write_fake_web_runtime(&lovely_js.join("dist/web-compat"));

    assert!(
        Command::new(binary())
            .arg("init")
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let config = fs::read_to_string(root.join("lovely.toml")).unwrap();
    fs::write(
        root.join("lovely.toml"),
        config.replace(
            "runtime_path = \"\"",
            "runtime_path = \"../_utils/lovely.js/dist/web-compat\"",
        ),
    )
    .unwrap();

    let output = Command::new(binary())
        .args(["doctor", "web"])
        .env("LOVELY_JS_PATH", &lovely_js)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("runtime.restorable"));

    let output = Command::new(binary())
        .args(["build", "web"])
        .env("LOVELY_JS_PATH", &lovely_js)
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("dist/web/love.js").is_file());
    assert!(root.join("dist/web/love.wasm").is_file());
    assert!(root.join("dist/web/lovely-game-loader.js").is_file());
    assert!(root.join("dist/web/lovely-web-shims.js").is_file());
}

#[test]
fn runtime_fetch_installs_local_runtime_and_web_build_consumes_it() {
    let root = tempfile_dir("runtime-web");
    let cache = tempfile_dir("runtime-cache");
    copy_fixture(&root);
    let runtime_dir = root.join("fake-web-runtime");
    write_fake_web_runtime(&runtime_dir);

    assert!(
        Command::new(binary())
            .arg("init")
            .env("LOVELY_CACHE_DIR", &cache)
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let output = Command::new(binary())
        .args(["runtime", "fetch", "web", "fake-web-runtime"])
        .env("LOVELY_CACHE_DIR", &cache)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new(binary())
        .args(["runtime", "doctor", "web"])
        .env("LOVELY_CACHE_DIR", &cache)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new(binary())
        .args(["build", "web"])
        .env("LOVELY_CACHE_DIR", &cache)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("dist/web/love.js").is_file());
    assert!(root.join("dist/web/love.wasm").is_file());
    assert!(root.join("dist/web/lovely-game-loader.js").is_file());
    assert!(root.join("dist/web/lovely-web-shims.js").is_file());

    let index = fs::read_to_string(root.join("dist/web/index.html")).unwrap();
    assert!(index.contains("<title>Lovely Test Runtime Web"));
    assert!(index.contains("args=[\"./game.love\"]"));
}

#[test]
fn runtime_fetch_rejects_checksum_mismatch() {
    let root = tempfile_dir("runtime-checksum");
    let cache = tempfile_dir("runtime-checksum-cache");
    fs::write(root.join("runtime.bin"), b"runtime").unwrap();

    let output = Command::new(binary())
        .args([
            "runtime",
            "fetch",
            "web",
            "runtime.bin",
            "--sha256",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ])
        .env("LOVELY_CACHE_DIR", &cache)
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("checksum mismatch"));
}

#[test]
fn publish_itch_invokes_butler_with_explicit_targets() {
    let root = tempfile_dir("publish-itch");
    let lovely_js = fake_lovely_js_runtime(&root);
    let cache = tempfile_dir("publish-itch-cache");
    let butler_dir = tempfile_dir("publish-itch-butler-bin");
    let butler_log = root.join("butler.log");
    copy_fixture(&root);
    write_fake_butler(&butler_dir);

    assert!(
        Command::new(binary())
            .arg("init")
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let config = fs::read_to_string(root.join("lovely.toml")).unwrap();
    fs::write(
        root.join("lovely.toml"),
        config.replace("project = \"\"", "project = \"dvd/sailman\""),
    )
    .unwrap();

    let output = Command::new(binary())
        .args(["build", "web"])
        .env("LOVELY_JS_PATH", &lovely_js)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let path = prepend_to_path(&butler_dir);
    let output = Command::new(binary())
        .args(["publish", "itch", "staging"])
        .env("PATH", &path)
        .env("LOVELY_CACHE_DIR", &cache)
        .env("LOVELY_FAKE_BUTLER_LOG", &butler_log)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let staging = fs::read_to_string(&butler_log).unwrap();
    assert!(staging.contains("push\n"));
    assert!(staging.contains("-web.zip\n"));
    assert!(staging.contains("dvd/sailman:web-prerelease\n"));

    let output = Command::new(binary())
        .args(["publish", "itch", "release"])
        .env("PATH", &path)
        .env("LOVELY_CACHE_DIR", &cache)
        .env("LOVELY_FAKE_BUTLER_LOG", &butler_log)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let release = fs::read_to_string(&butler_log).unwrap();
    assert!(release.contains("push\n"));
    assert!(release.contains("-web.zip\n"));
    assert!(release.contains("dvd/sailman:web\n"));
}

#[test]
fn publish_itch_rejects_unknown_target() {
    let root = tempfile_dir("publish-itch-unknown");
    copy_fixture(&root);
    assert!(
        Command::new(binary())
            .arg("init")
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let output = Command::new(binary())
        .args(["publish", "itch", "nightly"])
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("expected staging or release"));
}

#[test]
fn build_respects_included_paths() {
    let root = tempfile_dir("included-paths");
    let lovely_js = fake_lovely_js_runtime(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
    fs::write(root.join("main.lua"), "-- main-marker\n").unwrap();
    fs::write(root.join("conf.lua"), "-- conf-marker\n").unwrap();
    fs::write(root.join("src/game.lua"), "-- src-marker\n").unwrap();
    fs::write(root.join("src/dev-tool.lua"), "-- excluded-src-marker\n").unwrap();
    fs::write(root.join("assets/sprite.txt"), "asset-marker\n").unwrap();
    fs::write(
        root.join("node_modules/pkg/tool.lua"),
        "-- excluded-marker\n",
    )
    .unwrap();

    assert!(
        Command::new(binary())
            .arg("init")
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let config = fs::read_to_string(root.join("lovely.toml")).unwrap();
    fs::write(
        root.join("lovely.toml"),
        config
            .replace(
                "includes = [\"**/*\"]",
                "includes = [\"main.lua\", \"conf.lua\", \"src/**\", \"assets/**\"]",
            )
            .replace(
                "excludes = [\"node_modules/**\"]",
                "excludes = [\"node_modules/**\", \"src/dev-tool.lua\"]",
            ),
    )
    .unwrap();

    let output = Command::new(binary())
        .args(["build", "web"])
        .env("LOVELY_JS_PATH", &lovely_js)
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let love = fs::read(root.join("dist/web/game.love")).unwrap();
    assert!(contains_bytes(&love, b"main-marker"));
    assert!(contains_bytes(&love, b"src-marker"));
    assert!(contains_bytes(&love, b"asset-marker"));
    assert!(!contains_bytes(&love, b"excluded-marker"));
    assert!(!contains_bytes(&love, b"excluded-src-marker"));
}

#[test]
fn check_finds_web_ffi() {
    let root = tempfile_dir("check");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("main.lua"), "local ffi = require('ffi')\n").unwrap();
    assert!(
        Command::new(binary())
            .arg("init")
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let output = Command::new(binary())
        .args(["check", "web"])
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("web.ffi") || stderr.contains("web.ffi"));
}

#[test]
fn check_warns_about_lovejs_porting_hazards() {
    let root = tempfile_dir("check-porting-hazards");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("main.lua"),
        r#"
local bit = require('bit')
local values = unpack(items)
love.audio.play(sound)
local shader = love.graphics.newShader("varying vec2 v; void effect() {}")
"#,
    )
    .unwrap();
    assert!(
        Command::new(binary())
            .arg("init")
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let output = Command::new(binary())
        .args(["check", "web"])
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("web.bit_module"));
    assert!(stdout.contains("web.lua52_unpack"));
    assert!(stdout.contains("web.module_audio"));
    assert!(stdout.contains("web.shader_varying"));
}

#[test]
fn check_respects_included_paths() {
    let root = tempfile_dir("check-included-paths");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("src/dev")).unwrap();
    fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
    fs::write(root.join("main.lua"), "require('src.game')\n").unwrap();
    fs::write(root.join("src/game.lua"), "return {}\n").unwrap();
    fs::write(
        root.join("src/dev/native.lua"),
        "local ffi = require('ffi')\n",
    )
    .unwrap();
    fs::write(
        root.join("node_modules/pkg/native.lua"),
        "local ffi = require('ffi')\n",
    )
    .unwrap();

    assert!(
        Command::new(binary())
            .arg("init")
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );

    let config = fs::read_to_string(root.join("lovely.toml")).unwrap();
    fs::write(
        root.join("lovely.toml"),
        config
            .replace(
                "includes = [\"**/*\"]",
                "includes = [\"main.lua\", \"src/**\"]",
            )
            .replace(
                "excludes = [\"node_modules/**\"]",
                "excludes = [\"node_modules/**\", \"src/dev/**\"]",
            ),
    )
    .unwrap();

    let output = Command::new(binary())
        .args(["check", "web"])
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_fake_web_runtime(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::write(
        path.join("index.html"),
        r#"<!doctype html>
<html>
<head><title>__GAME_TITLE__</title></head>
<body>
<script src="lovely-web-shims.js"></script>
<script>const args=__WEB_ARGUMENTS__; const memory=__WEB_MEMORY__;</script>
<script src="lovely-game-loader.js"></script>
<script async src="love.js"></script>
</body>
</html>
"#,
    )
    .unwrap();
    fs::write(
        path.join("lovely-game-loader.js"),
        "console.log('loader');\n",
    )
    .unwrap();
    fs::write(path.join("lovely-web-shims.js"), "console.log('shims');\n").unwrap();
    fs::write(path.join("love.js"), "console.log('love runtime');\n").unwrap();
    fs::write(path.join("love.wasm"), b"wasm").unwrap();
    fs::write(
        path.join("lovely-runtime.json"),
        r#"{
  "schema": 1,
  "target": "web",
  "variant": "web-compat",
  "channel": "love-11-plus",
  "loveVersion": "11.5",
  "emscriptenVersion": "2.0.0",
  "html": "index.html",
  "loader": "lovely-game-loader.js",
  "shims": "lovely-web-shims.js",
  "entrypoint": "love.js",
  "wasm": "love.wasm",
  "worker": null,
  "files": []
}
"#,
    )
    .unwrap();
}

fn fake_lovely_js_runtime(root: &Path) -> std::path::PathBuf {
    let lovely_js = root.join("fake-lovely-js");
    write_fake_web_runtime(&lovely_js.join("dist/web-compat"));
    lovely_js
}

fn find_web_zip(root: &Path) -> std::path::PathBuf {
    fs::read_dir(root.join("dist"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("-web.zip")
        })
        .unwrap()
}

fn tempfile_dir(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "lovely-test-{name}-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn write_fake_butler(path: &Path) {
    fs::create_dir_all(path).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = path.join("butler");
        fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$LOVELY_FAKE_BUTLER_LOG\"\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
    }

    #[cfg(windows)]
    {
        fs::write(
            path.join("butler.cmd"),
            "@echo off\r\n(for %%A in (%*) do echo %%~A) > \"%LOVELY_FAKE_BUTLER_LOG%\"\r\n",
        )
        .unwrap();
    }
}

fn prepend_to_path(path: &Path) -> std::ffi::OsString {
    let mut paths = vec![path.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).unwrap()
}
