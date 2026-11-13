use std::path::PathBuf;
use std::sync::OnceLock;
use serde::Deserialize;
use crate::colors::{self, Palette};
use crate::term::CursorShape;
const TEMPLATE: &str = r##"# aeternus: configuração
# Tudo abaixo está comentado, e os valores são os padrões.
# Descomente e edite o que quiser mudar. Ctrl+Shift+R recarrega este arquivo.
[font]
# family = "GoogleSansCode Nerd Font Mono"
# size = 15.0           # pixels lógicos
# line_height = 1.25    # multiplicador do tamanho da fonte
[window]
# width = 960           # só vale ao abrir (pixels lógicos)
# height = 560          # só vale ao abrir
# padding = 8           # margem interna, em pixels lógicos
# toolbar = false       # barra de menus no topo: Arquivo, Editar, Ver, Ajuda
[terminal]
# shell = ""            # vazio = usa $SHELL; só vale ao abrir; aceita argumentos, ex. "bash -l"
# scrollback = 10000    # linhas de histórico
# working_directory = ""  # vazio = diretório atual; só vale ao abrir
# osc52 = true          # permite que programas escrevam no clipboard via OSC 52
[cursor]
# shape = "block"       # block, underline ou bar
# blink = true
[colors]
# foreground = "#efdcf9"
# background = "#120a18"
# selection = "#583a7e"
# cursor = ""           # vazio = mesma cor do texto
# normal = ["#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de"]
# bright = ["#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#cdd6f4"]

[keybindings]
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
"##;
#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub line_height: f32,
}
impl Default for FontConfig {
    fn default() -> Self {
        Self { family: "GoogleSansCode Nerd Font Mono".into(), size: 15.0, line_height: 1.25 }
    }
}
#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
    pub padding: f32,
    pub toolbar: bool,
}
impl Default for WindowConfig {
    fn default() -> Self {
        Self { width: 960, height: 560, padding: 8.0, toolbar: false }
    }
}
#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    pub shell: String,
    pub scrollback: usize,
    pub working_directory: String,
    pub osc52: bool,
}
impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            shell: String::new(),
            scrollback: 10_000,
            working_directory: String::new(),
            osc52: true,
        }
    }
}
#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct CursorConfig {
    pub shape: CursorShape,
    pub blink: bool,
}
impl Default for CursorConfig {
    fn default() -> Self {
        Self { shape: CursorShape::Block, blink: true }
    }
}
#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct ColorsConfig {
    pub foreground: String,
    pub background: String,
    pub selection: String,
    pub cursor: String,
    pub normal: Vec<String>,
    pub bright: Vec<String>,
}
impl Default for ColorsConfig {
    fn default() -> Self {
        let v = |a: [&str; 8]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        Self {
            foreground: "#efdcf9".into(),
            background: "#120a18".into(),
            selection: "#583a7e".into(),
            cursor: String::new(),
            normal: v([
                "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5",
                "#bac2de",
            ]),
            bright: v([
                "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5",
                "#cdd6f4",
            ]),
        }
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
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
    pub key: String,
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
}

#[derive(Clone, Debug, Default)]
pub struct Keymap(pub Vec<(KeyCombo, Action)>);

fn add_bindings(v: &mut Vec<(KeyCombo, Action)>, list: &[String], action: Action) {
    for s in list {
        match KeyCombo::parse(s) {
            Some(combo) => v.push((combo, action)),
            None => report_error(&format!("aeternus: combinação de tecla inválida: {s:?}")),
        }
    }
}

impl KeybindingsConfig {
    fn build(&self) -> Keymap {
        let mut v = Vec::new();
        add_bindings(&mut v, &self.copy, Action::Copy);
        add_bindings(&mut v, &self.paste, Action::Paste);
        add_bindings(&mut v, &self.paste_primary, Action::PastePrimary);
        add_bindings(&mut v, &self.reload_config, Action::ReloadConfig);
        add_bindings(&mut v, &self.zoom_in, Action::ZoomIn);
        add_bindings(&mut v, &self.zoom_out, Action::ZoomOut);
        add_bindings(&mut v, &self.zoom_reset, Action::ZoomReset);
        add_bindings(&mut v, &self.scroll_page_up, Action::ScrollPageUp);
        add_bindings(&mut v, &self.scroll_page_down, Action::ScrollPageDown);
        add_bindings(&mut v, &self.scroll_to_top, Action::ScrollToTop);
        add_bindings(&mut v, &self.scroll_to_bottom, Action::ScrollToBottom);
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
    pub colors: ColorsConfig,
    pub keybindings: KeybindingsConfig,
    #[serde(skip)]
    pub palette: Palette,
    #[serde(skip)]
    pub keymap: Keymap,
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
        self.terminal.scrollback = self.terminal.scrollback.min(1_000_000);
    }
    fn build_palette(&self) -> Palette {
        let d = Palette::default();
        let c = &self.colors;
        let col = |name: &str, s: &str, fallback: [u8; 3]| -> [u8; 3] {
            colors::parse_hex(s).unwrap_or_else(|| {
                report_error(&format!(
                    "aeternus: cor inválida em colors.{name}: {s:?} (esperado \"#rrggbb\")"
                ));
                fallback
            })
        };
        let fg = col("foreground", &c.foreground, d.fg);
        let bg = col("background", &c.background, d.bg);
        let selection = col("selection", &c.selection, d.selection);
        let cursor = if c.cursor.trim().is_empty() { fg } else { col("cursor", &c.cursor, fg) };
        let mut ansi = d.ansi;
        for (name, list, base) in [("normal", &c.normal, 0usize), ("bright", &c.bright, 8usize)] {
            if list.len() != 8 {
                report_error(&format!(
                    "aeternus: colors.{name} precisa de 8 cores (tem {})",
                    list.len()
                ));
                continue;
            }
            for (i, s) in list.iter().enumerate() {
                ansi[base + i] = col(&format!("{name}[{i}]"), s, d.ansi[base + i]);
            }
        }
        Palette { fg, bg, selection, cursor, ansi }
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

pub fn report_error(msg: &str) {
    eprintln!("{msg}");
}
fn write_template(path: &PathBuf) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, TEMPLATE);
}
pub fn load() -> Result<Config, String> {
    let mut cfg = match config_path() {
        None => Config::default(),
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str::<Config>(&text)
                .map_err(|e| format!("{}:\n{e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                write_template(&path);
                Config::default()
            }
            Err(e) => return Err(format!("{}: {e}", path.display())),
        },
    };
    cfg.sanitize();
    cfg.palette = cfg.build_palette();
    cfg.keymap = cfg.keybindings.build();
    Ok(cfg)
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
        .ok_or_else(|| format!("a seção [{name}] no config.toml não é uma tabela"))
}
pub fn save(cfg: &Config) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "sem caminho de configuração".to_string())?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => TEMPLATE.to_string(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let font = section(&mut doc, "font")?;
    set_key(font, "family", toml_edit::Value::from(cfg.font.family.clone()));
    set_key(font, "size", toml_edit::Value::from(f64::from(cfg.font.size)));
    set_key(font, "line_height", toml_edit::Value::from(f64::from(cfg.font.line_height)));
    let window = section(&mut doc, "window")?;
    set_key(window, "width", toml_edit::Value::from(i64::from(cfg.window.width)));
    set_key(window, "height", toml_edit::Value::from(i64::from(cfg.window.height)));
    set_key(window, "padding", toml_edit::Value::from(f64::from(cfg.window.padding)));
    set_key(window, "toolbar", toml_edit::Value::from(cfg.window.toolbar));
    let terminal = section(&mut doc, "terminal")?;
    set_key(terminal, "shell", toml_edit::Value::from(cfg.terminal.shell.clone()));
    set_key(terminal, "scrollback", toml_edit::Value::from(cfg.terminal.scrollback as i64));
    set_key(
        terminal,
        "working_directory",
        toml_edit::Value::from(cfg.terminal.working_directory.clone()),
    );
    set_key(terminal, "osc52", toml_edit::Value::from(cfg.terminal.osc52));
    let cursor = section(&mut doc, "cursor")?;
    let shape = match cfg.cursor.shape {
        CursorShape::Block => "block",
        CursorShape::Underline => "underline",
        CursorShape::Bar => "bar",
    };
    set_key(cursor, "shape", toml_edit::Value::from(shape));
    set_key(cursor, "blink", toml_edit::Value::from(cfg.cursor.blink));
    let colors = section(&mut doc, "colors")?;
    set_key(colors, "foreground", toml_edit::Value::from(cfg.colors.foreground.clone()));
    set_key(colors, "background", toml_edit::Value::from(cfg.colors.background.clone()));
    set_key(colors, "selection", toml_edit::Value::from(cfg.colors.selection.clone()));
    set_key(colors, "cursor", toml_edit::Value::from(cfg.colors.cursor.clone()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, doc.to_string()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn keybindings_default_builds_known_actions() {
        let km = KeybindingsConfig::default().build();
        assert!(km.0.iter().any(|(_, a)| *a == Action::Copy));
        assert!(km.0.iter().any(|(_, a)| *a == Action::ZoomIn));
        assert!(km.0.iter().any(|(_, a)| *a == Action::ScrollToBottom));
    }
}
