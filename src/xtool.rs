//! Everything that knows how to talk to the `xtool` executable:
//! locating it, building invocations, and parsing its output.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// How `xtool` is reached on this machine.
#[derive(Clone, Debug)]
pub struct Env {
    /// Path to the xtool binary (inside WSL when `wsl` is set).
    pub xtool: String,
    /// PATH to give child processes (login-shell PATH merged with ours).
    pub path_var: Option<String>,
    /// Route everything through `wsl.exe` (Windows).
    pub wsl: bool,
}

pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub path_var: Option<String>,
    /// What gets printed in the console header.
    pub display: String,
}

impl Env {
    pub fn invocation(&self, args: &[String], cwd: Option<&Path>) -> Invocation {
        let display =
            std::iter::once("xtool".to_string()).chain(args.iter().map(|a| quote(a))).collect::<Vec<_>>().join(" ");
        if self.wsl {
            let mut a = Vec::new();
            if let Some(dir) = cwd {
                a.push("--cd".into());
                a.push(dir.display().to_string());
            }
            a.extend(["--exec", "bash", "-lc", r#"exec "$0" "$@""#].map(String::from));
            a.push(self.xtool.clone());
            a.extend(args.iter().cloned());
            Invocation { program: "wsl.exe".into(), args: a, cwd: None, path_var: None, display }
        } else {
            Invocation {
                program: self.xtool.clone(),
                args: args.to_vec(),
                cwd: cwd.map(Path::to_path_buf),
                path_var: self.path_var.clone(),
                display,
            }
        }
    }

    /// Converts a local path into one xtool can read (translates for WSL).
    pub fn arg_path(&self, p: &Path) -> String {
        let s = p.display().to_string();
        if !self.wsl {
            return s;
        }
        let b = s.as_bytes();
        if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
            let rest = s[2..].replace('\\', "/");
            return format!("/mnt/{}{}", (b[0] as char).to_ascii_lowercase(), rest);
        }
        s
    }

    /// Converts a path printed by xtool back into a local one.
    pub fn local_path(&self, s: &str) -> PathBuf {
        if self.wsl
            && let Some(out) = quiet_raw("wsl.exe", &["wslpath", "-w", s], None)
        {
            let out = out.trim();
            if !out.is_empty() {
                return PathBuf::from(out);
            }
        }
        PathBuf::from(s)
    }

    /// Runs xtool without a terminal and returns its combined output.
    pub fn quiet(&self, args: &[&str]) -> Result<String, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let inv = self.invocation(&args, None);
        let mut cmd = Command::new(&inv.program);
        cmd.args(&inv.args).stdin(Stdio::null());
        if let Some(p) = &inv.path_var {
            cmd.env("PATH", p);
        }
        no_window(&mut cmd);
        let out = cmd.output().map_err(|e| format!("Could not run {}: {e}", inv.program))?;
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        if out.status.success() { Ok(text) } else { Err(text) }
    }
}

fn quote(a: &str) -> String {
    if !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@+,".contains(c)) {
        a.to_string()
    } else {
        format!("'{}'", a.replace('\'', r"'\''"))
    }
}

fn quiet_raw(program: &str, args: &[&str], path_var: Option<&str>) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stderr(Stdio::null());
    if let Some(p) = path_var {
        cmd.env("PATH", p);
    }
    no_window(&mut cmd);
    let out = cmd.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}
#[cfg(not(windows))]
fn no_window(_: &mut Command) {}

/// Asks the user's login shell for its PATH, so tools installed via
/// shell profiles (swiftly, ~/.local/bin, Homebrew) are visible even when
/// the GUI was started from a desktop launcher.
#[cfg(unix)]
pub fn login_path() -> Option<String> {
    use std::io::Read;
    use std::time::Duration;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let mut child = Command::new(shell)
        .args(["-l", "-c", r#"printf '__XG__%s__XG__' "$PATH""#])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < Duration::from_secs(4) => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    let shell_path = out.split("__XG__").nth(1)?.to_string();
    let mut parts: Vec<String> = shell_path.split(':').map(String::from).collect();
    if let Ok(cur) = std::env::var("PATH") {
        for p in cur.split(':') {
            if !parts.iter().any(|x| x == p) {
                parts.push(p.into());
            }
        }
    }
    Some(parts.join(":"))
}
#[cfg(not(unix))]
pub fn login_path() -> Option<String> {
    None
}

/// Finds the xtool executable. `configured` wins when non-empty.
pub fn locate(configured: &str, wsl: bool, path_var: Option<&str>) -> Option<String> {
    if !configured.trim().is_empty() {
        return Some(configured.trim().to_string());
    }
    if wsl {
        let out = quiet_raw("wsl.exe", &["--exec", "bash", "-lc", "command -v xtool"], None)?;
        let p = out.trim();
        return (!p.is_empty()).then(|| p.to_string());
    }
    let exe = if cfg!(windows) { "xtool.exe" } else { "xtool" };
    let path = path_var.map(String::from).or_else(|| std::env::var("PATH").ok()).unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    if let Some(home) = home_dir() {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join("bin"));
    }
    for d in ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin", "/Applications/xtool.app/Contents/MacOS"] {
        dirs.push(d.into());
    }
    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.is_file()).map(|p| p.display().to_string())
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

// ---------------------------------------------------------------- parsing

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub name: String,
    pub connection: String,
    pub udid: String,
}

/// Parses `xtool devices` output: `Name [usb]: UDID`.
pub fn parse_devices(out: &str) -> Vec<Device> {
    out.lines()
        .filter_map(|l| {
            let (head, udid) = l.trim().rsplit_once("]: ")?;
            let (name, conn) = head.rsplit_once(" [")?;
            Some(Device { name: name.into(), connection: conn.into(), udid: udid.trim().into() })
        })
        .collect()
}

/// Finds the artifact path in `Wrote to <path>`.
pub fn parse_wrote_to(out: &str) -> Option<String> {
    out.lines().rev().find_map(|l| l.trim().strip_prefix("Wrote to ").map(|s| s.trim().to_string()))
}

/// Reads `bundleID:` from an xtool.yml document.
pub fn bundle_id(yml: &str) -> Option<String> {
    yml.lines().find_map(|l| {
        let v = l.trim().strip_prefix("bundleID:")?.trim().trim_matches(['"', '\'']);
        (!v.is_empty()).then(|| v.to_string())
    })
}

/// Splits a launch-arguments string like a shell would (quotes, escapes).
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut quote: Option<char> = None;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') | (None, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                has = true;
            }
            (None, c) if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ---------------------------------------------------------------- desktop

/// Opens a file or folder with the system default handler.
pub fn open(p: &Path) {
    let (prog, args): (&str, Vec<String>) = if cfg!(target_os = "macos") {
        ("open", vec![p.display().to_string()])
    } else if cfg!(windows) {
        ("explorer", vec![p.display().to_string()])
    } else {
        ("xdg-open", vec![p.display().to_string()])
    };
    spawn_detached(prog, &args);
}

/// Shows a file in the system file manager.
pub fn reveal(p: &Path) {
    if cfg!(target_os = "macos") {
        spawn_detached("open", &["-R".into(), p.display().to_string()]);
    } else if cfg!(windows) {
        spawn_detached("explorer", &[format!("/select,{}", p.display())]);
    } else {
        open(p.parent().unwrap_or(p));
    }
}

pub fn spawn_detached(prog: &str, args: &[String]) {
    let mut cmd = Command::new(prog);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    no_window(&mut cmd);
    if let Ok(mut child) = cmd.spawn() {
        std::thread::spawn(move || child.wait());
    }
}

/// Copies a file or directory (`.app` bundles are directories).
pub fn copy_any(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_any(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices() {
        let d = parse_devices("Waiting...\nMy iPhone [usb]: 00008110-001\niPad [network]: abc\n");
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].name, "My iPhone");
        assert_eq!(d[0].connection, "usb");
        assert_eq!(d[1].udid, "abc");
    }

    #[test]
    fn args() {
        assert_eq!(split_args(r#"-a "b c" 'd e' f\ g"#), vec!["-a", "b c", "d e", "f g"]);
        assert_eq!(split_args(r#"x """#), vec!["x", ""]);
    }

    #[test]
    fn yml() {
        assert_eq!(bundle_id("version: 1\nbundleID: com.example.App\n").as_deref(), Some("com.example.App"));
    }

    #[test]
    fn wsl_paths() {
        let e = Env { xtool: "xtool".into(), path_var: None, wsl: true };
        assert_eq!(e.arg_path(Path::new(r"C:\Users\me\a.ipa")), "/mnt/c/Users/me/a.ipa");
    }
}
