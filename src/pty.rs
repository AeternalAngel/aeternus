use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

pub type Writer = Arc<Mutex<Box<dyn io::Write + Send>>>;

pub struct Pty {
    pub writer: Writer,
    pub child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
}

#[derive(Clone, Copy, Debug)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
    pub cell_w: u16,
    pub cell_h: u16,
}

impl Size {
    fn pty(self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: self.cols.saturating_mul(self.cell_w),
            pixel_height: self.rows.saturating_mul(self.cell_h),
        }
    }
}

pub fn parse_command(shell: &str) -> Result<Vec<String>, String> {
    let argv = shell_words::split(shell).map_err(|e| format!("invalid shell command {shell:?}: {e}"))?;
    if argv.is_empty() {
        return Err("empty shell command".to_string());
    }
    Ok(argv)
}

impl Pty {
    pub fn spawn(
        argv: &[String],
        cwd: Option<&Path>,
        size: Size,
    ) -> io::Result<(Self, Box<dyn io::Read + Send>)> {
        let pty = native_pty_system().openpty(size.pty()).map_err(io::Error::other)?;
        let (program, args) = argv.split_first().ok_or_else(|| io::Error::other("empty command"))?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        if let Some(dir) = cwd.filter(|d| d.is_dir()) {
            cmd.cwd(dir);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "aeternus");
        cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        let child = pty.slave.spawn_command(cmd).map_err(io::Error::other)?;
        let reader = pty.master.try_clone_reader().map_err(io::Error::other)?;
        let writer = pty.master.take_writer().map_err(io::Error::other)?;
        drop(pty.slave);
        Ok((Self { writer: Arc::new(Mutex::new(writer)), child, master: pty.master }, reader))
    }
    pub fn resize(&self, size: Size) {
        let _ = self.master.resize(size.pty());
    }
    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_commands() {
        assert_eq!(parse_command("bash -l").unwrap(), vec!["bash", "-l"]);
        assert_eq!(
            parse_command("\"/opt/my shell/zsh\" -c 'echo hi'").unwrap(),
            vec!["/opt/my shell/zsh", "-c", "echo hi"]
        );
        assert!(parse_command("").is_err());
        assert!(parse_command("bash 'unterminated").is_err());
    }

    #[test]
    fn spawns_and_reads_output() {
        let argv = vec!["/bin/sh".to_string(), "-c".to_string(), "printf ok".to_string()];
        let size = Size { cols: 80, rows: 24, cell_w: 8, cell_h: 16 };
        let (mut pty, mut reader) = Pty::spawn(&argv, None, size).unwrap();
        let mut out = Vec::new();
        let mut buf = [0u8; 64];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
            if out.windows(2).any(|w| w == b"ok") {
                break;
            }
        }
        assert!(String::from_utf8_lossy(&out).contains("ok"));
        let _ = pty.child.wait();
    }
    use std::io::Read;
}
