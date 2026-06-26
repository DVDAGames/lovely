use crate::fsutil;
use crate::{LovelyError, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "lovely.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub game: GameConfig,
    pub paths: PathConfig,
    pub targets: TargetsConfig,
    pub itch: ItchConfig,
    pub steam: SteamConfig,
    pub compatibility: CompatibilityConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameConfig {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathConfig {
    pub source: PathBuf,
    pub output: PathBuf,
    pub icon: Option<PathBuf>,
    pub includes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetsConfig {
    pub web: WebTargetConfig,
    pub windows: DesktopTargetConfig,
    pub macos: DesktopTargetConfig,
    pub linux: DesktopTargetConfig,
    pub switch: SwitchTargetConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebTargetConfig {
    pub enabled: bool,
    pub variant: String,
    pub html_template: Option<PathBuf>,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopTargetConfig {
    pub enabled: bool,
    pub runtime_archive: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchTargetConfig {
    pub enabled: bool,
    pub mode: String,
    pub lovepotion_elf: Option<PathBuf>,
    pub lovepotion_nro: Option<PathBuf>,
    pub icon: Option<PathBuf>,
    pub title_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItchConfig {
    pub project: Option<String>,
    pub prerelease_channel: String,
    pub release_channel: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamConfig {
    pub app_id: Option<String>,
    pub windows_depot_id: Option<String>,
    pub macos_depot_id: Option<String>,
    pub linux_depot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityConfig {
    pub allow_warnings: Vec<String>,
}

impl Config {
    pub fn default_for_dir(dir: &Path) -> Self {
        let id = dir
            .file_name()
            .map(|name| slugify(&name.to_string_lossy()))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "my-love-game".to_string());

        let name = id
            .split('-')
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        Self {
            game: GameConfig {
                id,
                name,
                version: "0.1.0".to_string(),
                author: "Unknown".to_string(),
            },
            paths: PathConfig {
                source: PathBuf::from("."),
                output: PathBuf::from("dist"),
                icon: Some(PathBuf::from("assets/icon.png")),
                includes: vec!["**/*".to_string()],
            },
            targets: TargetsConfig::default(),
            itch: ItchConfig {
                project: None,
                prerelease_channel: "web-prerelease".to_string(),
                release_channel: "web".to_string(),
            },
            steam: SteamConfig {
                app_id: None,
                windows_depot_id: None,
                macos_depot_id: None,
                linux_depot_id: None,
            },
            compatibility: CompatibilityConfig {
                allow_warnings: Vec::new(),
            },
        }
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let text = fsutil::read_to_string(path)?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut table: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut section = String::new();

        for (index, raw_line) in text.lines().enumerate() {
            let line = strip_comment(raw_line).trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].trim().to_string();
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                return Err(LovelyError::Config(format!(
                    "line {} is not a key/value pair",
                    index + 1
                )));
            };
            table
                .entry(section.clone())
                .or_default()
                .insert(key.trim().to_string(), value.trim().to_string());
        }

        let mut config = Self::default_for_dir(Path::new("."));
        if let Some(game) = table.get("game") {
            config.game.id = get_string(game, "id").unwrap_or(config.game.id);
            config.game.name = get_string(game, "name").unwrap_or(config.game.name);
            config.game.version = get_string(game, "version").unwrap_or(config.game.version);
            config.game.author = get_string(game, "author").unwrap_or(config.game.author);
        }
        if let Some(paths) = table.get("paths") {
            if let Some(source) = get_string(paths, "source") {
                config.paths.source = PathBuf::from(source);
            }
            if let Some(output) = get_string(paths, "output") {
                config.paths.output = PathBuf::from(output);
            }
            config.paths.icon = get_optional_path(paths, "icon", config.paths.icon);
            if let Some(includes) = get_string_array(paths, "includes")? {
                config.paths.includes = includes;
            }
        }

        apply_web(&mut config, table.get("targets.web"))?;
        apply_desktop(&mut config.targets.windows, table.get("targets.windows"));
        apply_desktop(&mut config.targets.macos, table.get("targets.macos"));
        apply_desktop(&mut config.targets.linux, table.get("targets.linux"));
        apply_switch(&mut config.targets.switch, table.get("targets.switch"));

        if let Some(itch) = table.get("itch") {
            config.itch.project = get_optional_string(itch, "project", config.itch.project);
            config.itch.prerelease_channel =
                get_string(itch, "prerelease_channel").unwrap_or(config.itch.prerelease_channel);
            config.itch.release_channel =
                get_string(itch, "release_channel").unwrap_or(config.itch.release_channel);
        }
        if let Some(steam) = table.get("steam") {
            config.steam.app_id = get_optional_string(steam, "app_id", config.steam.app_id);
            config.steam.windows_depot_id =
                get_optional_string(steam, "windows_depot_id", config.steam.windows_depot_id);
            config.steam.macos_depot_id =
                get_optional_string(steam, "macos_depot_id", config.steam.macos_depot_id);
            config.steam.linux_depot_id =
                get_optional_string(steam, "linux_depot_id", config.steam.linux_depot_id);
        }
        if let Some(compatibility) = table.get("compatibility") {
            if let Some(warnings) = get_string_array(compatibility, "allow_warnings")? {
                config.compatibility.allow_warnings = warnings;
            }
        }

        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.game.id.trim().is_empty() {
            return Err(LovelyError::Config("game.id must not be empty".to_string()));
        }
        if self.game.name.trim().is_empty() {
            return Err(LovelyError::Config(
                "game.name must not be empty".to_string(),
            ));
        }
        if !matches!(
            self.targets.web.variant.as_str(),
            "web-compat" | "web-threaded"
        ) {
            return Err(LovelyError::Config(
                "targets.web.variant must be web-compat or web-threaded".to_string(),
            ));
        }
        if !matches!(self.targets.switch.mode.as_str(), "bundler" | "fuse") {
            return Err(LovelyError::Config(
                "targets.switch.mode must be bundler or fuse".to_string(),
            ));
        }
        Ok(())
    }

    pub fn to_toml(&self) -> String {
        format!(
            r#"[game]
id = "{id}"
name = "{name}"
version = "{version}"
author = "{author}"

[paths]
source = "{source}"
output = "{output}"
icon = {icon}
includes = ["**/*"]

[targets.web]
enabled = true
variant = "web-compat"
memory_bytes = 67108864
html_template = ""

[targets.windows]
enabled = true
runtime_archive = ""

[targets.macos]
enabled = true
runtime_archive = ""

[targets.linux]
enabled = true
runtime_archive = ""

[targets.switch]
enabled = true
mode = "bundler"
lovepotion_elf = ""
lovepotion_nro = ""
icon = ""
title_id = "0x010444C0DE000000"

[itch]
project = ""
prerelease_channel = "web-prerelease"
release_channel = "web"

[steam]
app_id = ""
windows_depot_id = ""
macos_depot_id = ""
linux_depot_id = ""

[compatibility]
allow_warnings = []
"#,
            id = escape(&self.game.id),
            name = escape(&self.game.name),
            version = escape(&self.game.version),
            author = escape(&self.game.author),
            source = escape(&fsutil::normalize_slashes(&self.paths.source)),
            output = escape(&fsutil::normalize_slashes(&self.paths.output)),
            icon = self
                .paths
                .icon
                .as_ref()
                .map(|path| format!("\"{}\"", escape(&fsutil::normalize_slashes(path))))
                .unwrap_or_else(|| "\"\"".to_string())
        )
    }
}

impl Default for TargetsConfig {
    fn default() -> Self {
        Self {
            web: WebTargetConfig {
                enabled: true,
                variant: "web-compat".to_string(),
                html_template: None,
                memory_bytes: 67_108_864,
            },
            windows: DesktopTargetConfig {
                enabled: true,
                runtime_archive: None,
            },
            macos: DesktopTargetConfig {
                enabled: true,
                runtime_archive: None,
            },
            linux: DesktopTargetConfig {
                enabled: true,
                runtime_archive: None,
            },
            switch: SwitchTargetConfig {
                enabled: true,
                mode: "bundler".to_string(),
                lovepotion_elf: None,
                lovepotion_nro: None,
                icon: None,
                title_id: "0x010444C0DE000000".to_string(),
            },
        }
    }
}

fn apply_web(config: &mut Config, values: Option<&BTreeMap<String, String>>) -> Result<()> {
    let Some(values) = values else {
        return Ok(());
    };
    config.targets.web.enabled = get_bool(values, "enabled").unwrap_or(config.targets.web.enabled);
    config.targets.web.variant =
        get_string(values, "variant").unwrap_or(config.targets.web.variant.clone());
    config.targets.web.html_template = get_optional_path(
        values,
        "html_template",
        config.targets.web.html_template.clone(),
    );
    if let Some(memory) = values.get("memory_bytes") {
        config.targets.web.memory_bytes = parse_integer(memory, "targets.web.memory_bytes")?;
    }
    Ok(())
}

fn apply_desktop(config: &mut DesktopTargetConfig, values: Option<&BTreeMap<String, String>>) {
    let Some(values) = values else {
        return;
    };
    config.enabled = get_bool(values, "enabled").unwrap_or(config.enabled);
    config.runtime_archive =
        get_optional_path(values, "runtime_archive", config.runtime_archive.clone());
}

fn apply_switch(config: &mut SwitchTargetConfig, values: Option<&BTreeMap<String, String>>) {
    let Some(values) = values else {
        return;
    };
    config.enabled = get_bool(values, "enabled").unwrap_or(config.enabled);
    config.mode = get_string(values, "mode").unwrap_or(config.mode.clone());
    config.lovepotion_elf =
        get_optional_path(values, "lovepotion_elf", config.lovepotion_elf.clone());
    config.lovepotion_nro =
        get_optional_path(values, "lovepotion_nro", config.lovepotion_nro.clone());
    config.icon = get_optional_path(values, "icon", config.icon.clone());
    config.title_id = get_string(values, "title_id").unwrap_or(config.title_id.clone());
}

fn get_string(values: &BTreeMap<String, String>, key: &str) -> Option<String> {
    let value = values.get(key)?;
    parse_string(value).filter(|value| !value.is_empty())
}

fn get_optional_string(
    values: &BTreeMap<String, String>,
    key: &str,
    previous: Option<String>,
) -> Option<String> {
    match values.get(key).and_then(|value| parse_string(value)) {
        Some(value) if value.is_empty() => None,
        Some(value) => Some(value),
        None => previous,
    }
}

fn get_optional_path(
    values: &BTreeMap<String, String>,
    key: &str,
    previous: Option<PathBuf>,
) -> Option<PathBuf> {
    match values.get(key).and_then(|value| parse_string(value)) {
        Some(value) if value.is_empty() => None,
        Some(value) => Some(PathBuf::from(value)),
        None => previous,
    }
}

fn get_bool(values: &BTreeMap<String, String>, key: &str) -> Option<bool> {
    match values.get(key)?.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn get_string_array(values: &BTreeMap<String, String>, key: &str) -> Result<Option<Vec<String>>> {
    let Some(value) = values.get(key) else {
        return Ok(None);
    };
    parse_string_array(value).map(Some)
}

fn parse_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        Some(value[1..value.len() - 1].replace("\\\"", "\""))
    } else {
        None
    }
}

fn parse_integer(value: &str, field: &str) -> Result<u64> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| LovelyError::Config(format!("{field} must be an integer")))
}

fn parse_string_array(value: &str) -> Result<Vec<String>> {
    let value = value.trim();
    if !(value.starts_with('[') && value.ends_with(']')) {
        return Err(LovelyError::Config(
            "expected an array of strings".to_string(),
        ));
    }
    let inner = value[1..value.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }

    inner
        .split(',')
        .map(|part| {
            parse_string(part.trim()).ok_or_else(|| {
                LovelyError::Config(format!("array item {part:?} is not a quoted string"))
            })
        })
        .collect()
}

fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    for (index, ch) in line.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..index],
            _ => {}
        }
    }
    line
}

fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in input.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn escape(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config() {
        let config = Config::parse(
            r#"[game]
id = "sailman"
name = "Sailman"
version = "1.2.3"
author = "Team"

[targets.web]
variant = "web-threaded"

[compatibility]
allow_warnings = ["switch.shader"]
"#,
        )
        .unwrap();

        assert_eq!(config.game.id, "sailman");
        assert_eq!(config.targets.web.variant, "web-threaded");
        assert_eq!(config.compatibility.allow_warnings, vec!["switch.shader"]);
    }
}
