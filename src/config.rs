use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::colors::{self, Palette};
use crate::term::CursorShape;

/// Modelo comentado, escrito na primeira execução. Tudo vem comentado, então
/// os valores padrão do código continuam valendo até você descomentar algo.
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
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self { width: 960, height: 560, padding: 8.0 }
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

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub font: FontConfig,
    pub window: WindowConfig,
    pub terminal: TerminalConfig,
    pub cursor: CursorConfig,
    pub colors: ColorsConfig,
    /// Cores já resolvidas (calculado em `load`, não vem do arquivo).
    #[serde(skip)]
    pub palette: Palette,
}

impl Config {
    /// Mantém os valores em faixas que não quebram o terminal.
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

    /// Converte as cores em texto para RGB. Valor inválido avisa e usa o padrão.
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

#[cfg(windows)]
fn default_config_path() -> Option<PathBuf> {
    let base = PathBuf::from(std::env::var_os("APPDATA")?);
    Some(base.join("aeternus").join("config.toml"))
}

#[cfg(not(windows))]
fn default_config_path() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("aeternus").join("config.toml"))
}

pub fn report_error(msg: &str) {
    #[cfg(windows)]
    {
        use std::io::Write;
        let Some(log) = config_path().map(|p| p.with_file_name("aeternus.log")) else { return };
        if let Some(dir) = log.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
            let _ = writeln!(f, "{msg}");
        }
    }
    #[cfg(not(windows))]
    eprintln!("{msg}");
}

fn write_template(path: &PathBuf) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, TEMPLATE);
}

/// Lê a configuração. Arquivo inexistente: cria o modelo e usa os padrões.
/// Erro de sintaxe ou chave desconhecida: devolve a mensagem (com linha/coluna).
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
    Ok(cfg)
}
