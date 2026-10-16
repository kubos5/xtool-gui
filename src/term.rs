//! Turns raw pseudo-terminal output into plain lines.
//!
//! Handles carriage returns (progress lines overwrite themselves), backspace,
//! and strips ANSI/VT escape sequences. Partial UTF-8 sequences split across
//! reads are kept as bytes until the line completes.

use std::collections::VecDeque;

const MAX_LINES: usize = 20_000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Out,
    Cmd,
    Ok,
    Err,
    Info,
}

pub struct Line {
    pub text: String,
    pub kind: LineKind,
}

#[derive(Clone, Copy)]
enum Esc {
    None,
    Start,
    Csi,
    Osc,
    OscEnd,
    Skip,
}

pub struct Console {
    pub lines: VecDeque<Line>,
    partial: Vec<u8>,
    esc: Esc,
    pending_cr: bool,
    /// Completed lines of the active job, used for parsing results.
    capture: Option<String>,
    /// Bumped whenever the content changes.
    pub generation: u64,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            lines: VecDeque::new(),
            partial: Vec::new(),
            esc: Esc::None,
            pending_cr: false,
            capture: None,
            generation: 0,
        }
    }
}

impl Console {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            match self.esc {
                Esc::Start => {
                    self.esc = match b {
                        b'[' => Esc::Csi,
                        b']' => Esc::Osc,
                        b'(' | b')' | b'*' | b'+' | b'#' | b'%' => Esc::Skip,
                        _ => Esc::None,
                    };
                    continue;
                }
                Esc::Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        self.esc = Esc::None;
                    }
                    continue;
                }
                Esc::Osc => {
                    match b {
                        0x07 => self.esc = Esc::None,
                        0x1b => self.esc = Esc::OscEnd,
                        _ => {}
                    }
                    continue;
                }
                Esc::OscEnd => {
                    self.esc = if b == b'\\' { Esc::None } else { Esc::Osc };
                    continue;
                }
                Esc::Skip => {
                    self.esc = Esc::None;
                    continue;
                }
                Esc::None => {}
            }
            match b {
                0x1b => self.esc = Esc::Start,
                b'\r' => self.pending_cr = true,
                b'\n' => {
                    self.pending_cr = false;
                    self.end_line();
                }
                0x08 => {
                    self.partial.pop();
                }
                b'\t' => {
                    self.take_cr();
                    self.partial.extend_from_slice(b"    ");
                }
                0x00..=0x1f | 0x7f => {}
                _ => {
                    self.take_cr();
                    self.partial.push(b);
                }
            }
        }
        self.generation += 1;
    }

    fn take_cr(&mut self) {
        if self.pending_cr {
            self.pending_cr = false;
            self.partial.clear();
        }
    }

    fn end_line(&mut self) {
        let text = String::from_utf8_lossy(&self.partial).into_owned();
        self.partial.clear();
        if let Some(c) = &mut self.capture {
            c.push_str(&text);
            c.push('\n');
        }
        self.push_raw(LineKind::Out, text);
    }

    fn push_raw(&mut self, kind: LineKind, text: String) {
        if self.lines.len() >= MAX_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(Line { text, kind });
    }

    /// Adds a line written by the GUI itself (not by the process).
    pub fn push(&mut self, kind: LineKind, text: impl Into<String>) {
        self.flush_partial();
        self.push_raw(kind, text.into());
        self.generation += 1;
    }

    fn flush_partial(&mut self) {
        if !self.partial.is_empty() {
            self.end_line();
        }
        self.pending_cr = false;
    }

    /// The incomplete last line (usually a prompt waiting for input).
    pub fn partial(&self) -> String {
        if self.pending_cr {
            return String::new();
        }
        String::from_utf8_lossy(&self.partial).into_owned()
    }

    pub fn start_capture(&mut self) {
        self.capture = Some(String::new());
    }

    pub fn finish_capture(&mut self) -> String {
        self.flush_partial();
        self.capture.take().unwrap_or_default()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.partial.clear();
        self.pending_cr = false;
        self.generation += 1;
    }

    pub fn plain_text(&self) -> String {
        let mut s = String::new();
        for l in &self.lines {
            s.push_str(&l.text);
            s.push('\n');
        }
        s.push_str(&self.partial());
        s
    }

    /// Options listed before an interactive `Choice (a-b): ` prompt.
    pub fn choices(&self) -> Option<Vec<(usize, String)>> {
        let p = self.partial();
        let inner = p.trim_end().strip_prefix("Choice (")?.strip_suffix("):")?;
        let (a, b) = inner.split_once('-')?;
        let (a, b): (usize, usize) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
        if b < a || b - a > 200 {
            return None;
        }
        // Walk back to the most recent "a: ..." line, then read forward.
        let first = format!("{a}: ");
        let start = self.lines.iter().rposition(|l| l.text.starts_with(&first))?;
        let mut out = Vec::new();
        for (i, line) in self.lines.iter().skip(start).enumerate() {
            let n = a + i;
            if n > b {
                break;
            }
            let label = line.text.strip_prefix(&format!("{n}: "))?;
            out.push((n, label.to_string()));
        }
        (out.len() == b - a + 1).then_some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_escapes_and_handles_cr() {
        let mut c = Console::default();
        c.feed(b"\x1b[1mBold\x1b[0m line\r\nProg 1%\rProg 50%\rProg 100%\r\n");
        assert_eq!(c.lines[0].text, "Bold line");
        assert_eq!(c.lines[1].text, "Prog 100%");
    }

    #[test]
    fn split_utf8_and_partial() {
        let mut c = Console::default();
        let s = "héllo".as_bytes();
        c.feed(&s[..2]);
        c.feed(&s[2..]);
        assert_eq!(c.partial(), "héllo");
    }

    #[test]
    fn parses_choices() {
        let mut c = Console::default();
        c.feed(b"Select a team\r\n0: Alpha (AAA)\r\n1: Beta (BBB)\r\nChoice (0-1): ");
        let ch = c.choices().unwrap();
        assert_eq!(ch, vec![(0, "Alpha (AAA)".into()), (1, "Beta (BBB)".into())]);
    }
}
