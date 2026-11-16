use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::colors::{self, Palette};
use crate::log;
use crate::term::CursorShape;

pub const TEMPLATE: &str = r##"# Aeternus configuration
# Every option below is commented out and shows its default value.
# Uncomment and edit what you want to change. The file is reloaded automatically.

[font]
# family = "monospace"  # any installed font; "monospace" uses the system default
# size = 15.0           # logical pixels
# line_height = 1.25    # multiplier of the font size
# text_composition_strategy = "platform"  # like kitty: "platform", "legacy" or "<gamma> <contrast>", e.g. "1.7 30"

[window]
# width = 960           # logical pixels, only applied at startup
# height = 560          # only applied at startup
# padding = 8           # inner margin in logical pixels
# toolbar = false       # menu bar at the top: File, Edit, View, Help
# opacity = 1.0         # background opacity, 0.0 (transparent) to 1.0 (opaque)

[terminal]
# shell = ""            # empty = $SHELL; accepts arguments and quotes, e.g. "bash -l"
# scrollback = 10000    # lines of history
# working_directory = ""  # empty = current directory
# osc52 = true          # let programs write to the clipboard through OSC 52
# smart_copy_paste = false  # Ctrl+C copies when there is a selection, Ctrl+V pastes

[cursor]
# shape = "block"       # block, underline or bar
# blink = true
# trail = 0             # cursor trail: ms the cursor must stay still before the next move animates (0 = off)
# trail_decay = [0.1, 0.4]  # fastest and slowest trail decay, in seconds
# trail_start_threshold = 2 # minimum distance in cells for the trail to start
# trail_color = ""      # empty = cursor color

[bell]
# visual = true         # flash the window on BEL
# urgent = true         # request attention when the window is not focused

[colors]
# Same names as kitty. Two ways to set them, and they can be combined:
# 1. theme: a kitty color theme (.conf), e.g. one from kitty-themes or kitty's current-theme.conf.
# 2. the keys below; anything set here overrides the theme.
# theme = ""            # e.g. "~/.config/kitty/current-theme.conf"; relative paths start at this folder
# foreground = "#efdcf9"
# background = "#120a18"
# selection_background = "#583a7e"
# selection_foreground = ""   # empty = keep the text color
# cursor = ""                 # empty = same as the foreground
# cursor_text_color = ""      # empty = the background color
# color0 = "#45475a"    # black
# color1 = "#f38ba8"    # red
# color2 = "#a6e3a1"    # green
# color3 = "#f9e2af"    # yellow
# color4 = "#89b4fa"    # blue
# color5 = "#f5c2e7"    # magenta
# color6 = "#94e2d5"    # cyan
# color7 = "#bac2de"    # white
# color8 = "#585b70"    # bright black
# color9 = "#f38ba8"    # bright red
# color10 = "#a6e3a1"   # bright green
# color11 = "#f9e2af"   # bright yellow
# color12 = "#89b4fa"   # bright blue
# color13 = "#f5c2e7"   # bright magenta
# color14 = "#94e2d5"   # bright cyan
# color15 = "#cdd6f4"   # bright white
# color16 to color255 are accepted too.

[keybindings]
# Each action takes a list of key combinations; use [] to disable it.
# copy = ["ctrl+shift+c"]
# paste = ["ctrl+shift+v"]
# paste_primary = ["shift+insert"]
# reload_config = ["ctrl+shift+r"]
# zoom_in = ["ctrl+shift+plus", "ctrl+shift+equal"]
# zoom_out = ["ctrl+shift+minus", "ctrl+shift+underscore"]
# zoom_reset = ["ctrl+shift+zero", "ctrl+shift+paren_right"]
# scroll_page_up = ["shift+pageup"]
# scroll_page_down = ["shift+pagedown"]
# scroll_to_top = ["shift+home"]
# scroll_to_bottom = ["shift+end"]
# new_tab = ["ctrl+shift+t"]
# close_tab = ["ctrl+shift+w"]
# next_tab = ["ctrl+tab", "ctrl+pagedown"]
# prev_tab = ["ctrl+shift+tab", "ctrl+pageup"]
# fullscreen = ["f11"]
# preferences = []
"##;

#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub line_height: f32,
    pub text_composition_strategy: String,
}
impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "monospace".into(),
            size: 15.0,
            line_height: 1.25,
            text_composition_strategy: "platform".into(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextComposition {
    pub linear: bool,
    pub gamma: f32,
    pub contrast: f32,
}
impl Default for TextComposition {
    fn default() -> Self {
        Self { linear: true, gamma: 1.0, contrast: 0.0 }
    }
}
impl TextComposition {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "" | "platform" => Some(Self::default()),
            "legacy" => Some(Self { linear: false, gamma: 1.0, contrast: 0.0 }),
            other => {
                let mut it = other.split_whitespace();
                let gamma: f32 = it.next()?.parse().ok()?;
                let contrast: f32 = it.next().map_or(Some(0.0), |c| c.parse().ok())?;
                if it.next().is_some() || !gamma.is_finite() || !contrast.is_finite() {
                    return None;
                }
                Some(Self {
                    linear: true,
                    gamma: gamma.clamp(0.01, 10.0),
                    contrast: contrast.clamp(0.0, 100.0) / 100.0,
                })
            }
        }
    }
}
#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
    pub padding: f32,
    pub toolbar: bool,
    pub opacity: f32,
}
impl Default for WindowConfig {
    fn default() -> Self {
        Self { width: 960, height: 560, padding: 8.0, toolbar: false, opacity: 1.0 }
    }
}
#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    pub shell: String,
    pub scrollback: usize,
    pub working_directory: String,
    pub osc52: bool,
    pub smart_copy_paste: bool,
}
impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            shell: String::new(),
            scrollback: 10_000,
            working_directory: String::new(),
            osc52: true,
            smart_copy_paste: false,
        }
    }
}
#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct CursorConfig {
    pub shape: CursorShape,
    pub blink: bool,
    pub trail: u32,
    pub trail_decay: [f32; 2],
    pub trail_start_threshold: u32,
    pub trail_color: String,
}
impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            shape: CursorShape::Block,
            blink: true,
            trail: 0,
            trail_decay: [0.1, 0.4],
            trail_start_threshold: 2,
            trail_color: String::new(),
        }
    }
}
#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct BellConfig {
    pub visual: bool,
    pub urgent: bool,
}
impl Default for BellConfig {
    fn default() -> Self {
        Self { visual: true, urgent: true }
    }
}
#[derive(Clone, Debug, Default)]
pub struct ColorsConfig {
    pub theme: String,
    pub values: std::collections::BTreeMap<String, String>,
    pub normal: Option<Vec<String>>,
    pub bright: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for ColorsConfig {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let table = toml::Table::deserialize(d)?;
        let mut out = ColorsConfig::default();
        for (key, value) in table {
            match key.as_str() {
                "theme" => {
                    out.theme = value
                        .as_str()
                        .ok_or_else(|| D::Error::custom("colors.theme must be a string"))?
                        .into()
                }
                "normal" | "bright" => {
                    let list = value
                        .as_array()
                        .and_then(|a| {
                            a.iter().map(|v| v.as_str().map(str::to_string)).collect::<Option<Vec<_>>>()
                        })
                        .ok_or_else(|| D::Error::custom(format!("colors.{key} must be a list of colors")))?;
                    if key == "normal" {
                        out.normal = Some(list);
                    } else {
                        out.bright = Some(list);
                    }
                }
                k => {
                    let name = if k == "selection" { "selection_background" } else { k };
                    if !colors::is_color_key(name) {
                        return Err(D::Error::custom(format!(
                            "unknown color `{k}`; use kitty names such as foreground, background, \
                             selection_background, cursor or color0 to color255"
                        )));
                    }
                    let v = value
                        .as_str()
                        .ok_or_else(|| D::Error::custom(format!("colors.{k} must be a string")))?;
                    if !v.trim().is_empty() {
                        out.values.insert(name.to_string(), v.to_string());
                    }
                }
            }
        }
        Ok(out)
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct KeybindingsConfig {
    pub copy: Vec<String>,
    pub paste: Vec<String>,
    pub paste_primary: Vec<String>,
    pub reload_config: Vec<String>,
    pub zoom_in: Vec<String>,
    pub zoom_out: Vec<String>,
    pub zoom_reset: Vec<String>,
    pub scroll_page_up: Vec<String>,
    pub scroll_page_down: Vec<String>,
    pub scroll_to_top: Vec<String>,
    pub scroll_to_bottom: Vec<String>,
    pub new_tab: Vec<String>,
    pub close_tab: Vec<String>,
    pub next_tab: Vec<String>,
    pub prev_tab: Vec<String>,
    pub fullscreen: Vec<String>,
    pub preferences: Vec<String>,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
        Self {
            copy: v(&["ctrl+shift+c"]),
            paste: v(&["ctrl+shift+v"]),
            paste_primary: v(&["shift+insert"]),
            reload_config: v(&["ctrl+shift+r"]),
            zoom_in: v(&["ctrl+shift+plus", "ctrl+shift+equal"]),
            zoom_out: v(&["ctrl+shift+minus", "ctrl+shift+underscore"]),
            zoom_reset: v(&["ctrl+shift+zero", "ctrl+shift+paren_right"]),
            scroll_page_up: v(&["shift+pageup"]),
            scroll_page_down: v(&["shift+pagedown"]),
            scroll_to_top: v(&["shift+home"]),
            scroll_to_bottom: v(&["shift+end"]),
            new_tab: v(&["ctrl+shift+t"]),
            close_tab: v(&["ctrl+shift+w"]),
            next_tab: v(&["ctrl+tab", "ctrl+pagedown"]),
            prev_tab: v(&["ctrl+shift+tab", "ctrl+pageup"]),
            fullscreen: v(&["f11"]),
            preferences: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Copy,
    Paste,
    PastePrimary,
    ReloadConfig,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToTop,
    ScrollToBottom,
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    Fullscreen,
    Preferences,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
    pub key: String,
}

fn key_label(key: &str) -> String {
    let fixed = match key {
        "plus" => "Plus",
        "equal" => "=",
        "minus" => "-",
        "underscore" => "_",
        "zero" => "0",
        "paren_right" => ")",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "home" => "Home",
        "end" => "End",
        "insert" => "Insert",
        "delete" => "Delete",
        "enter" => "Enter",
        "escape" => "Esc",
        "tab" => "Tab",
        "space" => "Space",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    key.to_uppercase()
}

impl KeyCombo {
    pub fn parse(s: &str) -> Option<Self> {
        let mut c = KeyCombo { shift: false, ctrl: false, alt: false, logo: false, key: String::new() };
        for part in s.split('+') {
            match part.trim().to_lowercase().as_str() {
                "ctrl" | "control" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" => c.alt = true,
                "super" | "logo" | "cmd" | "meta" => c.logo = true,
                "" => {}
                other => c.key = other.to_string(),
            }
        }
        if c.key.is_empty() {
            None
        } else {
            Some(c)
        }
    }
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.logo {
            parts.push("Super".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        parts.push(key_label(&self.key));
        parts.join("+")
    }
}

#[derive(Clone, Debug, Default)]
pub struct Keymap(pub Vec<(KeyCombo, Action)>);

impl Keymap {
    pub fn label(&self, action: Action) -> Option<String> {
        self.0.iter().find(|(_, a)| *a == action).map(|(c, _)| c.label())
    }
}

fn add_bindings(v: &mut Vec<(KeyCombo, Action)>, list: &[String], action: Action) {
    for s in list {
        match KeyCombo::parse(s) {
            Some(combo) => v.push((combo, action)),
            None => log::error(&format!("aeternus: invalid key combination: {s:?}")),
        }
    }
}

impl KeybindingsConfig {
    fn build(&self) -> Keymap {
        let mut v = Vec::new();
        let table: [(&[String], Action); 17] = [
            (&self.copy, Action::Copy),
            (&self.paste, Action::Paste),
            (&self.paste_primary, Action::PastePrimary),
            (&self.reload_config, Action::ReloadConfig),
            (&self.zoom_in, Action::ZoomIn),
            (&self.zoom_out, Action::ZoomOut),
            (&self.zoom_reset, Action::ZoomReset),
            (&self.scroll_page_up, Action::ScrollPageUp),
            (&self.scroll_page_down, Action::ScrollPageDown),
            (&self.scroll_to_top, Action::ScrollToTop),
            (&self.scroll_to_bottom, Action::ScrollToBottom),
            (&self.new_tab, Action::NewTab),
            (&self.close_tab, Action::CloseTab),
            (&self.next_tab, Action::NextTab),
            (&self.prev_tab, Action::PrevTab),
            (&self.fullscreen, Action::Fullscreen),
            (&self.preferences, Action::Preferences),
        ];
        for (list, action) in table {
            add_bindings(&mut v, list, action);
        }
        Keymap(v)
    }
}
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub font: FontConfig,
    pub window: WindowConfig,
    pub terminal: TerminalConfig,
    pub cursor: CursorConfig,
    pub bell: BellConfig,
    pub colors: ColorsConfig,
    pub keybindings: KeybindingsConfig,
    #[serde(skip)]
    pub palette: Palette,
    #[serde(skip)]
    pub keymap: Keymap,
    #[serde(skip)]
    pub trail_color: Option<[u8; 3]>,
    #[serde(skip)]
    pub theme_path: Option<PathBuf>,
    #[serde(skip)]
    pub text_composition: TextComposition,
}
impl Config {
    fn sanitize(&mut self) {
        if self.font.family.trim().is_empty() {
            self.font.family = FontConfig::default().family;
        }
        self.font.size = self.font.size.clamp(4.0, 128.0);
        self.font.line_height = self.font.line_height.clamp(1.0, 3.0);
        self.window.width = self.window.width.clamp(200, 10_000);
        self.window.height = self.window.height.clamp(120, 10_000);
        self.window.padding = self.window.padding.clamp(0.0, 200.0);
        self.window.opacity =
            if self.window.opacity.is_finite() { self.window.opacity.clamp(0.0, 1.0) } else { 1.0 };
        self.terminal.scrollback = self.terminal.scrollback.min(1_000_000);
        self.cursor.trail = self.cursor.trail.min(60_000);
        for d in &mut self.cursor.trail_decay {
            *d = if d.is_finite() { d.clamp(0.01, 5.0) } else { 0.1 };
        }
        if self.cursor.trail_decay[0] > self.cursor.trail_decay[1] {
            self.cursor.trail_decay.swap(0, 1);
        }
        self.cursor.trail_start_threshold = self.cursor.trail_start_threshold.min(1000);
    }
    fn build_palette(&self) -> Palette {
        let c = &self.colors;
        let mut p = Palette::default();
        let mut cursor_set = false;
        let mut apply = |p: &mut Palette, key: &str, value: &str, origin: &str| {
            if key == "cursor" && !value.trim().is_empty() {
                cursor_set = !value.trim().eq_ignore_ascii_case("none");
            }
            if let colors::Applied::Invalid = p.apply(key, value) {
                log::error(&format!("aeternus: invalid color {key} = {value:?} in {origin}"));
            }
        };
        if let Some(path) = &self.theme_path {
            let mut entries = Vec::new();
            let mut errors = Vec::new();
            colors::read_kitty_theme(path, &mut entries, &mut errors, 0);
            for e in errors {
                log::error(&format!("aeternus: color theme: {e}"));
            }
            let origin = path.display().to_string();
            for (k, v) in entries {
                apply(&mut p, &k, &v, &origin);
            }
        }
        for (name, list, base) in [("normal", &c.normal, 0usize), ("bright", &c.bright, 8usize)] {
            let Some(list) = list else { continue };
            if list.len() != 8 {
                log::error(&format!("aeternus: colors.{name} needs 8 colors (found {})", list.len()));
                continue;
            }
            for (i, v) in list.iter().enumerate() {
                apply(&mut p, &format!("color{}", base + i), v, &format!("colors.{name}"));
            }
        }
        for (k, v) in &c.values {
            apply(&mut p, k, v, "[colors]");
        }
        if !cursor_set {
            p.cursor = p.fg;
        }
        p
    }
    fn finish(&mut self) {
        self.sanitize();
        self.theme_path = Some(self.colors.theme.trim()).filter(|t| !t.is_empty()).map(|t| {
            let base = config_path().and_then(|p| p.parent().map(Path::to_path_buf));
            colors::expand_path(t, base.as_deref())
        });
        self.palette = self.build_palette();
        self.keymap = self.keybindings.build();
        self.text_composition = TextComposition::parse(&self.font.text_composition_strategy).unwrap_or_else(|| {
            log::error(&format!(
                "aeternus: invalid font.text_composition_strategy {:?} (use \"platform\", \"legacy\" or \"<gamma> <contrast>\")",
                self.font.text_composition_strategy
            ));
            TextComposition::default()
        });
        let t = self.cursor.trail_color.trim();
        self.trail_color = if t.is_empty() {
            None
        } else {
            let parsed = colors::parse_hex(t);
            if parsed.is_none() {
                log::error(&format!("aeternus: invalid color in cursor.trail_color: {t:?}"));
            }
            parsed
        };
    }
    pub fn defaults() -> Self {
        let mut c = Config::default();
        c.finish();
        c
    }
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let mut cfg = toml::from_str::<Config>(text).map_err(|e| e.to_string())?;
        cfg.finish();
        Ok(cfg)
    }
}
static CONFIG_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();
pub fn set_config_path(path: PathBuf) {
    let _ = CONFIG_OVERRIDE.set(path);
}
pub fn config_path() -> Option<PathBuf> {
    match CONFIG_OVERRIDE.get() {
        Some(p) => Some(p.clone()),
        None => default_config_path(),
    }
}
fn default_config_path() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("aeternus").join("config.toml"))
}

pub fn mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(config_path()?).ok()?.modified().ok()
}

pub fn watched_mtime(cfg: &Config) -> Option<std::time::SystemTime> {
    let theme = cfg.theme_path.as_ref().and_then(|p| std::fs::metadata(p).ok()?.modified().ok());
    match (mtime(), theme) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = target.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
        if let Ok(meta) = std::fs::metadata(&target) {
            let _ = std::fs::set_permissions(&tmp, meta.permissions());
        }
        std::fs::rename(&tmp, &target)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", target.display()));
    }
    Ok(())
}

pub fn load() -> Result<Config, String> {
    match config_path() {
        None => Ok(Config::defaults()),
        Some(path) => load_from(&path),
    }
}

pub fn load_from(path: &Path) -> Result<Config, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Config::from_toml(&text).map_err(|e| format!("{}:\n{e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Err(e) = write_atomic(path, TEMPLATE) {
                log::error(&format!("aeternus: could not write the default config: {e}"));
            }
            Ok(Config::defaults())
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}
fn set_key(table: &mut dyn toml_edit::TableLike, key: &str, value: toml_edit::Value) {
    let decor = match table.get(key) {
        Some(toml_edit::Item::Value(v)) => v.decor().clone(),
        _ => toml_edit::Decor::default(),
    };
    let mut value = value;
    *value.decor_mut() = decor;
    table.insert(key, toml_edit::Item::Value(value));
}
fn section<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    name: &str,
) -> Result<&'a mut dyn toml_edit::TableLike, String> {
    if doc.get(name).is_none() {
        doc.as_table_mut().insert(name, toml_edit::Item::Table(toml_edit::Table::new()));
    }
    doc.as_table_mut()
        .get_mut(name)
        .and_then(|item| item.as_table_like_mut())
        .ok_or_else(|| format!("the [{name}] section in config.toml is not a table"))
}
pub fn save(cfg: &Config) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "no configuration path".to_string())?;
    save_to(&path, cfg)
}
pub fn save_to(path: &Path, cfg: &Config) -> Result<(), String> {
    use toml_edit::Value;
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => TEMPLATE.to_string(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| format!("{}: {e}", path.display()))?;
    let font = section(&mut doc, "font")?;
    set_key(font, "family", Value::from(cfg.font.family.clone()));
    set_key(font, "size", Value::from(f64::from(cfg.font.size)));
    set_key(font, "line_height", Value::from(f64::from(cfg.font.line_height)));
    let window = section(&mut doc, "window")?;
    set_key(window, "width", Value::from(i64::from(cfg.window.width)));
    set_key(window, "height", Value::from(i64::from(cfg.window.height)));
    set_key(window, "padding", Value::from(f64::from(cfg.window.padding)));
    set_key(window, "toolbar", Value::from(cfg.window.toolbar));
    set_key(window, "opacity", Value::from((f64::from(cfg.window.opacity) * 100.0).round() / 100.0));
    let terminal = section(&mut doc, "terminal")?;
    set_key(terminal, "shell", Value::from(cfg.terminal.shell.clone()));
    set_key(terminal, "scrollback", Value::from(cfg.terminal.scrollback as i64));
    set_key(terminal, "working_directory", Value::from(cfg.terminal.working_directory.clone()));
    set_key(terminal, "osc52", Value::from(cfg.terminal.osc52));
    set_key(terminal, "smart_copy_paste", Value::from(cfg.terminal.smart_copy_paste));
    let cursor = section(&mut doc, "cursor")?;
    let shape = match cfg.cursor.shape {
        CursorShape::Block => "block",
        CursorShape::Underline => "underline",
        CursorShape::Bar => "bar",
    };
    set_key(cursor, "shape", Value::from(shape));
    set_key(cursor, "blink", Value::from(cfg.cursor.blink));
    set_key(cursor, "trail", Value::from(i64::from(cfg.cursor.trail)));
    let bell = section(&mut doc, "bell")?;
    set_key(bell, "visual", Value::from(cfg.bell.visual));
    let colors = section(&mut doc, "colors")?;
    for key in ["foreground", "background", "selection_background", "cursor"] {
        let legacy = key == "selection_background" && colors.contains_key("selection");
        match cfg.colors.values.get(key) {
            Some(v) if legacy && !colors.contains_key(key) => {
                set_key(colors, "selection", Value::from(v.clone()))
            }
            Some(v) => set_key(colors, key, Value::from(v.clone())),
            None => {
                colors.remove(key);
                if legacy {
                    colors.remove("selection");
                }
            }
        }
    }
    write_atomic(path, &doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aeternus-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn key_combo_parse_modifiers_and_key() {
        let c = KeyCombo::parse("ctrl+shift+c").unwrap();
        assert!(c.ctrl && c.shift && !c.alt && !c.logo);
        assert_eq!(c.key, "c");
    }

    #[test]
    fn key_combo_parse_single_key() {
        let c = KeyCombo::parse("pageup").unwrap();
        assert!(!c.ctrl && !c.shift && !c.alt && !c.logo);
        assert_eq!(c.key, "pageup");
    }

    #[test]
    fn key_combo_parse_rejects_only_modifiers() {
        assert!(KeyCombo::parse("ctrl+shift").is_none());
    }

    #[test]
    fn key_combo_parse_is_case_insensitive() {
        let c = KeyCombo::parse("Ctrl+Shift+R").unwrap();
        assert!(c.ctrl && c.shift);
        assert_eq!(c.key, "r");
    }

    #[test]
    fn key_combo_label() {
        assert_eq!(KeyCombo::parse("ctrl+shift+t").unwrap().label(), "Ctrl+Shift+T");
        assert_eq!(KeyCombo::parse("ctrl+shift+equal").unwrap().label(), "Ctrl+Shift+=");
        assert_eq!(KeyCombo::parse("f11").unwrap().label(), "F11");
        assert_eq!(KeyCombo::parse("shift+pageup").unwrap().label(), "Shift+PageUp");
    }

    #[test]
    fn keybindings_default_builds_known_actions() {
        let km = KeybindingsConfig::default().build();
        for a in [Action::Copy, Action::ZoomIn, Action::ScrollToBottom, Action::NewTab, Action::NextTab] {
            assert!(km.0.iter().any(|(_, x)| *x == a), "{a:?}");
        }
        assert!(km.label(Action::Preferences).is_none());
    }

    #[test]
    fn keybindings_can_be_disabled() {
        let cfg = Config::from_toml("[keybindings]\nnew_tab = []\n").unwrap();
        assert!(cfg.keymap.label(Action::NewTab).is_none());
        assert_eq!(cfg.keymap.label(Action::CloseTab).as_deref(), Some("Ctrl+Shift+W"));
    }

    #[test]
    fn template_parses_to_defaults() {
        let cfg = Config::from_toml(TEMPLATE).unwrap();
        assert_eq!(cfg.font.family, "monospace");
        assert_eq!(cfg.palette, Palette::default());
        assert!(!cfg.terminal.smart_copy_paste);
        assert_eq!(cfg.cursor.trail, 0);
    }

    #[test]
    fn text_composition_strategy() {
        assert_eq!(TextComposition::parse("platform"), Some(TextComposition::default()));
        assert!(!TextComposition::parse("legacy").unwrap().linear);
        let c = TextComposition::parse("1.7 30").unwrap();
        assert!(c.linear && c.gamma == 1.7 && (c.contrast - 0.3).abs() < 1e-6);
        assert_eq!(TextComposition::parse("2").unwrap().contrast, 0.0);
        assert!(TextComposition::parse("thin").is_none());
        assert!(TextComposition::parse("1 2 3").is_none());
        let cfg = Config::from_toml("[font]\ntext_composition_strategy = \"bogus\"\n").unwrap();
        assert_eq!(cfg.text_composition, TextComposition::default());
    }

    #[test]
    fn kitty_color_names() {
        let cfg = Config::from_toml(
            "[colors]\nforeground = \"#010203\"\ncolor1 = \"#ff0000\"\ncolor200 = \"#00ff00\"\n\
             selection_foreground = \"#ffffff\"\ncursor_text_color = \"#000000\"\n",
        )
        .unwrap();
        assert_eq!(cfg.palette.fg, [1, 2, 3]);
        assert_eq!(cfg.palette.cursor, [1, 2, 3]);
        assert_eq!(cfg.palette.ansi[1], [255, 0, 0]);
        assert_eq!(cfg.palette.ansi[200], [0, 255, 0]);
        assert_eq!(cfg.palette.selection_fg, Some([255, 255, 255]));
        assert_eq!(cfg.palette.cursor_text, Some([0, 0, 0]));
        assert!(Config::from_toml("[colors]\nurl_color = \"#fff\"\n").is_err());
    }

    #[test]
    fn legacy_color_format_still_works() {
        let cfg = Config::from_toml(
            "[colors]\nselection = \"#111111\"\nnormal = [\"#000000\", \"#100000\", \"#200000\", \"#300000\", \
             \"#400000\", \"#500000\", \"#600000\", \"#700000\"]\n",
        )
        .unwrap();
        assert_eq!(cfg.palette.selection, [0x11, 0x11, 0x11]);
        assert_eq!(cfg.palette.ansi[3], [0x30, 0, 0]);
        assert_eq!(cfg.palette.ansi[9], Palette::default().ansi[9]);
    }

    #[test]
    fn kitty_theme_file_and_overrides() {
        let dir = temp_dir("theme");
        std::fs::write(
            dir.join("t.conf"),
            "foreground #aaaaaa\nbackground #bbbbbb\ncolor2 #00aa00\ncursor #cccccc\n",
        )
        .unwrap();
        let text =
            format!("[colors]\ntheme = \"{}\"\nbackground = \"#000000\"\n", dir.join("t.conf").display());
        let cfg = Config::from_toml(&text).unwrap();
        assert_eq!(cfg.palette.fg, [0xaa; 3]);
        assert_eq!(cfg.palette.bg, [0, 0, 0]);
        assert_eq!(cfg.palette.ansi[2], [0, 0xaa, 0]);
        assert_eq!(cfg.palette.cursor, [0xcc; 3]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_writes_kitty_names_and_keeps_legacy_selection() {
        let dir = temp_dir("savecolors");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[colors]\nselection = \"#111111\"\ncolor1 = \"#ff0000\"\n").unwrap();
        let mut cfg = Config::from_toml(&std::fs::read_to_string(&path).unwrap()).unwrap();
        cfg.colors.values.insert("selection_background".into(), "#222222".into());
        cfg.colors.values.insert("foreground".into(), "#333333".into());
        save_to(&path, &cfg).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("selection = \"#222222\""));
        assert!(text.contains("foreground = \"#333333\""));
        assert!(text.contains("color1 = \"#ff0000\""));
        let back = Config::from_toml(&text).unwrap();
        assert_eq!(back.palette.selection, [0x22; 3]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(Config::from_toml("[font]\nfamliy = \"x\"\n").is_err());
    }

    #[test]
    fn sanitize_clamps_values() {
        let cfg = Config::from_toml("[font]\nsize = 1000.0\n[cursor]\ntrail_decay = [0.9, 0.2]\n").unwrap();
        assert_eq!(cfg.font.size, 128.0);
        let op = Config::from_toml("[window]\nopacity = 7.0\n").unwrap();
        assert_eq!(op.window.opacity, 1.0);
        let op = Config::from_toml("[window]\nopacity = -1.0\n").unwrap();
        assert_eq!(op.window.opacity, 0.0);
        assert_eq!(cfg.cursor.trail_decay, [0.2, 0.9]);
    }

    #[test]
    fn save_preserves_comments_and_updates_values() {
        let dir = temp_dir("save");
        let path = dir.join("config.toml");
        std::fs::write(&path, "# my notes\n[font]\nsize = 12.0 # tiny\n").unwrap();
        let mut cfg = Config::defaults();
        cfg.font.size = 20.0;
        cfg.cursor.trail = 3;
        save_to(&path, &cfg).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my notes"));
        assert!(text.contains("# tiny"));
        let back = Config::from_toml(&text).unwrap();
        assert_eq!(back.font.size, 20.0);
        assert_eq!(back.cursor.trail, 3);
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_keeps_symlink() {
        let dir = temp_dir("symlink");
        let real = dir.join("real.toml");
        let link = dir.join("config.toml");
        std::fs::write(&real, "").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        save_to(&link, &Config::defaults()).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert!(std::fs::read_to_string(&real).unwrap().contains("[font]"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_from_missing_writes_template() {
        let dir = temp_dir("missing");
        let path = dir.join("sub").join("config.toml");
        let cfg = load_from(&path).unwrap();
        assert_eq!(cfg.font.family, "monospace");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
