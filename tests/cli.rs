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
    assert!(fs::read_dir(root.join("dist")).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with("-web.zip")
    }));
}

#[test]
fn runtime_fetch_installs_local_runtime_and_web_build_consumes_it() {
    let root = tempfile_dir("runtime-web");
    let cache = tempfile_dir("runtime-cache");
    copy_fixture(&root);
    let runtime_dir = root.join("fake-web-runtime");
    fs::create_dir_all(&runtime_dir).unwrap();
    fs::write(runtime_dir.join("love.js"), "console.log('love 12');\n").unwrap();
    fs::write(runtime_dir.join("love.wasm"), b"wasm").unwrap();

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

#[cfg(unix)]
#[test]
fn builds_switch_with_bundler_mode_and_fake_devkit_tools() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile_dir("switch");
    copy_fixture(&root);
    fs::create_dir_all(root.join("runtimes/switch")).unwrap();
    fs::create_dir_all(root.join("assets/brand")).unwrap();
    fs::write(root.join("runtimes/switch/lovepotion.elf"), b"ELF").unwrap();
    fs::write(root.join("assets/brand/switch-icon.jpg"), b"JPEG").unwrap();

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
                "lovepotion_elf = \"\"",
                "lovepotion_elf = \"runtimes/switch/lovepotion.elf\"",
            )
            .replace("icon = \"\"", "icon = \"assets/brand/switch-icon.jpg\""),
    )
    .unwrap();

    let tools = root.join("fake-tools");
    fs::create_dir_all(&tools).unwrap();
    write_executable(
        &tools.join("nacptool"),
        "#!/bin/sh\nfor last do true; done\nprintf NACP > \"$last\"\n",
    );
    write_executable(&tools.join("elf2nro"), "#!/bin/sh\ncp \"$1\" \"$2\"\n");

    let path = format!(
        "{}:{}",
        tools.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(binary())
        .args(["build", "switch"])
        .env("PATH", path)
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let final_nro = fs::read_dir(root.join("dist/switch"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("nro")
                && !path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains("-base")
        })
        .unwrap();
    let nro = fs::read(final_nro).unwrap();
    assert!(nro.starts_with(b"ELF"));
    assert!(nro.windows(4).any(|window| window == b"PK\x03\x04"));

    fn write_executable(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
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
