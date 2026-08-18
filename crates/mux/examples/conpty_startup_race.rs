//! Investigation harness for the "input echo lands one row below the prompt"
//! bug. It reproduces, outside the GUI, exactly what `--start-conf` does:
//! open a pty at the initial (small) size, spawn the shell, immediately type
//! a command into it without waiting for a prompt, and then resize the pty
//! shortly afterwards the way maximizing the window does.
//!
//! Everything the pty produces is both (a) recorded with timestamps and
//! dumped as escaped text, and (b) fed through a real `wezterm_term::Terminal`
//! so the resulting grid can be printed. If the grid shows the echo one row
//! below the prompt, the defect is reproducible headlessly and can be turned
//! into a regression test.
//!
//! Not a test: it drives a real shell and is inherently timing-dependent.
//! Run it by hand:
//!
//! ```text
//! cargo run -p mux --example conpty_startup_race -- "git branch" 150
//! ```

use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::Write;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wezterm_term::{Terminal, TerminalSize};

/// A `Write` that several owners can hold at once: the `Terminal` (for
/// answerbacks such as DA1 and the cursor-position report ConPTY asks for at
/// startup) and this example's own "typing".
#[derive(Clone)]
struct SharedWriter(Arc<Mutex<Box<dyn Write + Send>>>);

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "git branch".to_string());
    let resize_after_ms: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(150);

    // The geometry a `--start-conf` tab is born with, then the geometry it
    // gets once the window maximizes.
    let start = TerminalSize {
        rows: 24,
        cols: 80,
        pixel_width: 640,
        pixel_height: 384,
        dpi: 96,
    };
    let maximized = TerminalSize {
        rows: 51,
        cols: 209,
        pixel_width: 1672,
        pixel_height: 816,
        dpi: 96,
    };

    let pty_system = NativePtySystem::default();
    let pair = pty_system.openpty(PtySize {
        rows: start.rows as u16,
        cols: start.cols as u16,
        pixel_width: start.pixel_width as u16,
        pixel_height: start.pixel_height as u16,
    })?;

    let mut cmd = CommandBuilder::new("cmd.exe");
    cmd.cwd(std::env::current_dir()?);
    let _child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);

    let writer = SharedWriter(Arc::new(Mutex::new(pair.master.take_writer()?)));

    let mut terminal = Terminal::new(
        start,
        Arc::new(config::TermConfig::new()),
        "OnlyTerm",
        config::wezterm_version(),
        Box::new(writer.clone()),
    );
    terminal.enable_conpty_quirks();

    let t0 = Instant::now();

    let (tx, rx) = channel::<(Duration, Vec<u8>)>();
    let mut reader = pair.master.try_clone_reader()?;
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send((t0.elapsed(), buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Exactly what spawn_startup_layout does: type straight in, immediately
    // after the spawn, without waiting for a prompt.
    {
        let mut w = writer.clone();
        write!(w, "{command}\r")?;
        w.flush()?;
    }
    println!(
        "[{:>7.1}ms] typed {:?}\\r",
        t0.elapsed().as_secs_f64() * 1000.,
        command
    );

    let resize_at = Instant::now() + Duration::from_millis(resize_after_ms);
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut resized = false;
    let mut transcript: Vec<(Duration, Vec<u8>)> = vec![];

    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        if !resized && now >= resize_at {
            resized = true;
            println!(
                "[{:>7.1}ms] resize {}x{} -> {}x{}",
                t0.elapsed().as_secs_f64() * 1000.,
                start.cols,
                start.rows,
                maximized.cols,
                maximized.rows
            );
            pair.master.resize(PtySize {
                rows: maximized.rows as u16,
                cols: maximized.cols as u16,
                pixel_width: maximized.pixel_width as u16,
                pixel_height: maximized.pixel_height as u16,
            })?;
            terminal.resize(maximized);
            continue;
        }
        let wait = resize_at.saturating_duration_since(now).min(
            deadline
                .saturating_duration_since(now)
                .min(Duration::from_millis(20)),
        );
        match rx.recv_timeout(wait.max(Duration::from_millis(1))) {
            Ok((at, bytes)) => {
                terminal.advance_bytes(&bytes);
                transcript.push((at, bytes));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    println!("\n===== pty transcript ({} chunks) =====", transcript.len());
    for (at, bytes) in &transcript {
        let text: String = String::from_utf8_lossy(bytes)
            .chars()
            .flat_map(|c| c.escape_debug())
            .collect();
        println!("[{:>7.1}ms] {}", at.as_secs_f64() * 1000., text);
    }

    println!("\n===== terminal grid =====");
    let screen = terminal.screen();
    let cursor = terminal.cursor_pos();
    let first = screen.phys_row(0);
    let lines = screen.lines_in_phys_range(first..screen.scrollback_rows());
    for (row, line) in lines.iter().enumerate() {
        let s = line.as_str();
        let s = s.trim_end();
        if !s.is_empty() {
            println!("row {:>2}: {:?}", row, s);
        }
    }
    println!(
        "cursor: x={} y={} (physical_rows={} scrollback_rows={})",
        cursor.x,
        cursor.y,
        screen.physical_rows,
        screen.scrollback_rows()
    );

    drop(pair.master);
    Ok(())
}
