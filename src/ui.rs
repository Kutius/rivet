use std::io::{self, IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn stderr_is_tty() -> bool {
    io::stderr().is_terminal()
}

pub fn stdout_is_tty() -> bool {
    io::stdout().is_terminal()
}

fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

/// Print a completed action line. In an interactive terminal, renders a green
/// `✓` prefix and optional dim suffix (e.g., elapsed time or cache status);
/// when piped/redirected, prints `plain` verbatim.
pub fn print_done(plain: &str, tty_detail: Option<&str>) {
    if stdout_is_tty() {
        if color_enabled() {
            match tty_detail {
                Some(detail) if !detail.is_empty() => {
                    println!("\x1b[32m✓\x1b[0m {plain} \x1b[2m{detail}\x1b[0m");
                }
                _ => println!("\x1b[32m✓\x1b[0m {plain}"),
            }
        } else {
            match tty_detail {
                Some(detail) if !detail.is_empty() => println!("✓ {plain} {detail}"),
                _ => println!("✓ {plain}"),
            }
        }
    } else {
        println!("{plain}");
    }
}

/// Print a quiet, indented hint line only when `stdout` is an interactive TTY.
pub fn print_hint(message: &str) {
    if !stdout_is_tty() {
        return;
    }
    if color_enabled() {
        println!("  \x1b[2m{message}\x1b[0m");
    } else {
        println!("  {message}");
    }
}

/// Print a short batch summary line when multiple items were processed in a TTY.
pub fn print_summary(verb: &str, count: usize, elapsed: Duration) {
    if !stdout_is_tty() || count <= 1 {
        return;
    }
    let noun = if count == 1 { "package" } else { "packages" };
    let time = format_duration(elapsed);
    if color_enabled() {
        println!("\x1b[2mDone — {count} {noun} {verb} in {time}\x1b[0m");
    } else {
        println!("Done — {count} {noun} {verb} in {time}");
    }
}

/// Format a duration concisely (`42ms`, `1.2s`, `1m 14s`).
pub fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1000 {
        format!("{millis}ms")
    } else if millis < 60_000 {
        format!("{:.1}s", duration.as_secs_f64())
    } else {
        let secs = duration.as_secs();
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

/// Format a byte count into a human-readable string (`B`, `KiB`, `MiB`, `GiB`).
pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        format!("{:.1} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.1} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.1} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// Format a sleek horizontal progress bar (`━━━━━━━━━━━━────────`).
pub fn format_bar(current: u64, total: u64, width: usize) -> String {
    if total == 0 || width == 0 {
        return String::new();
    }
    let ratio = (current.min(total) as f64) / (total as f64);
    let filled = ((ratio * width as f64).round() as usize).min(width);
    let empty = width - filled;
    format!("{}{}", "━".repeat(filled), "─".repeat(empty))
}

fn format_colored_bar(current: u64, total: u64, width: usize) -> String {
    if total == 0 || width == 0 {
        return String::new();
    }
    let ratio = (current.min(total) as f64) / (total as f64);
    let filled = ((ratio * width as f64).round() as usize).min(width);
    let empty = width - filled;
    format!(
        "\x1b[36m{}\x1b[2;37m{}\x1b[0m",
        "━".repeat(filled),
        "─".repeat(empty)
    )
}

/// A lightweight RAII spinner rendered on `stderr` when attached to a TTY.
/// Becomes a zero-cost no-op when `stderr` is not a TTY.
pub struct Spinner {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    active: bool,
}

impl Spinner {
    pub fn start(message: impl Into<String>) -> Self {
        if !stderr_is_tty() {
            return Self {
                stop: Arc::new(AtomicBool::new(true)),
                handle: None,
                active: false,
            };
        }
        let message = message.into();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let colored = color_enabled();
        let handle = thread::spawn(move || {
            let mut idx = 0usize;
            while !flag.load(Ordering::Relaxed) {
                let frame = FRAMES[idx % FRAMES.len()];
                if colored {
                    let _ = write!(io::stderr(), "\r\x1b[2K\x1b[36m{frame}\x1b[0m {message}");
                } else {
                    let _ = write!(io::stderr(), "\r\x1b[2K{frame} {message}");
                }
                let _ = io::stderr().flush();
                idx = idx.wrapping_add(1);
                thread::sleep(Duration::from_millis(80));
            }
        });
        Self {
            stop,
            handle: Some(handle),
            active: true,
        }
    }

    pub fn clear(mut self) {
        self.stop_inner();
    }

    fn stop_inner(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let _ = write!(io::stderr(), "\r\x1b[2K");
        let _ = io::stderr().flush();
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

/// Single-line download progress bar rendered on `stderr` when attached to a TTY.
pub struct ProgressBar {
    label: String,
    started: Instant,
    last_draw: Instant,
    frame: usize,
    active: bool,
    colored: bool,
}

impl ProgressBar {
    pub fn new(label: impl Into<String>) -> Self {
        let now = Instant::now();
        Self {
            label: label.into(),
            started: now,
            last_draw: now - Duration::from_secs(1),
            frame: 0,
            active: stderr_is_tty(),
            colored: color_enabled(),
        }
    }

    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }

    pub fn update(&mut self, current: u64, total: Option<u64>) {
        if !self.active {
            return;
        }
        let now = Instant::now();
        let done = total.is_some_and(|t| current >= t);
        if !done && now.duration_since(self.last_draw) < Duration::from_millis(60) {
            return;
        }
        self.last_draw = now;
        let frame = FRAMES[self.frame % FRAMES.len()];
        self.frame = self.frame.wrapping_add(1);

        let elapsed = now.duration_since(self.started).as_secs_f64().max(0.001);
        let speed = format!("{}/s", format_bytes((current as f64 / elapsed) as u64));

        let line = match total {
            Some(total) if total > 0 => {
                let pct = ((current.min(total) as f64 / total as f64) * 100.0).round() as u64;
                if self.colored {
                    let bar = format_colored_bar(current, total, 22);
                    format!(
                        "\r\x1b[2K\x1b[36m{frame}\x1b[0m {}  {bar} {:>3}% \x1b[2m{} / {} · {speed}\x1b[0m",
                        self.label,
                        pct,
                        format_bytes(current),
                        format_bytes(total),
                    )
                } else {
                    let bar = format_bar(current, total, 22);
                    format!(
                        "\r\x1b[2K{frame} {}  {bar} {:>3}% {} / {} · {speed}",
                        self.label,
                        pct,
                        format_bytes(current),
                        format_bytes(total),
                    )
                }
            }
            _ => {
                if self.colored {
                    format!(
                        "\r\x1b[2K\x1b[36m{frame}\x1b[0m {}  \x1b[2m{} · {speed}\x1b[0m",
                        self.label,
                        format_bytes(current),
                    )
                } else {
                    format!(
                        "\r\x1b[2K{frame} {}  {} · {speed}",
                        self.label,
                        format_bytes(current),
                    )
                }
            }
        };
        let _ = write!(io::stderr(), "{line}");
        let _ = io::stderr().flush();
    }

    pub fn clear(mut self) {
        self.clear_inner();
    }

    fn clear_inner(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let _ = write!(io::stderr(), "\r\x1b[2K");
        let _ = io::stderr().flush();
    }
}

impl Drop for ProgressBar {
    fn drop(&mut self) {
        self.clear_inner();
    }
}

#[cfg(test)]
mod tests {
    use super::{format_bar, format_bytes, format_duration};
    use std::time::Duration;

    #[test]
    fn formats_byte_sizes_cleanly() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_bytes(3 * 1024 * 1024), "3.0 MiB");
    }

    #[test]
    fn formats_progress_bar_proportionally() {
        assert_eq!(format_bar(0, 100, 10), "──────────");
        assert_eq!(format_bar(50, 100, 10), "━━━━━─────");
        assert_eq!(format_bar(100, 100, 10), "━━━━━━━━━━");
    }

    #[test]
    fn formats_durations_concisely() {
        assert_eq!(format_duration(Duration::from_millis(42)), "42ms");
        assert_eq!(format_duration(Duration::from_millis(1250)), "1.2s");
        assert_eq!(format_duration(Duration::from_secs(74)), "1m 14s");
    }
}
