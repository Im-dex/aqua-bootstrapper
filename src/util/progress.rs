use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

const DRAW_INTERVAL: Duration = Duration::from_millis(100);

pub struct Progress {
    label: String,
    total: Option<u64>,
    current: u64,
    interactive: bool,
    last_draw: Instant,
    drawn: bool,
    finished: bool,
}

impl Progress {
    pub fn new(label: impl Into<String>, total: Option<u64>) -> Self {
        let label = label.into();
        eprintln!("{label}...");

        Self {
            label,
            total,
            current: 0,
            interactive: io::stderr().is_terminal(),
            last_draw: Instant::now() - DRAW_INTERVAL,
            drawn: false,
            finished: false,
        }
    }

    pub fn advance(&mut self, amount: u64) {
        self.current += amount;
        self.draw(false);
    }

    pub fn finish(&mut self, message: impl AsRef<str>) {
        self.draw(true);
        if self.drawn {
            eprintln!();
        }
        eprintln!("{}", message.as_ref());
        self.finished = true;
    }

    fn draw(&mut self, force: bool) {
        if !self.interactive {
            return;
        }

        let now = Instant::now();
        if !force && now.duration_since(self.last_draw) < DRAW_INTERVAL {
            return;
        }

        match self.total {
            Some(total) if total > 0 => {
                let percent = (self.current as f64 / total as f64 * 100.0).min(100.0);
                eprint!(
                    "\r{}: {} / {} ({percent:.1}%)",
                    self.label,
                    format_bytes(self.current),
                    format_bytes(total)
                );
            }
            _ => {
                eprint!("\r{}: {}", self.label, format_bytes(self.current));
            }
        }

        let _ = io::stderr().flush();
        self.last_draw = now;
        self.drawn = true;
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        if self.drawn && !self.finished {
            eprintln!();
        }
    }
}

pub fn step(message: impl AsRef<str>) {
    eprintln!("{}", message.as_ref());
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::format_bytes;

    #[test]
    fn formats_byte_counts() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(42), "42 B");
        assert_eq!(format_bytes(1536), "1.5 KiB");
    }
}
