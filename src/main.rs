use aeternus::{app, bench, config, log};

const HELP: &str = "\
Aeternus - a GPU-accelerated terminal emulator

usage: aeternus [options]

options:
  -c, --config <path>  use this configuration file instead of the default one
      --bench          run the parser benchmark and exit
  -h, --help           show this help and exit
  -V, --version        print the version and exit

The companion `aet` command talks to a running instance (see `aet --help`).";

enum Mode {
    Run,
    Bench,
}

fn parse_args() -> Result<Option<Mode>, String> {
    let mut args = std::env::args().skip(1);
    let mut mode = Mode::Run;
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(None);
            }
            "-V" | "-v" | "--version" => {
                println!("aeternus {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--bench" => mode = Mode::Bench,
            "-c" | "--config" => match args.next() {
                Some(p) => config::set_config_path(p.into()),
                None => return Err(format!("{a} needs a path")),
            },
            s if s.starts_with("--config=") => config::set_config_path(s["--config=".len()..].into()),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Some(mode))
}

fn main() {
    let mode = match parse_args() {
        Ok(Some(m)) => m,
        Ok(None) => return,
        Err(e) => {
            eprintln!("aeternus: {e}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    if let Mode::Bench = mode {
        bench::run_all();
        return;
    }
    log::init();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error(&format!("aeternus: crashed: {info}"));
        app::cleanup_ipc_files();
        default_hook(info);
    }));
    if let Err(e) = app::run() {
        log::fatal(&format!("aeternus: {e}"));
        std::process::exit(1);
    }
}
