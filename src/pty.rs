use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io;
use std::sync::{Arc, Mutex};

pub struct Pty {
    pub reader: Box<dyn io::Read + Send>,
    pub writer: Arc<Mutex<Box<dyn io::Write + Send>>>,
    pub child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
}

impl Pty {
    pub fn spawn(shell: &str, cols: u16, rows: u16) -> io::Result<Self> {
        let size = PtySize { rows, cols, pixel_width: 0, pixel_height: 0 };
        let pty = native_pty_system().openpty(size).map_err(io::Error::other)?;

        let mut cmd = CommandBuilder::new(shell);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = pty.slave.spawn_command(cmd).map_err(io::Error::other)?;
        let reader = pty.master.try_clone_reader().map_err(io::Error::other)?;
        let writer = pty.master.take_writer().map_err(io::Error::other)?;
        drop(pty.slave);

        Ok(Self {
            reader,
            writer: Arc::new(Mutex::new(writer)),
            child,
            master: pty.master,
        })
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }
}
