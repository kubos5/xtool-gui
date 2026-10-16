# xtool-gui

A minimal, fast desktop GUI for [xtool](https://github.com/xtool-org/xtool), the cross-platform Xcode replacement.
Build, run, sign, install and export SwiftPM-based iOS apps without touching the terminal.

Written in Rust with [egui](https://github.com/emilk/egui). It's a single native binary, with no web view, animations or background polling.

## Features

Everything xtool can do, one click away:

| Area | What you can do | xtool command |
| --- | --- | --- |
| **Project** | Open/create projects, edit `xtool.yml`, build (Debug/Release, signed, `.app` or `.ipa`, custom triple), run on a device or the iOS Simulator, export a release `.ipa`, generate an Xcode project | `new`, `dev build`, `dev run`, `dev generate-xcode-project` |
| **Devices** | List USB/network devices, pick the target device, install `.ipa`/`.app`, launch with arguments, uninstall | `devices`, `install`, `launch`, `uninstall` |
| **Account** | Sign in with an Apple ID (2FA supported) or an App Store Connect API key, sign out, reset 2FA, run first-time setup | `auth login/logout/status`, `setup` |
| **SDK** | Check, install (full or slim), update, remove, and build the Darwin SDK | `sdk status/install/update/remove/build` |
| **Developer Services** | Browse teams, certificates, bundle IDs, profiles and registered devices | `ds … list` |

The **Run** button builds, installs and then launches the app using the `bundleID` from `xtool.yml`.

### Interactive prompts

xtool runs inside a pseudo-terminal, so it behaves exactly as it does in a shell. When it asks a question, the console shows an input box:

- **Two-factor codes:** type the code and press Enter.
- **Team selection** (and any other numbered choice): click a button.
- **File paths** (e.g. `Path to Xcode.xip`): type the path, or use the **Browse…** button.
- **Passwords:** these are sent through the terminal with echo off. They never go on the command line, into the log, or into saved settings.

### Shortcuts

| | |
| --- | --- |
| `Ctrl/Cmd + R` | Run |
| `Ctrl/Cmd + B` | Build |
| `Ctrl/Cmd + .` | Stop |

## Requirements

- [xtool](https://xtool.sh/documentation/xtooldocs/) installed and on your `PATH`. If it lives somewhere else, set the location in **Settings**.
- Platform-specific:
  - **macOS:** Xcode, as xtool requires.
  - **Linux:** the Swift toolchain and Darwin SDK, as xtool requires. Native file dialogs use the XDG desktop portal.
  - **Windows:** xtool is installed inside WSL. xtool-gui runs natively on Windows and calls xtool through `wsl.exe`, translating file paths automatically.

xtool-gui reads your login shell's `PATH` on startup, so tools set up in your shell profile (swiftly, Homebrew, `~/.local/bin`) are found even when the app is started from a desktop launcher.

## Building

```bash
cargo build --release
```

The binary is written to `target/release/xtool-gui` (`xtool-gui.exe` on Windows).

On Linux you may need development headers for the windowing system:

```bash
sudo apt-get install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev
```

### Tests

```bash
cargo test
```

The tests cover output parsing and the pseudo-terminal runner. The runner test answers a hidden password prompt, a code prompt and a numbered choice against a real shell process.

## Layout of the source

| File | Purpose |
| --- | --- |
| `src/app.rs` | UI and app state |
| `src/runner.rs` | Runs xtool in a pseudo-terminal, auto-answers known prompts, handles stop |
| `src/term.rs` | Turns terminal output into lines (carriage returns, ANSI stripping, prompt/choice detection) |
| `src/xtool.rs` | Locating xtool, building invocations (incl. WSL), parsing output, opening files |
| `src/theme.rs` | Dark and light themes |

## License

[MIT](LICENSE)
