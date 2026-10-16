use crate::runner::{Exit, Job, Responder};
use crate::term::{Console, LineKind};
use crate::theme::{self, ACCENT, ERR, OK, WARN};
use crate::xtool::{self, Device, Env, Invocation};
use egui::{
    Align, CentralPanel, Color32, ComboBox, Frame, Key, KeyboardShortcut, Layout, Margin, Modifiers, Panel, RichText,
    ScrollArea, TextEdit, TextStyle, TextWrapMode, Ui,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

// ------------------------------------------------------------------ config

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Project,
    Devices,
    Account,
    Sdk,
    Developer,
    Settings,
}

impl Page {
    const ALL: [Page; 6] = [Page::Project, Page::Devices, Page::Account, Page::Sdk, Page::Developer, Page::Settings];
    fn label(self) -> &'static str {
        match self {
            Page::Project => "Project",
            Page::Devices => "Devices",
            Page::Account => "Account",
            Page::Sdk => "SDK",
            Page::Developer => "Developer Services",
            Page::Settings => "Settings",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
enum ThemeChoice {
    System,
    Dark,
    Light,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
enum Search {
    All,
    Usb,
    Network,
}

impl Search {
    fn flag(self) -> &'static str {
        match self {
            Search::All => "--all",
            Search::Usb => "--usb",
            Search::Network => "--network",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
enum LoginMode {
    Password,
    Key,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
enum Arch {
    Auto,
    Arm64,
    X86_64,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Config {
    xtool_path: String,
    use_wsl: bool,
    theme: ThemeChoice,
    zoom: f32,
    page: Page,
    project: Option<PathBuf>,
    recent: Vec<PathBuf>,

    release: bool,
    simulator: bool,
    sign: bool,
    ipa: bool,
    triple: String,
    launch_after: bool,

    search: Search,
    device: Option<String>,
    launch_bundle: String,
    launch_args: String,
    uninstall_bundle: String,

    login_mode: LoginMode,
    apple_id: String,
    key_id: String,
    issuer_id: String,
    key_path: String,
    reset_2fa: bool,

    new_name: String,
    new_dir: String,
    new_setup: bool,

    sdk_source: String,
    sdk_slim: bool,
    sdk_build_src: String,
    sdk_build_out: String,
    sdk_arch: Arch,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            xtool_path: String::new(),
            use_wsl: cfg!(windows),
            theme: ThemeChoice::Dark,
            zoom: 1.0,
            page: Page::Project,
            project: None,
            recent: Vec::new(),
            release: false,
            simulator: false,
            sign: false,
            ipa: false,
            triple: String::new(),
            launch_after: true,
            search: Search::All,
            device: None,
            launch_bundle: String::new(),
            launch_args: String::new(),
            uninstall_bundle: String::new(),
            login_mode: LoginMode::Password,
            apple_id: String::new(),
            key_id: String::new(),
            issuer_id: String::new(),
            key_path: String::new(),
            reset_2fa: false,
            new_name: String::new(),
            new_dir: xtool::home_dir().map(|h| h.display().to_string()).unwrap_or_default(),
            new_setup: false,
            sdk_source: String::new(),
            sdk_slim: false,
            sdk_build_src: String::new(),
            sdk_build_out: String::new(),
            sdk_arch: Arch::Auto,
        }
    }
}

// ------------------------------------------------------------------ state

enum Msg {
    Env(Option<String>, Option<String>),
    Version(Result<String, String>),
    Auth(Result<String, String>),
    Sdk(Result<String, String>),
    Devices(Result<Vec<Device>, String>),
}

/// What a running job is, so its result can be acted on.
enum Kind {
    Plain,
    Auth,
    Sdk,
    Build { export: bool },
    Run { launch: Option<String> },
    RunSimulator { launch: Option<String> },
    New(PathBuf),
    Xcode,
    Ds(&'static str),
}

#[derive(Default)]
struct Loaded<T> {
    loading: bool,
    value: Option<Result<T, String>>,
}

struct Yml {
    path: PathBuf,
    text: String,
    saved: String,
}

pub struct App {
    cfg: Config,
    env: Option<Env>,
    path_var: Option<String>,
    detecting: bool,
    version: Loaded<String>,
    auth: Loaded<String>,
    sdk: Loaded<String>,
    devices: Loaded<Vec<Device>>,

    console: Arc<Mutex<Console>>,
    job: Option<(Kind, Job)>,
    job_title: String,
    input: String,
    focus_input: bool,
    last_partial: String,

    password: String,
    yml: Option<Yml>,
    artifact: Option<PathBuf>,
    ds: (&'static str, String),
    armed: Option<&'static str>,
    notice: Option<(Color32, String)>,

    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
}

const CONFIG_KEY: &str = "xtool-gui";

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let cfg: Config = cc.storage.and_then(|s| eframe::get_value(s, CONFIG_KEY)).unwrap_or_default();
        theme::apply(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            cfg,
            env: None,
            path_var: None,
            detecting: false,
            version: Loaded::default(),
            auth: Loaded::default(),
            sdk: Loaded::default(),
            devices: Loaded::default(),
            console: Arc::default(),
            job: None,
            job_title: String::new(),
            input: String::new(),
            focus_input: false,
            last_partial: String::new(),
            password: String::new(),
            yml: None,
            artifact: None,
            ds: ("", String::new()),
            armed: None,
            notice: None,
            tx,
            rx,
        };
        app.apply_appearance(&cc.egui_ctx);
        if let Some(p) = app.cfg.project.clone() {
            app.open_project(p);
        }
        app.detect(&cc.egui_ctx, true);
        app
    }

    fn apply_appearance(&self, ctx: &egui::Context) {
        ctx.set_theme(match self.cfg.theme {
            ThemeChoice::System => egui::ThemePreference::System,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
            ThemeChoice::Light => egui::ThemePreference::Light,
        });
        ctx.set_zoom_factor(self.cfg.zoom);
    }

    // -------------------------------------------------------- background

    /// Resolves the shell PATH (first time only) and locates xtool.
    fn detect(&mut self, ctx: &egui::Context, resolve_shell: bool) {
        self.detecting = true;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        let (configured, wsl, known) = (self.cfg.xtool_path.clone(), self.cfg.use_wsl, self.path_var.clone());
        std::thread::spawn(move || {
            let path_var = if resolve_shell { xtool::login_path() } else { known };
            let found = xtool::locate(&configured, wsl, path_var.as_deref());
            let _ = tx.send(Msg::Env(path_var, found));
            ctx.request_repaint();
        });
    }

    fn bg(&self, ctx: &egui::Context, f: impl FnOnce(&Env) -> Msg + Send + 'static) {
        let Some(env) = self.env.clone() else { return };
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(f(&env));
            ctx.request_repaint();
        });
    }

    fn refresh_auth(&mut self, ctx: &egui::Context) {
        self.auth.loading = true;
        self.bg(ctx, |e| Msg::Auth(e.quiet(&["auth", "status"])));
    }

    fn refresh_sdk(&mut self, ctx: &egui::Context) {
        self.sdk.loading = true;
        self.bg(ctx, |e| Msg::Sdk(e.quiet(&["sdk", "status"])));
    }

    fn refresh_devices(&mut self, ctx: &egui::Context) {
        self.devices.loading = true;
        let flag = self.cfg.search.flag();
        self.bg(ctx, move |e| Msg::Devices(e.quiet(&["devices", "--no-wait", flag]).map(|o| xtool::parse_devices(&o))));
    }

    fn refresh_all(&mut self, ctx: &egui::Context) {
        if self.env.is_none() {
            return;
        }
        self.version.loading = true;
        self.bg(ctx, |e| Msg::Version(e.quiet(&["--version"]).map(|s| s.trim().to_string())));
        self.refresh_auth(ctx);
        self.refresh_sdk(ctx);
        self.refresh_devices(ctx);
    }

    fn pump(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Env(path_var, found) => {
                    self.detecting = false;
                    self.path_var = path_var.clone();
                    self.env = found.map(|xtool| Env { xtool, path_var, wsl: cfg!(windows) && self.cfg.use_wsl });
                    if self.env.is_none() {
                        self.version = Loaded { loading: false, value: Some(Err("not found".into())) };
                    }
                    self.refresh_all(ctx);
                    #[cfg(debug_assertions)]
                    self.debug_action(ctx);
                }
                Msg::Version(r) => self.version = Loaded { loading: false, value: Some(r) },
                Msg::Auth(r) => self.auth = Loaded { loading: false, value: Some(r) },
                Msg::Sdk(r) => self.sdk = Loaded { loading: false, value: Some(r) },
                Msg::Devices(r) => self.devices = Loaded { loading: false, value: Some(r) },
            }
        }

        let finished = match &mut self.job {
            Some((_, job)) => job.poll(),
            None => None,
        };
        if let Some(fin) = finished {
            let (kind, _) = self.job.take().unwrap();
            self.on_finished(ctx, kind, fin.exit, fin.output);
        }
    }

    // -------------------------------------------------------------- jobs

    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn start(&mut self, ctx: &egui::Context, kind: Kind, args: Vec<String>, cwd: Option<&Path>) {
        self.start_with(ctx, kind, args, cwd, Vec::new());
    }

    fn start_with(
        &mut self,
        ctx: &egui::Context,
        kind: Kind,
        args: Vec<String>,
        cwd: Option<&Path>,
        responders: Vec<Responder>,
    ) {
        let Some(env) = &self.env else {
            self.notice = Some((ERR, "xtool was not found. Set its location in Settings.".into()));
            return;
        };
        let inv = env.invocation(&args, cwd);
        self.launch(ctx, kind, inv, responders);
    }

    fn launch(&mut self, ctx: &egui::Context, kind: Kind, inv: Invocation, responders: Vec<Responder>) {
        if self.busy() {
            return;
        }
        self.notice = None;
        self.armed = None;
        self.input.clear();
        self.job_title = inv.display.clone();
        if let Ok(job) = Job::start(inv, responders, self.console.clone(), ctx.clone()) {
            self.job = Some((kind, job));
        }
    }

    fn on_finished(&mut self, ctx: &egui::Context, kind: Kind, exit: Exit, output: String) {
        let ok = matches!(exit, Exit::Ok);
        match kind {
            Kind::Plain => {}
            Kind::Auth => {
                self.password.clear();
                self.refresh_auth(ctx);
                self.refresh_sdk(ctx);
            }
            Kind::Sdk => self.refresh_sdk(ctx),
            Kind::Build { export } => {
                if let (true, Some(p)) = (ok, xtool::parse_wrote_to(&output)) {
                    let local = self.local(&p);
                    self.artifact = Some(local.clone());
                    if export {
                        self.save_copy(&local);
                    }
                }
            }
            Kind::Run { launch } if ok => {
                if let Some(bundle) = launch {
                    let mut args = vec!["launch".into()];
                    args.extend(self.device_args());
                    args.push(bundle);
                    let cwd = self.cfg.project.clone();
                    self.start(ctx, Kind::Plain, args, cwd.as_deref());
                }
            }
            Kind::RunSimulator { launch } if ok => {
                if let Some(bundle) = launch {
                    let inv = Invocation {
                        program: "xcrun".into(),
                        args: vec!["simctl".into(), "launch".into(), "booted".into(), bundle.clone()],
                        cwd: None,
                        path_var: self.path_var.clone(),
                        display: format!("xcrun simctl launch booted {bundle}"),
                    };
                    self.launch(ctx, Kind::Plain, inv, Vec::new());
                }
            }
            Kind::Run { .. } | Kind::RunSimulator { .. } => {}
            Kind::New(path) => {
                if ok && path.is_dir() {
                    self.open_project(path);
                }
            }
            Kind::Xcode => {
                if let (true, Some(p)) = (ok, xtool::parse_wrote_to(&output)) {
                    self.artifact = Some(self.local(&p));
                }
            }
            Kind::Ds(title) => self.ds = (title, output),
        }
    }

    /// Debug builds only: start an action at launch, for testing the UI
    /// without clicking (XTOOL_GUI_DEBUG_ACTION=build|export|run|login).
    #[cfg(debug_assertions)]
    fn debug_action(&mut self, ctx: &egui::Context) {
        match std::env::var("XTOOL_GUI_DEBUG_ACTION").as_deref() {
            Ok("build") => self.build(ctx, false),
            Ok("export") => self.build(ctx, true),
            Ok("run") => self.run(ctx),
            Ok("login") => {
                self.cfg.page = Page::Account;
                self.password = "debug-password".into();
                self.sign_in(ctx);
            }
            _ => {}
        }
    }

    fn local(&self, p: &str) -> PathBuf {
        match &self.env {
            Some(e) => e.local_path(p),
            None => PathBuf::from(p),
        }
    }

    fn arg_path(&self, p: &Path) -> String {
        match &self.env {
            Some(e) => e.arg_path(p),
            None => p.display().to_string(),
        }
    }

    fn device_args(&self) -> Vec<String> {
        let mut a = Vec::new();
        if let Some(udid) = &self.cfg.device {
            a.push("--udid".into());
            a.push(udid.clone());
        }
        a.push(self.cfg.search.flag().into());
        a
    }

    fn config_args(&self) -> Vec<String> {
        vec!["-c".into(), if self.cfg.release { "release" } else { "debug" }.into()]
    }

    fn project_bundle_id(&self) -> Option<String> {
        self.yml.as_ref().and_then(|y| xtool::bundle_id(&y.saved))
    }

    fn can_simulate(&self) -> bool {
        cfg!(target_os = "macos")
    }

    fn run(&mut self, ctx: &egui::Context) {
        let Some(dir) = self.cfg.project.clone() else { return };
        let launch = self.cfg.launch_after.then(|| self.project_bundle_id()).flatten();
        let mut args = vec!["dev".into(), "run".into()];
        args.extend(self.config_args());
        if self.can_simulate() && self.cfg.simulator {
            args.push("--simulator".into());
            self.start(ctx, Kind::RunSimulator { launch }, args, Some(&dir));
        } else {
            args.extend(self.device_args());
            self.start(ctx, Kind::Run { launch }, args, Some(&dir));
        }
    }

    fn build(&mut self, ctx: &egui::Context, export: bool) {
        let Some(dir) = self.cfg.project.clone() else { return };
        let mut args = vec!["dev".into(), "build".into()];
        if export {
            args.extend(["-c".into(), "release".into(), "--ipa".into()]);
        } else {
            args.extend(self.config_args());
            if self.cfg.ipa {
                args.push("--ipa".into());
            }
        }
        if self.cfg.sign {
            args.push("--sign".into());
        }
        if !self.cfg.triple.trim().is_empty() {
            args.extend(["--triple".into(), self.cfg.triple.trim().into()]);
        }
        self.start(ctx, Kind::Build { export }, args, Some(&dir));
    }

    fn save_copy(&mut self, from: &Path) {
        let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = from.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        let mut dlg = rfd::FileDialog::new().set_file_name(&name);
        if !ext.is_empty() {
            dlg = dlg.add_filter(&ext, &[&ext]);
        }
        if let Some(to) = dlg.save_file() {
            self.notice = Some(match xtool::copy_any(from, &to) {
                Ok(()) => (OK, format!("Saved {}", to.display())),
                Err(e) => (ERR, format!("Could not save: {e}")),
            });
        }
    }

    // ----------------------------------------------------------- project

    fn open_project(&mut self, path: PathBuf) {
        self.cfg.recent.retain(|p| p != &path);
        self.cfg.recent.insert(0, path.clone());
        self.cfg.recent.truncate(12);
        self.load_yml(&path);
        if let Some(b) = self.project_bundle_id() {
            self.cfg.launch_bundle = b.clone();
            self.cfg.uninstall_bundle = b;
        }
        self.cfg.project = Some(path);
        self.artifact = None;
    }

    fn load_yml(&mut self, dir: &Path) {
        let path = dir.join("xtool.yml");
        self.yml = std::fs::read_to_string(&path).ok().map(|text| Yml { path, saved: text.clone(), text });
    }

    // ---------------------------------------------------------------- ui

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let sc = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
        let (run, build, stop) = ctx.input_mut(|i| {
            (i.consume_shortcut(&sc(Key::R)), i.consume_shortcut(&sc(Key::B)), i.consume_shortcut(&sc(Key::Period)))
        });
        if stop && let Some((_, j)) = &mut self.job {
            j.stop();
        }
        if !self.busy() && self.cfg.project.is_some() {
            if run {
                self.run(ctx);
            } else if build {
                self.build(ctx, false);
            }
        }
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        ui.add_space(10.0);
        ui.label(RichText::new("xtool-gui").strong().size(16.0));
        ui.add_space(12.0);
        ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
            ui.spacing_mut().button_padding.y = 6.0;
            for page in Page::ALL {
                let selected = self.cfg.page == page;
                if ui.selectable_label(selected, page.label()).clicked() {
                    self.cfg.page = page;
                    self.armed = None;
                }
            }
        });

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(8.0);
            let (c, t) = match &self.version.value {
                _ if self.detecting => (WARN, "Looking for xtool…".to_string()),
                Some(Ok(v)) => (OK, v.clone()),
                Some(Err(_)) if self.env.is_none() => (ERR, "xtool not found".into()),
                Some(Err(_)) => (ERR, "xtool error".into()),
                None => (WARN, "…".into()),
            };
            status_dot(ui, c, &t);
            if !cfg!(target_os = "macos") {
                let (c, t) = match &self.sdk.value {
                    Some(Ok(s)) if s.contains("is installed") => (OK, "SDK installed"),
                    Some(Ok(_)) => (WARN, "SDK not installed"),
                    Some(Err(_)) => (ERR, "SDK error"),
                    None => (WARN, "SDK …"),
                };
                status_dot(ui, c, t);
            }
            let (c, t) = match &self.auth.value {
                Some(Ok(s)) if s.starts_with("Logged in") => (OK, "Signed in"),
                Some(Ok(_)) => (WARN, "Signed out"),
                Some(Err(_)) => (ERR, "Auth error"),
                None => (WARN, "Auth …"),
            };
            status_dot(ui, c, t);
        });
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            let name = self.cfg.project.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
            ui.label(RichText::new(name.as_deref().unwrap_or("No project")).strong());
            ui.separator();

            ComboBox::from_id_salt("cfg")
                .width(80.0)
                .selected_text(if self.cfg.release { "Release" } else { "Debug" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.cfg.release, false, "Debug");
                    ui.selectable_value(&mut self.cfg.release, true, "Release");
                });
            self.destination_combo(ui);

            let has_project = self.cfg.project.is_some();
            let idle = !self.busy() && has_project;
            if ui
                .add_enabled(
                    idle,
                    egui::Button::new(RichText::new("▶ Run").color(if idle {
                        OK
                    } else {
                        ui.visuals().weak_text_color()
                    })),
                )
                .on_hover_text("Build, install and launch (Ctrl/Cmd+R)")
                .clicked()
            {
                self.run(&ctx);
            }
            if ui.add_enabled(idle, egui::Button::new("Build")).on_hover_text("Ctrl/Cmd+B").clicked() {
                self.build(&ctx, false);
            }
            if let Some((_, job)) = &mut self.job {
                if ui.button(RichText::new("■ Stop").color(ERR)).on_hover_text("Ctrl/Cmd+.").clicked() {
                    job.stop();
                }
                let secs = job.elapsed().as_secs();
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("{}:{:02}  {}", secs / 60, secs % 60, self.job_title)).weak(),
                    )
                    .truncate(),
                );
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
            } else if let Some((c, n)) = &self.notice {
                ui.label(RichText::new(n).color(*c));
            }
        });
    }

    fn destination_combo(&mut self, ui: &mut Ui) {
        let devices: Vec<Device> = match &self.devices.value {
            Some(Ok(d)) => d.clone(),
            _ => Vec::new(),
        };
        let sim = self.can_simulate() && self.cfg.simulator;
        let text = if sim {
            "iOS Simulator".to_string()
        } else {
            match &self.cfg.device {
                None => "Any device".to_string(),
                Some(u) => devices
                    .iter()
                    .find(|d| &d.udid == u)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| format!("{} (offline)", short(u))),
            }
        };
        let r = ComboBox::from_id_salt("dest").width(170.0).selected_text(text).show_ui(ui, |ui| {
            if ui.selectable_label(!sim && self.cfg.device.is_none(), "Any device").clicked() {
                self.cfg.simulator = false;
                self.cfg.device = None;
            }
            for d in &devices {
                let sel = !sim && self.cfg.device.as_ref() == Some(&d.udid);
                if ui.selectable_label(sel, format!("{}  ({})", d.name, d.connection)).clicked() {
                    self.cfg.simulator = false;
                    self.cfg.device = Some(d.udid.clone());
                }
            }
            if self.can_simulate() {
                ui.separator();
                if ui.selectable_label(sim, "iOS Simulator (booted)").clicked() {
                    self.cfg.simulator = true;
                }
            }
        });
        if r.response.clicked() {
            self.refresh_devices(ui.ctx());
        }
    }

    fn console_ui(&mut self, ui: &mut Ui) {
        let running = self.job.is_some();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Console").strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Clear").clicked() {
                    self.console.lock().unwrap().clear();
                }
                if ui.small_button("Copy").clicked() {
                    let t = self.console.lock().unwrap().plain_text();
                    ui.ctx().copy_text(t);
                }
            });
        });

        let (partial, choices) = {
            let c = self.console.lock().unwrap();
            (c.partial(), if running { c.choices() } else { None })
        };
        // A new prompt appeared: put the cursor in the input box.
        if running && partial != self.last_partial && partial.trim_end().ends_with(':') {
            self.focus_input = true;
        }
        self.last_partial = partial.clone();

        let input_h = if running { 34.0 } else { 0.0 };
        let mono = TextStyle::Monospace.resolve(ui.style());
        let row_h = ui.fonts_mut(|f| f.row_height(&mono));
        let s = theme::surfaces(ui.visuals().dark_mode);
        Frame::new().fill(s.console).inner_margin(Margin::same(6)).show(ui, |ui| {
            ui.set_min_height((ui.available_height() - input_h).max(40.0));
            let c = self.console.lock().unwrap();
            let total = c.lines.len() + usize::from(!partial.is_empty());
            let text_col = ui.visuals().text_color();
            ScrollArea::both()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .max_height((ui.available_height() - input_h).max(40.0))
                .show_rows(ui, row_h, total, |ui, range| {
                    ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                    for i in range {
                        let (text, kind) = match c.lines.get(i) {
                            Some(l) => (l.text.as_str(), l.kind),
                            None => (partial.as_str(), LineKind::Out),
                        };
                        let color = match kind {
                            LineKind::Out => text_col,
                            LineKind::Cmd => ACCENT,
                            LineKind::Ok => OK,
                            LineKind::Err => ERR,
                            LineKind::Info => WARN,
                        };
                        let mut rt = RichText::new(text).monospace().color(color);
                        if kind == LineKind::Cmd {
                            rt = rt.strong();
                        }
                        ui.add(egui::Label::new(rt).selectable(true));
                    }
                });
        });

        if running {
            self.prompt_row(ui, &partial, choices);
        }
    }

    fn prompt_row(&mut self, ui: &mut Ui, partial: &str, choices: Option<Vec<(usize, String)>>) {
        let lower = partial.to_lowercase();
        let secret = lower.contains("password");
        let wants_path = lower.contains("path");
        let mut send: Option<String> = None;
        ui.horizontal(|ui| {
            if let Some(choices) = &choices {
                for (n, label) in choices {
                    if ui.button(label).clicked() {
                        send = Some(n.to_string());
                    }
                }
                return;
            }
            let hint = if partial.trim().is_empty() { "Input to the running command…" } else { partial.trim() };
            let w = ui.available_width() - if wants_path { 150.0 } else { 70.0 };
            let r = ui.add(TextEdit::singleline(&mut self.input).password(secret).hint_text(hint).desired_width(w));
            if self.focus_input {
                r.request_focus();
                self.focus_input = false;
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                send = Some(std::mem::take(&mut self.input));
                r.request_focus();
            }
            if wants_path
                && ui.button("Browse…").clicked()
                && let Some(p) = rfd::FileDialog::new().pick_file()
            {
                send = Some(self.arg_path(&p));
            }
            if ui.button("Send").clicked() {
                send = Some(std::mem::take(&mut self.input));
            }
        });
        if let (Some(text), Some((_, job))) = (send, &self.job) {
            job.send_line(&text);
        }
    }

    // ------------------------------------------------------------- pages

    fn page_project(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let idle = !self.busy();
        let Some(dir) = self.cfg.project.clone() else {
            heading(ui, "Project");
            ui.horizontal(|ui| {
                if ui.button("Open folder…").clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    self.open_project(p);
                }
            });
            if !self.cfg.recent.is_empty() {
                section(ui, "Recent");
                let mut open = None;
                let mut remove = None;
                for (i, p) in self.cfg.recent.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        if ui.link(name).clicked() {
                            open = Some(p.clone());
                        }
                        ui.label(RichText::new(p.display().to_string()).weak());
                        if ui.small_button("✕").on_hover_text("Remove from list").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    self.cfg.recent.remove(i);
                }
                if let Some(p) = open {
                    self.open_project(p);
                }
            }
            self.new_project_form(ui);
            return;
        };

        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        ui.horizontal(|ui| {
            heading(ui, &name);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    self.cfg.project = None;
                    self.yml = None;
                    self.artifact = None;
                }
                if ui.button("Open folder").clicked() {
                    xtool::open(&dir);
                }
                if ui.button("Switch…").clicked()
                    && let Some(p) = rfd::FileDialog::new().set_directory(&dir).pick_folder()
                {
                    self.open_project(p);
                }
            });
        });
        ui.label(RichText::new(dir.display().to_string()).weak());
        if !dir.join("Package.swift").exists() {
            ui.colored_label(
                WARN,
                "No Package.swift here. xtool builds SwiftPM packages — create one with “New project”.",
            );
        }

        section(ui, "Build");
        egui::Grid::new("build").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("Configuration");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.cfg.release, false, "Debug");
                ui.radio_value(&mut self.cfg.release, true, "Release");
            });
            ui.end_row();
            ui.label("Signing");
            ui.checkbox(&mut self.cfg.sign, "Codesign the build with your Apple account");
            ui.end_row();
            ui.label("Output");
            ui.checkbox(&mut self.cfg.ipa, "Package as .ipa instead of .app");
            ui.end_row();
            ui.label("Target triple");
            ui.add(TextEdit::singleline(&mut self.cfg.triple).hint_text("arm64-apple-ios").desired_width(220.0));
            ui.end_row();
            ui.label("After install");
            ui.checkbox(&mut self.cfg.launch_after, "Launch the app").on_hover_text("Uses the bundleID from xtool.yml");
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(idle, egui::Button::new("▶ Run")).clicked() {
                self.run(&ctx);
            }
            if ui.add_enabled(idle, egui::Button::new("Build")).clicked() {
                self.build(&ctx, false);
            }
            if ui
                .add_enabled(idle, egui::Button::new("Export .ipa…"))
                .on_hover_text("Release build packaged as .ipa, then choose where to save it")
                .clicked()
            {
                self.build(&ctx, true);
            }
            if ui.add_enabled(idle, egui::Button::new("Generate Xcode project")).clicked() {
                self.start(&ctx, Kind::Xcode, vec!["dev".into(), "generate-xcode-project".into()], Some(&dir));
            }
        });

        if let Some(art) = self.artifact.clone() {
            section(ui, "Last output");
            ui.label(RichText::new(art.display().to_string()).monospace());
            ui.horizontal(|ui| {
                if ui.button("Show in folder").clicked() {
                    xtool::reveal(&art);
                }
                if ui.button("Save a copy…").clicked() {
                    self.save_copy(&art);
                }
                let installable = matches!(art.extension().and_then(|e| e.to_str()), Some("ipa" | "app"));
                if installable && ui.add_enabled(idle, egui::Button::new("Install to device")).clicked() {
                    self.install(&ctx, &art);
                }
                if art.extension().and_then(|e| e.to_str()) == Some("xcodeproj") && ui.button("Open").clicked() {
                    xtool::open(&art);
                }
            });
        }

        section(ui, "xtool.yml");
        match &mut self.yml {
            None => {
                ui.label(RichText::new("No xtool.yml — xtool will use the default com.example organization.").weak());
                if ui.button("Create xtool.yml").clicked() {
                    let text = format!("version: 1\nbundleID: com.example.{name}\n");
                    if std::fs::write(dir.join("xtool.yml"), &text).is_ok() {
                        self.load_yml(&dir);
                    }
                }
            }
            Some(y) => {
                let dirty = y.text != y.saved;
                ui.add(TextEdit::multiline(&mut y.text).code_editor().desired_rows(6).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    if ui.add_enabled(dirty, egui::Button::new("Save")).clicked() {
                        match std::fs::write(&y.path, &y.text) {
                            Ok(()) => y.saved = y.text.clone(),
                            Err(e) => self.notice = Some((ERR, format!("Could not save xtool.yml: {e}"))),
                        }
                    }
                    if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
                        y.text = y.saved.clone();
                    }
                    if dirty {
                        ui.label(RichText::new("modified").color(WARN));
                    }
                });
            }
        }

        self.new_project_form(ui);
    }

    fn new_project_form(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        section(ui, "New project");
        egui::Grid::new("new").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("Name");
            ui.add(TextEdit::singleline(&mut self.cfg.new_name).hint_text("MyApp").desired_width(220.0));
            ui.end_row();
            ui.label("Location");
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut self.cfg.new_dir).desired_width(300.0));
                if ui.button("Browse…").clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    self.cfg.new_dir = p.display().to_string();
                }
            });
            ui.end_row();
            ui.label("");
            ui.checkbox(&mut self.cfg.new_setup, "Run setup first (sign in, install SDK)");
            ui.end_row();
        });
        let name = self.cfg.new_name.trim().to_string();
        let valid = valid_package_name(&name);
        let parent = PathBuf::from(self.cfg.new_dir.trim());
        let target = parent.join(&name);
        let exists = !name.is_empty() && target.exists();
        ui.horizontal(|ui| {
            let ok = valid && parent.is_dir() && !exists && !self.busy();
            if ui.add_enabled(ok, egui::Button::new("Create")).clicked() {
                let mut args = vec!["new".to_string(), name.clone()];
                if !self.cfg.new_setup {
                    args.push("--skip-setup".into());
                }
                self.start(&ctx, Kind::New(target.clone()), args, Some(&parent));
            }
            if !name.is_empty() && !valid {
                ui.colored_label(ERR, "Use letters, digits, - and _; start with a letter or _.");
            } else if exists {
                ui.colored_label(ERR, "That folder already exists.");
            } else if !parent.is_dir() {
                ui.colored_label(ERR, "Location is not a folder.");
            }
        });
    }

    fn install(&mut self, ctx: &egui::Context, path: &Path) {
        let mut args = vec!["install".into()];
        args.extend(self.device_args());
        args.push(self.arg_path(path));
        self.start(ctx, Kind::Plain, args, None);
    }

    fn page_devices(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let idle = !self.busy();
        heading(ui, "Devices");
        ui.horizontal(|ui| {
            let before = self.cfg.search;
            ui.radio_value(&mut self.cfg.search, Search::All, "All");
            ui.radio_value(&mut self.cfg.search, Search::Usb, "USB");
            ui.radio_value(&mut self.cfg.search, Search::Network, "Network");
            if ui.button("Refresh").clicked() || before != self.cfg.search {
                self.refresh_devices(&ctx);
            }
            if self.devices.loading {
                ui.label(RichText::new("Searching…").weak());
            }
        });
        ui.add_space(4.0);

        let devices = match &self.devices.value {
            Some(Ok(d)) => d.clone(),
            Some(Err(e)) => {
                ui.colored_label(ERR, first_line(e));
                Vec::new()
            }
            None => Vec::new(),
        };
        egui::Grid::new("devs").num_columns(3).striped(true).spacing([18.0, 6.0]).show(ui, |ui| {
            if ui.radio(self.cfg.device.is_none() && !self.cfg.simulator, "Any device").clicked() {
                self.cfg.device = None;
                self.cfg.simulator = false;
            }
            ui.label(RichText::new("ask when several are connected").weak());
            ui.label("");
            ui.end_row();
            for d in &devices {
                let sel = self.cfg.device.as_ref() == Some(&d.udid) && !self.cfg.simulator;
                if ui.radio(sel, &d.name).clicked() {
                    self.cfg.device = Some(d.udid.clone());
                    self.cfg.simulator = false;
                }
                ui.label(&d.connection);
                ui.label(RichText::new(&d.udid).monospace().weak());
                ui.end_row();
            }
        });
        if devices.is_empty() && !self.devices.loading {
            ui.label(RichText::new("No devices found. Connect a device by USB and trust this computer.").weak());
        }

        section(ui, "Install");
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(idle, egui::Button::new("Install .ipa…")).clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("iOS app", &["ipa"]).pick_file()
            {
                self.install(&ctx, &p);
            }
            if ui
                .add_enabled(idle, egui::Button::new("Install .app…"))
                .on_hover_text("Choose the .app folder")
                .clicked()
                && let Some(p) = rfd::FileDialog::new().pick_folder()
            {
                self.install(&ctx, &p);
            }
            if let Some(art) = self.artifact.clone()
                && matches!(art.extension().and_then(|e| e.to_str()), Some("ipa" | "app"))
                && ui.add_enabled(idle, egui::Button::new("Install last build")).clicked()
            {
                self.install(&ctx, &art);
            }
        });

        section(ui, "Launch");
        egui::Grid::new("launch").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("Bundle ID");
            ui.add(
                TextEdit::singleline(&mut self.cfg.launch_bundle).hint_text("com.example.MyApp").desired_width(260.0),
            );
            ui.end_row();
            ui.label("Arguments");
            ui.add(TextEdit::singleline(&mut self.cfg.launch_args).hint_text("optional").desired_width(260.0));
            ui.end_row();
        });
        let bundle = self.cfg.launch_bundle.trim().to_string();
        if ui.add_enabled(idle && !bundle.is_empty(), egui::Button::new("Launch")).clicked() {
            let mut args = vec!["launch".into()];
            args.extend(self.device_args());
            let extra = xtool::split_args(&self.cfg.launch_args);
            if !extra.is_empty() {
                args.push("--".into());
            }
            args.push(bundle);
            args.extend(extra);
            self.start(&ctx, Kind::Plain, args, None);
        }

        section(ui, "Uninstall");
        ui.horizontal(|ui| {
            ui.label("Bundle ID");
            ui.add(
                TextEdit::singleline(&mut self.cfg.uninstall_bundle)
                    .hint_text("com.example.MyApp")
                    .desired_width(260.0),
            );
            let b = self.cfg.uninstall_bundle.trim().to_string();
            if confirm_button(ui, &mut self.armed, "uninstall", "Uninstall", idle && !b.is_empty()) {
                let mut args = vec!["uninstall".into()];
                args.extend(self.device_args());
                args.push(b);
                self.start(&ctx, Kind::Plain, args, None);
            }
        });

        if self.can_simulate() {
            section(ui, "Simulator");
            ui.label(RichText::new("Pick “iOS Simulator” as the destination to run on the booted simulator.").weak());
            if ui.button("Open Simulator").clicked() {
                xtool::spawn_detached("open", &["-a".into(), "Simulator".into()]);
            }
        }
    }

    fn sign_in(&mut self, ctx: &egui::Context) {
        let (args, responders) = match self.cfg.login_mode {
            LoginMode::Password => (
                vec![
                    "auth".into(),
                    "login".into(),
                    "-m".into(),
                    "password".into(),
                    "-u".into(),
                    self.cfg.apple_id.trim().into(),
                ],
                // Sent through the terminal, never on the command line.
                vec![Responder { prompt: "Password: ", answer: self.password.clone(), secret: true }],
            ),
            LoginMode::Key => (
                vec!["auth".into(), "login".into(), "-m".into(), "key".into()],
                vec![
                    Responder { prompt: "Key ID: ", answer: self.cfg.key_id.trim().into(), secret: false },
                    Responder { prompt: "Issuer ID: ", answer: self.cfg.issuer_id.trim().into(), secret: false },
                    Responder {
                        prompt: "Key path: ",
                        answer: self.arg_path(Path::new(self.cfg.key_path.trim())),
                        secret: false,
                    },
                ],
            ),
        };
        self.start_with(ctx, Kind::Auth, args, None, responders);
    }

    fn page_account(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let idle = !self.busy();
        heading(ui, "Account");
        status_block(ui, &self.auth, "auth status");
        if ui.button("Refresh").clicked() {
            self.refresh_auth(&ctx);
        }

        section(ui, "Sign in");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.cfg.login_mode, LoginMode::Password, "Apple ID");
            ui.radio_value(&mut self.cfg.login_mode, LoginMode::Key, "App Store Connect API key");
        });
        ui.label(
            RichText::new(match self.cfg.login_mode {
                LoginMode::Password => "Works with any Apple ID. You may be asked for a two-factor code below.",
                LoginMode::Key => "Requires a paid Apple Developer Program membership.",
            })
            .weak(),
        );
        ui.add_space(4.0);
        let ready = egui::Grid::new("login")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| match self.cfg.login_mode {
                LoginMode::Password => {
                    ui.label("Apple ID");
                    ui.add(TextEdit::singleline(&mut self.cfg.apple_id).desired_width(260.0));
                    ui.end_row();
                    ui.label("Password");
                    ui.add(TextEdit::singleline(&mut self.password).password(true).desired_width(260.0));
                    ui.end_row();
                    !self.cfg.apple_id.trim().is_empty() && !self.password.is_empty()
                }
                LoginMode::Key => {
                    ui.label("Key ID");
                    ui.add(TextEdit::singleline(&mut self.cfg.key_id).desired_width(260.0));
                    ui.end_row();
                    ui.label("Issuer ID");
                    ui.add(TextEdit::singleline(&mut self.cfg.issuer_id).desired_width(260.0));
                    ui.end_row();
                    ui.label("Private key (.p8)");
                    ui.horizontal(|ui| {
                        ui.add(TextEdit::singleline(&mut self.cfg.key_path).desired_width(260.0));
                        if ui.button("Browse…").clicked()
                            && let Some(p) = rfd::FileDialog::new().add_filter("Private key", &["p8"]).pick_file()
                        {
                            self.cfg.key_path = p.display().to_string();
                        }
                    });
                    ui.end_row();
                    [&self.cfg.key_id, &self.cfg.issuer_id, &self.cfg.key_path].iter().all(|s| !s.trim().is_empty())
                }
            })
            .inner;
        if ui.add_enabled(idle && ready, egui::Button::new("Sign in")).clicked() {
            self.sign_in(&ctx);
        }

        section(ui, "Sign out");
        ui.checkbox(&mut self.cfg.reset_2fa, "Also reset two-factor pseudo-device")
            .on_hover_text("You will be asked to complete two-factor authentication again next time.");
        if confirm_button(ui, &mut self.armed, "logout", "Sign out", idle) {
            let mut args = vec!["auth".into(), "logout".into()];
            if self.cfg.reset_2fa {
                args.push("--reset-2fa".into());
            }
            self.start(&ctx, Kind::Auth, args, None);
        }

        section(ui, "Setup");
        ui.label(RichText::new("Runs `xtool setup`: signs in if needed, then installs the iOS SDK.").weak());
        if ui.add_enabled(idle, egui::Button::new("Run setup")).clicked() {
            self.start(&ctx, Kind::Auth, vec!["setup".into()], None);
        }
    }

    fn page_sdk(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let idle = !self.busy();
        heading(ui, "Darwin SDK");
        if cfg!(target_os = "macos") {
            ui.label(RichText::new("On macOS xtool uses the SDK that ships with Xcode. The Darwin SDK is only needed on Linux and Windows (WSL).").weak());
        }
        status_block(ui, &self.sdk, "sdk status");
        if ui.button("Refresh").clicked() {
            self.refresh_sdk(&ctx);
        }

        section(ui, "Install");
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.cfg.sdk_source)
                    .hint_text("Xcode.xip, Xcode.app or darwin.xtoolsdk")
                    .desired_width(340.0),
            );
            if ui.button("File…").clicked()
                && let Some(p) = rfd::FileDialog::new().pick_file()
            {
                self.cfg.sdk_source = p.display().to_string();
            }
            if ui.button("Folder…").clicked()
                && let Some(p) = rfd::FileDialog::new().pick_folder()
            {
                self.cfg.sdk_source = p.display().to_string();
            }
        });
        ui.checkbox(&mut self.cfg.sdk_slim, "Slim install (less disk space; cannot be updated in place)");
        let src = self.cfg.sdk_source.trim().to_string();
        if ui.add_enabled(idle && !src.is_empty(), egui::Button::new("Install SDK")).clicked() {
            let mut args = vec!["sdk".into(), "install".into(), self.arg_path(Path::new(&src))];
            if self.cfg.sdk_slim {
                args.push("--slim".into());
            }
            self.start(&ctx, Kind::Sdk, args, None);
        }

        section(ui, "Manage");
        ui.horizontal(|ui| {
            if ui.add_enabled(idle, egui::Button::new("Update")).clicked() {
                self.start(&ctx, Kind::Sdk, vec!["sdk".into(), "update".into()], None);
            }
            if confirm_button(ui, &mut self.armed, "sdk-remove", "Remove", idle) {
                self.start(&ctx, Kind::Sdk, vec!["sdk".into(), "remove".into()], None);
            }
        });

        section(ui, "Build an SDK bundle");
        ui.label(RichText::new("Produce a darwin.xtoolsdk from Xcode to install elsewhere.").weak());
        egui::Grid::new("sdkb").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("Xcode");
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.cfg.sdk_build_src)
                        .hint_text("Xcode.xip or Xcode.app")
                        .desired_width(300.0),
                );
                if ui.button("Browse…").clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_file()
                {
                    self.cfg.sdk_build_src = p.display().to_string();
                }
            });
            ui.end_row();
            ui.label("Output folder");
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut self.cfg.sdk_build_out).desired_width(300.0));
                if ui.button("Browse…").clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    self.cfg.sdk_build_out = p.display().to_string();
                }
            });
            ui.end_row();
            ui.label("Architecture");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.cfg.sdk_arch, Arch::Auto, "Auto");
                ui.radio_value(&mut self.cfg.sdk_arch, Arch::Arm64, "arm64");
                ui.radio_value(&mut self.cfg.sdk_arch, Arch::X86_64, "x86_64");
            });
            ui.end_row();
        });
        let (s, o) = (self.cfg.sdk_build_src.trim().to_string(), self.cfg.sdk_build_out.trim().to_string());
        if ui.add_enabled(idle && !s.is_empty() && !o.is_empty(), egui::Button::new("Build SDK")).clicked() {
            let arch = match self.cfg.sdk_arch {
                Arch::Auto => "auto",
                Arch::Arm64 => "arm64",
                Arch::X86_64 => "x86_64",
            };
            let args = vec![
                "sdk".into(),
                "build".into(),
                self.arg_path(Path::new(&s)),
                self.arg_path(Path::new(&o)),
                "--arch".into(),
                arch.into(),
            ];
            self.start(&ctx, Kind::Sdk, args, None);
        }
    }

    fn page_developer(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let idle = !self.busy();
        heading(ui, "Developer Services");
        ui.label(RichText::new("Read-only views of your Apple Developer account. Requires signing in.").weak());
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (title, sub) in [
                ("Teams", "teams"),
                ("Certificates", "certificates"),
                ("Bundle IDs", "identifiers"),
                ("Profiles", "profiles"),
                ("Registered devices", "devices"),
            ] {
                if ui.add_enabled(idle, egui::Button::selectable(self.ds.0 == title, title)).clicked() {
                    self.start(&ctx, Kind::Ds(title), vec!["ds".into(), sub.into(), "list".into()], None);
                }
            }
        });
        if !self.ds.0.is_empty() {
            section(ui, self.ds.0);
            let mut text = self.ds.1.as_str();
            if text.trim().is_empty() {
                ui.label(RichText::new("Nothing returned. See the console for details.").weak());
            } else {
                ui.add(TextEdit::multiline(&mut text).code_editor().desired_width(f32::INFINITY));
            }
        }
    }

    fn page_settings(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        heading(ui, "Settings");

        section(ui, "xtool");
        if cfg!(windows) && ui.checkbox(&mut self.cfg.use_wsl, "Run xtool inside WSL").changed() {
            self.detect(&ctx, false);
        }
        ui.horizontal(|ui| {
            ui.label("Executable");
            let r = ui.add(
                TextEdit::singleline(&mut self.cfg.xtool_path)
                    .hint_text(self.env.as_ref().map(|e| e.xtool.as_str()).unwrap_or("auto-detect"))
                    .desired_width(320.0),
            );
            if r.lost_focus() {
                self.detect(&ctx, false);
            }
            if !self.cfg.use_wsl
                && ui.button("Browse…").clicked()
                && let Some(p) = rfd::FileDialog::new().pick_file()
            {
                self.cfg.xtool_path = p.display().to_string();
                self.detect(&ctx, false);
            }
            if ui.button("Detect").clicked() {
                self.cfg.xtool_path.clear();
                self.detect(&ctx, true);
            }
        });
        match (&self.env, &self.version.value) {
            (_, _) if self.detecting => {
                ui.label(RichText::new("Looking for xtool…").weak());
            }
            (Some(e), Some(Ok(v))) => {
                ui.label(RichText::new(format!("{v} — {}", e.xtool)).color(OK));
            }
            (Some(e), Some(Err(err))) => {
                ui.colored_label(ERR, format!("{} did not run: {}", e.xtool, first_line(err)));
            }
            (None, _) => {
                ui.colored_label(ERR, "xtool was not found.");
                ui.hyperlink_to("Install xtool", "https://xtool.sh/documentation/xtooldocs/");
            }
            _ => {}
        }

        section(ui, "Appearance");
        ui.horizontal(|ui| {
            let before = self.cfg.theme;
            ui.radio_value(&mut self.cfg.theme, ThemeChoice::Dark, "Dark");
            ui.radio_value(&mut self.cfg.theme, ThemeChoice::Light, "Light");
            ui.radio_value(&mut self.cfg.theme, ThemeChoice::System, "System");
            if before != self.cfg.theme {
                self.apply_appearance(&ctx);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Scale");
            for z in [0.9, 1.0, 1.1, 1.25, 1.5] {
                if ui.selectable_label((self.cfg.zoom - z).abs() < 0.01, format!("{:.0}%", z * 100.0)).clicked() {
                    self.cfg.zoom = z;
                    self.apply_appearance(&ctx);
                }
            }
        });

        section(ui, "About");
        ui.label(format!("xtool-gui {} — MIT License", env!("CARGO_PKG_VERSION")));
        ui.horizontal(|ui| {
            ui.hyperlink_to("xtool.sh", "https://xtool.sh");
            ui.hyperlink_to("xtool on GitHub", "https://github.com/xtool-org/xtool");
        });
        ui.label(RichText::new("Shortcuts: Ctrl/Cmd+R run · Ctrl/Cmd+B build · Ctrl/Cmd+. stop").weak());
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.pump(&ctx);
        self.shortcuts(&ctx);
        let s = theme::surfaces(ui.visuals().dark_mode);
        let line = egui::Stroke::new(1.0, s.border);

        Panel::left("nav")
            .exact_size(176.0)
            .resizable(false)
            .frame(Frame::new().fill(s.sidebar).inner_margin(Margin::symmetric(10, 6)).stroke(line))
            .show(ui, |ui| self.sidebar(ui));
        Panel::top("bar")
            .frame(Frame::new().fill(s.main).inner_margin(Margin::symmetric(12, 8)).stroke(line))
            .show(ui, |ui| self.toolbar(ui));
        Panel::bottom("console")
            .resizable(true)
            .default_size(230.0)
            .size_range(90.0..=900.0)
            .frame(Frame::new().fill(s.main).inner_margin(Margin::symmetric(10, 6)).stroke(line))
            .show(ui, |ui| self.console_ui(ui));
        CentralPanel::default().frame(Frame::new().fill(s.main).inner_margin(Margin::symmetric(20, 14))).show(
            ui,
            |ui| {
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(760.0);
                    match self.cfg.page {
                        Page::Project => self.page_project(ui),
                        Page::Devices => self.page_devices(ui),
                        Page::Account => self.page_account(ui),
                        Page::Sdk => self.page_sdk(ui),
                        Page::Developer => self.page_developer(ui),
                        Page::Settings => self.page_settings(ui),
                    }
                    ui.add_space(16.0);
                });
            },
        );
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, CONFIG_KEY, &self.cfg);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        theme::surfaces(visuals.dark_mode).main.to_normalized_gamma_f32()
    }
}

// ----------------------------------------------------------------- helpers

fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(18.0).strong());
}

fn section(ui: &mut Ui, text: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(text).strong());
    ui.separator();
}

fn status_dot(ui: &mut Ui, color: Color32, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.5, color);
        ui.label(RichText::new(text).small());
    });
}

fn status_block(ui: &mut Ui, l: &Loaded<String>, what: &str) {
    let text = match &l.value {
        _ if l.loading => "Checking…".to_string(),
        Some(Ok(s)) => s.trim().to_string(),
        Some(Err(e)) => format!("`xtool {what}` failed:\n{}", e.trim()),
        None => "Unknown".into(),
    };
    Frame::new().fill(ui.visuals().faint_bg_color).inner_margin(Margin::same(8)).corner_radius(4).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(egui::Label::new(RichText::new(text).monospace()).selectable(true));
    });
}

/// A destructive button that needs a second click to confirm.
fn confirm_button(ui: &mut Ui, armed: &mut Option<&'static str>, id: &'static str, label: &str, enabled: bool) -> bool {
    if *armed == Some(id) {
        let mut go = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(RichText::new(format!("Confirm {}", label.to_lowercase())).color(ERR)),
                )
                .clicked()
            {
                *armed = None;
                go = true;
            }
            if ui.button("Cancel").clicked() {
                *armed = None;
            }
        });
        go
    } else {
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            *armed = Some(id);
        }
        false
    }
}

fn first_line(s: &str) -> &str {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim()
}

fn short(udid: &str) -> &str {
    &udid[..udid.len().min(12)]
}

/// Mirrors the validation done by `xtool new`.
fn valid_package_name(n: &str) -> bool {
    let mut chars = n.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
