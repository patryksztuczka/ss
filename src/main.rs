mod accelerator;
#[cfg(target_os = "macos")]
mod actions;
#[cfg(target_os = "macos")]
mod app;
mod backgrounds;
mod capture_store;
#[cfg(target_os = "macos")]
mod clipboard;
#[cfg(target_os = "macos")]
mod color_panel;
mod compose;
#[cfg(target_os = "macos")]
mod debug;
#[cfg(target_os = "macos")]
mod desktop_ui;
mod displays;
#[cfg(target_os = "macos")]
mod drag;
#[cfg(target_os = "macos")]
mod hotkeys;
mod instance;
mod js_sort;
mod logging;
#[cfg(target_os = "macos")]
mod login_item;
#[cfg(target_os = "macos")]
mod notify;
#[cfg(target_os = "macos")]
mod ocr;
mod paths;
#[cfg(target_os = "macos")]
mod preview;
#[cfg(target_os = "macos")]
mod recorder;
mod screencapture;
mod settings;
#[cfg(target_os = "macos")]
mod setup;
#[cfg(target_os = "macos")]
mod system;
mod text_selection;
#[cfg(target_os = "macos")]
mod thumbnail;
#[cfg(target_os = "macos")]
mod tray;
#[cfg(target_os = "macos")]
mod windows;

use std::fs;
use std::path::Path;

use clap::{Parser, Subcommand};
use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::paths::Paths;

/// Developer subcommands exist only in debug builds, as in HEX.
const DEBUG_COMMANDS_ENABLED: bool = cfg!(debug_assertions);

#[derive(Parser)]
#[command(
    name = "ss",
    version,
    about = "Shortcut-driven screenshots and screen recordings for macOS"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the menu bar app. This is the default.
    App,
    /// Print the data folder, effective settings and capture count.
    Status,
    /// Open one window with fixture data, for UI work (debug builds only).
    #[command(hide = !DEBUG_COMMANDS_ENABLED)]
    Preview {
        #[arg(value_enum)]
        target: preview::Target,
        /// Onboarding step to show.
        #[arg(long, value_enum, default_value = "welcome")]
        step: preview::PreviewStep,
        /// Settings scroll position.
        #[arg(long, value_enum, default_value = "top")]
        scroll: preview::Scroll,
        /// Report Screen Recording as granted.
        #[arg(long)]
        granted: bool,
        /// Show two shortcuts as failed to register.
        #[arg(long)]
        failed: bool,
        /// Settings: show a shortcut row recording, with the ⌥-only hint.
        #[arg(long)]
        recording: bool,
        /// Preview: show the capture as a recording.
        #[arg(long)]
        video: bool,
        /// Preview: show the hover controls.
        #[arg(long)]
        hover: bool,
        /// Preview: the status chip text.
        #[arg(long)]
        chip: Option<String>,
        /// Editor: background id, e.g. "none".
        #[arg(long)]
        background: Option<String>,
        /// Editor: aspect ratio, e.g. "16:9".
        #[arg(long)]
        ratio: Option<String>,
        /// Editor: highlight the recognised text.
        #[arg(long)]
        highlight: bool,
        /// Editor: select this OCR line.
        #[arg(long)]
        select: Option<usize>,
        /// Editor: stay in the loading state.
        #[arg(long)]
        loading: bool,
        /// Editor: show the "no longer available" state.
        #[arg(long = "failed-load")]
        load_failed: bool,
        /// Region: a selection in progress, as "x,y,w,h" in points.
        #[arg(long)]
        drag: Option<String>,
    },
    /// Helpers ss runs itself.
    #[command(subcommand, hide = true)]
    Internal(InternalCommand),
    /// Developer tools (debug builds only).
    #[command(subcommand, hide = !DEBUG_COMMANDS_ENABLED)]
    Debug(DebugCommand),
}

#[derive(Subcommand)]
enum InternalCommand {
    /// Print "granted" or "denied" for Screen Recording, from a fresh process.
    ScreenAccess {
        /// Show the macOS prompt if it was never answered.
        #[arg(long)]
        request: bool,
    },
}

#[derive(Subcommand)]
enum DebugCommand {
    /// Print the default settings exactly as they would be written to disk.
    DefaultSettings,
    /// Capture the display under the cursor to a temp file and copy it.
    CaptureScreen,
    /// List displays in top-left global points.
    Displays,
    /// Composite a PNG with the editor style from settings (or defaults):
    /// prints the edge colour and writes the export.
    Compose {
        png: std::path::PathBuf,
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// Put the export on the clipboard, as the editor's Copy does.
        #[arg(long)]
        copy: bool,
    },
    /// Record the display under the pointer through the real recorder,
    /// stopping with SIGINT after `seconds`.
    RecordScreen {
        #[arg(long, default_value_t = 3)]
        seconds: u64,
        #[arg(long)]
        out: std::path::PathBuf,
    },
    /// Write a recording's QuickLook poster frame as PNG.
    Poster {
        movie: std::path::PathBuf,
        #[arg(long)]
        out: std::path::PathBuf,
    },
    /// Put a file reference on the clipboard, as recordings do.
    CopyFile { path: std::path::PathBuf },
    /// Print the setup state (read-only: never prompts or changes settings).
    SetupState,
    /// Recognise text in a PNG with Vision, optionally comparing against an
    /// Electron `.ocr.json`.
    Ocr {
        png: std::path::PathBuf,
        #[arg(long)]
        compare: Option<std::path::PathBuf>,
    },
    /// Show how an Electron accelerator parses.
    ParseAccelerator {
        /// For example "Shift+Command+4".
        accelerator: String,
    },
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();
    let paths = Paths::resolve()?;
    match cli.command.unwrap_or(Command::App) {
        Command::App => {
            logging::init(Some(&paths.log_file))?;
            let Some(_instance) = instance::acquire(&paths.lock_file)? else {
                tracing::info!(lock = %paths.lock_file.display(), "ss is already running");
                return Ok(());
            };
            app::run(paths)
        }
        // Read-only commands log to stderr only so they never write to the data folder.
        Command::Status => {
            logging::init(None)?;
            print_status(&paths);
            Ok(())
        }
        Command::Internal(InternalCommand::ScreenAccess { request }) => {
            println!("{}", system::probe(request).as_str());
            Ok(())
        }
        Command::Preview {
            target,
            step,
            scroll,
            granted,
            failed,
            recording,
            video,
            hover,
            chip,
            background,
            ratio,
            highlight,
            select,
            loading,
            load_failed,
            drag,
        } => {
            if !DEBUG_COMMANDS_ENABLED {
                return Err(eyre!("previews are only available in debug builds"));
            }
            logging::init(None)?;
            preview::run(
                target,
                preview::Options {
                    step,
                    scroll,
                    granted,
                    failed,
                    recording,
                    video,
                    hover,
                    chip,
                    background,
                    ratio,
                    highlight,
                    select,
                    loading,
                    load_failed,
                    drag,
                },
            )
        }
        Command::Debug(command) => {
            if !DEBUG_COMMANDS_ENABLED {
                return Err(eyre!("debug commands are only available in debug builds"));
            }
            logging::init(None)?;
            match command {
                DebugCommand::DefaultSettings => {
                    debug::default_settings();
                    Ok(())
                }
                DebugCommand::CaptureScreen => debug::capture_screen(),
                DebugCommand::Displays => debug::displays(),
                DebugCommand::Ocr { png, compare } => debug::ocr(&png, compare.as_deref()),
                DebugCommand::RecordScreen { seconds, out } => debug::record_screen(seconds, &out),
                DebugCommand::Poster { movie, out } => debug::poster(&movie, &out),
                DebugCommand::CopyFile { path } => {
                    clipboard::write_file_url(&path)?;
                    println!("copied a file reference to {}", path.display());
                    Ok(())
                }
                DebugCommand::Compose { png, out, copy } => {
                    debug::compose(&paths, &png, out.as_deref(), copy)
                }
                DebugCommand::SetupState => {
                    debug::setup_state();
                    Ok(())
                }
                DebugCommand::ParseAccelerator { accelerator } => {
                    debug::parse_accelerator(&accelerator)
                }
            }
        }
    }
}

fn print_status(paths: &Paths) {
    let loaded = settings::read(&paths.settings_file);
    let settings_note = if !paths.settings_file.exists() {
        " (missing, using defaults)"
    } else if loaded.repaired {
        " (⌥-only shortcuts repaired in memory)"
    } else {
        ""
    };
    println!("ss {}", env!("CARGO_PKG_VERSION"));
    println!("Data folder:   {}", paths.data.display());
    println!(
        "Settings file: {}{settings_note}",
        paths.settings_file.display()
    );
    println!(
        "Captures:      {} in {}",
        count_captures(&paths.captures),
        paths.captures.display()
    );
    println!("Log file:      {}", paths.log_file.display());
    println!("Settings:");
    println!("{}", loaded.settings.to_json());
}

/// Captures are `<id>.png` files, each optionally next to `<id>.ocr.json`.
fn count_captures(directory: &Path) -> usize {
    fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
                .count()
        })
        .unwrap_or(0)
}
