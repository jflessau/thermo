use std::env;
use std::io::Write;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};
use tracing::info;

const PORT_CANDIDATES: [&str; 4] = ["/dev/serial0", "/dev/ttyAMA0", "/dev/ttyS0", "/dev/ttyUSB0"];
const BAUDRATE: u32 = 9600;
const TIMEOUT: Duration = Duration::from_secs(1);
pub const MAX_CHARS_PER_LINE: usize = 27;

pub struct Printer {
    port: Box<dyn SerialPort>,
}

impl Printer {
    pub fn connect() -> Result<Self> {
        let env_port = env::var("THERMO_PORT").ok();
        let candidates: Vec<&str> = match env_port.as_deref() {
            Some(port) => vec![port],
            None => PORT_CANDIDATES.to_vec(),
        };

        let mut last_error = None;

        for port in candidates {
            info!("trying serial port: {}", port);

            match serialport::new(port, BAUDRATE)
                .data_bits(DataBits::Eight)
                .parity(Parity::None)
                .stop_bits(StopBits::One)
                .flow_control(FlowControl::None)
                .timeout(TIMEOUT)
                .open()
            {
                Ok(serial_port) => {
                    info!("opened serial port: {}", port);
                    let mut printer = Self { port: serial_port };
                    printer.wake()?;
                    return Ok(printer);
                }
                Err(err) => {
                    info!("failed to open {}: {}", port, err);
                    last_error = Some(err);
                }
            }
        }

        match last_error {
            Some(err) => bail!("could not open any serial port: {}", err),
            None => bail!("no serial ports configured"),
        }
    }

    pub fn write_line(&mut self, line: &str) -> Result<()> {
        self.port
            .write_all(line.as_bytes())
            .context("failed to write line bytes to printer")?;
        self.port
            .write_all(b"\r\n")
            .context("failed to write line ending to printer")?;
        self.port
            .flush()
            .context("failed to flush printer output")?;
        thread::sleep(Duration::from_millis(150));
        Ok(())
    }

    pub fn write_text(&mut self, text: &str) -> Result<()> {
        let normalized = normalize_text(text);

        for paragraph in normalized.split('\n') {
            if paragraph.trim().is_empty() {
                self.write_line("")?;
                continue;
            }

            for line in wrap_text(paragraph, MAX_CHARS_PER_LINE) {
                self.write_line(&line)?;
            }
        }

        Ok(())
    }

    pub fn feed(&mut self, lines: usize) -> Result<()> {
        for _ in 0..lines {
            self.write_line("")?;
        }
        Ok(())
    }

    fn wake(&mut self) -> Result<()> {
        thread::sleep(Duration::from_secs(1));
        self.port
            .write_all(b"\r\n")
            .context("failed to wake printer")?;
        self.port.flush().context("failed to flush wake bytes")?;
        thread::sleep(Duration::from_millis(300));
        Ok(())
    }
}

fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.is_empty() {
            if char_count(word) <= max_chars {
                current.push_str(word);
            } else {
                lines.extend(split_long_word(word, max_chars));
            }
            continue;
        }

        let candidate_len = char_count(&current) + 1 + char_count(word);
        if candidate_len <= max_chars {
            current.push(' ');
            current.push_str(word);
            continue;
        }

        lines.push(current);
        current = String::new();

        if char_count(word) <= max_chars {
            current.push_str(word);
        } else {
            let mut parts = split_long_word(word, max_chars);
            if let Some(last) = parts.pop() {
                lines.extend(parts);
                current = last;
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

fn split_long_word(word: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return vec![word.to_string()];
    }

    let mut parts = Vec::new();
    let mut current = String::new();

    for ch in word.chars() {
        current.push(ch);
        if current.chars().count() == max_chars {
            parts.push(current);
            current = String::new();
        }
    }

    if !current.is_empty() {
        parts.push(current);
    }

    parts
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn normalize_text(text: &str) -> String {
    let mut normalized = String::new();

    for ch in text.chars() {
        match ch {
            'ä' => normalized.push_str("ae"),
            'ö' => normalized.push_str("oe"),
            'ü' => normalized.push_str("ue"),
            'Ä' => normalized.push_str("AE"),
            'Ö' => normalized.push_str("OE"),
            'Ü' => normalized.push_str("UE"),
            'ß' => normalized.push_str("ss"),
            '\n' => normalized.push('\n'),
            '\r' => {}
            ch if ch.is_ascii() => normalized.push(ch),
            _ => normalized.push('?'),
        }
    }

    normalized
}
