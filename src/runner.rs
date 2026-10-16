//! Runs xtool inside a pseudo-terminal so it behaves exactly as in a shell:
//! line-buffered output, progress lines, hidden password input and
//! interactive prompts (2FA codes, team selection, file paths).

use crate::term::{Console, LineKind};
use crate::xtool::Invocation;
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Answers a prompt automatically, once, when the last output line ends with it.
pub struct Responder {
    pub prompt: &'static str,
    pub answer: String,
    /// Only sent once the terminal has echo turned off.
    pub secret: bool,
}

pub enum Exit {
    Ok,
    Failed(String),
    Stopped,
}

struct Shared {
    console: Arc<Mutex<Console>>,
    responders: Vec<Responder>,
    writer: Box<dyn Write + Send>,
}

pub struct Job {
    started: Instant,
    shared: Arc<Mutex<Shared>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    #[cfg(unix)]
    pid: Option<u32>,
    done: mpsc::Receiver<Result<u32, String>>,
    stopped: bool,
    finished: bool,
    _master: Box<dyn MasterPty + Send>,
}

pub struct Finished {
    pub exit: Exit,
    pub output: String,
}

impl Job {
    pub fn start(
        inv: Invocation,
        responders: Vec<Responder>,
        console: Arc<Mutex<Console>>,
        ctx: egui::Context,
    ) -> Result<Self, String> {
        {
            let mut c = console.lock().unwrap();
            c.push(LineKind::Cmd, format!("$ {}", inv.display));
            c.start_capture();
        }
        let fail = |e: String| {
            let mut c = console.lock().unwrap();
            c.push(LineKind::Err, e.clone());
            c.finish_capture();
            e
        };

        let pair = native_pty_system()
            .openpty(PtySize { rows: 50, cols: 220, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| fail(format!("Could not open a terminal: {e}")))?;
        let mut cmd = CommandBuilder::new(&inv.program);
        cmd.args(&inv.args);
        if let Some(dir) = &inv.cwd {
            cmd.cwd(dir);
        }
        if let Some(p) = &inv.path_var {
            cmd.env("PATH", p);
        }
        cmd.env("TERM", "dumb");
        cmd.env("NO_COLOR", "1");
        let child = pair.slave.spawn_command(cmd).map_err(|e| fail(format!("Could not start {}: {e}", inv.program)))?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| fail(e.to_string()))?;
        let writer = pair.master.take_writer().map_err(|e| fail(e.to_string()))?;
        let killer = child.clone_killer();
        #[cfg(unix)]
        let pid = child.process_id();
        #[cfg(unix)]
        let tty_fd = pair.master.as_raw_fd();
        #[cfg(not(unix))]
        let tty_fd: Option<i32> = None;
        let shared = Arc::new(Mutex::new(Shared { console, responders, writer }));

        let (read_done_tx, read_done_rx) = mpsc::channel::<()>();
        let sh = shared.clone();
        let rctx = ctx.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut s = sh.lock().unwrap();
                        let partial = {
                            let mut c = s.console.lock().unwrap();
                            c.feed(&buf[..n]);
                            c.partial()
                        };
                        let hit = s.responders.iter().position(|r| partial.ends_with(r.prompt));
                        let r = hit.map(|i| s.responders.remove(i));
                        drop(s);
                        if let Some(r) = r
                            && (!r.secret || echo_off(tty_fd))
                        {
                            let mut s = sh.lock().unwrap();
                            let _ = write!(s.writer, "{}\r", r.answer);
                            let _ = s.writer.flush();
                        }
                        rctx.request_repaint();
                    }
                }
            }
            let _ = read_done_tx.send(());
        });

        let (done_tx, done) = mpsc::channel();
        let mut child = child;
        std::thread::spawn(move || {
            let status = child.wait();
            // Let the reader drain whatever is left before reporting.
            let _ = read_done_rx.recv_timeout(Duration::from_millis(700));
            let _ = done_tx.send(match status {
                Ok(s) => Ok(s.exit_code()),
                Err(e) => Err(e.to_string()),
            });
            ctx.request_repaint();
        });

        Ok(Self {
            started: Instant::now(),
            shared,
            killer,
            #[cfg(unix)]
            pid,
            done,
            stopped: false,
            finished: false,
            _master: pair.master,
        })
    }

    /// Sends a line of input to the process.
    pub fn send_line(&self, text: &str) {
        let mut s = self.shared.lock().unwrap();
        let _ = write!(s.writer, "{text}\r");
        let _ = s.writer.flush();
    }

    pub fn stop(&mut self) {
        if self.finished {
            return;
        }
        self.stopped = true;
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            // The child leads its own session; signal the whole group so
            // compilers it spawned go away too.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
        }
        let _ = self.killer.kill();
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Returns the result once the process has exited.
    pub fn poll(&mut self) -> Option<Finished> {
        if self.finished {
            return None;
        }
        let res = match self.done.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err("lost track of process".into()),
        };
        self.finished = true;
        let secs = self.started.elapsed().as_secs_f32();
        let exit = match res {
            _ if self.stopped => Exit::Stopped,
            Ok(0) => Exit::Ok,
            Ok(code) => Exit::Failed(format!("exited with code {code}")),
            Err(e) => Exit::Failed(e),
        };
        let shared = self.shared.lock().unwrap();
        let mut c = shared.console.lock().unwrap();
        let output = c.finish_capture();
        match &exit {
            Exit::Ok => c.push(LineKind::Ok, format!("✔ Done in {secs:.1}s")),
            Exit::Failed(e) => c.push(LineKind::Err, format!("✖ Failed: {e} ({secs:.1}s)")),
            Exit::Stopped => c.push(LineKind::Info, "■ Stopped"),
        }
        drop(c);
        drop(shared);
        Some(Finished { exit, output })
    }
}

/// Waits briefly for the program to turn terminal echo off, so a secret
/// is never echoed back into the log. Gives up (returns false) after ~1.5s.
#[cfg(unix)]
fn echo_off(fd: Option<i32>) -> bool {
    let Some(fd) = fd else { return false };
    for _ in 0..75 {
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut t) } != 0 {
            return false;
        }
        if t.c_lflag & libc::ECHO == 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}
#[cfg(not(unix))]
fn echo_off(_: Option<i32>) -> bool {
    true
}

impl Drop for Job {
    fn drop(&mut self) {
        if !self.finished && self.done.try_recv().is_err() {
            self.stop();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn wait(job: &mut Job) -> Finished {
        let t = Instant::now();
        loop {
            if let Some(f) = job.poll() {
                return f;
            }
            assert!(t.elapsed() < Duration::from_secs(10), "job timed out");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_partial(console: &Arc<Mutex<Console>>, suffix: &str) {
        let t = Instant::now();
        while !console.lock().unwrap().partial().ends_with(suffix) {
            assert!(t.elapsed() < Duration::from_secs(10), "no prompt {suffix:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn interactive_prompts() {
        let script = r#"printf 'Password: '; stty -echo; read -r pw; stty echo; echo
printf 'Code: '; read -r code
printf '0: A\n1: B\nChoice (0-1): '; read -r c
echo "pw=$pw code=$code choice=$c""#;
        let inv = Invocation {
            program: "/bin/bash".into(),
            args: vec!["-c".into(), script.into()],
            cwd: None,
            path_var: None,
            display: "test".into(),
        };
        let console = Arc::new(Mutex::new(Console::default()));
        let responders = vec![Responder { prompt: "Password: ", answer: "s3cret".into(), secret: true }];
        let mut job = Job::start(inv, responders, console.clone(), egui::Context::default()).unwrap();
        wait_partial(&console, "Code: ");
        job.send_line("123456");
        wait_partial(&console, "Choice (0-1): ");
        assert_eq!(console.lock().unwrap().choices().unwrap().len(), 2);
        job.send_line("1");
        let fin = wait(&mut job);
        assert!(matches!(fin.exit, Exit::Ok));
        assert!(fin.output.contains("pw=s3cret code=123456 choice=1"), "{}", fin.output);
        // The password was typed with echo off, so it never shows up as typed text.
        assert_eq!(fin.output.matches("s3cret").count(), 1);
    }

    #[test]
    fn stop_kills_process() {
        let inv = Invocation {
            program: "/bin/sleep".into(),
            args: vec!["30".into()],
            cwd: None,
            path_var: None,
            display: "sleep".into(),
        };
        let console = Arc::new(Mutex::new(Console::default()));
        let mut job = Job::start(inv, vec![], console, egui::Context::default()).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        job.stop();
        assert!(matches!(wait(&mut job).exit, Exit::Stopped));
    }
}
