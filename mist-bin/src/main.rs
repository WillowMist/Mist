//! Mist — MUD client entry point.
//!
//! The binary is mostly a GUI, but a few things are easier to check
//! from a shell — where the config lives, whether a keyring is
//! reachable, what the version is. Those live behind subcommands so a
//! user can answer "where does Mist keep my password?" without
//! guessing.

use eframe::egui;
use mist_ui::MistApp;

/// What the command line asked for.
enum Command {
    /// Launch the GUI (the default).
    Run,
    /// Print the paths Mist uses.
    ConfigPaths,
    /// Print the version.
    Version,
    /// Print help.
    Help,
}

/// Parse the arguments. The first non-flag argument selects a
/// subcommand; everything else is ignored.
fn parse_args(args: &[String]) -> Command {
    match args.first().map(String::as_str) {
        Some("config") => match args.get(1).map(String::as_str) {
            Some("paths") => Command::ConfigPaths,
            _ => Command::Help,
        },
        Some("version") | Some("--version") | Some("-V") => Command::Version,
        Some("help") | Some("--help") | Some("-h") => Command::Help,
        _ => Command::Run,
    }
}

fn print_help() {
    println!(
        "\
Mist — a MUD client.

Usage:
  mist                     Launch the client.
  mist config paths        Print where Mist keeps its config and credentials.
  mist version             Print the version.
  mist help                This message.

The client itself is the point; these subcommands exist so you can
check where your data lives without opening the GUI.
"
    );
}

/// Print the config paths and the credential-storage situation.
fn print_config_paths() {
    let config_dir = match mist_core::config::config_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("mist: cannot determine the config directory: {e}");
            std::process::exit(1);
        }
    };
    let config_file = match mist_core::config::config_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("mist: cannot determine the config path: {e}");
            std::process::exit(1);
        }
    };

    println!("Config directory: {}", config_dir.display());
    println!("Config file:      {}", config_file.display());

    let keyring = mist_core::credentials::keyring_available();
    if keyring {
        println!(
            "Credentials:       system keyring ({}), service {}",
            keyring_backend(),
            mist_core::credentials::KEYRING_SERVICE
        );
    } else {
        println!(
            "Credentials:       no system keyring found — passwords are stored \
             obscured in the config file. Do not share that file."
        );
    }
}

/// Name the keyring backend, so the user knows which store is holding
/// their secrets.
fn keyring_backend() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macOS Keychain"
    }
    #[cfg(target_os = "windows")]
    {
        "Windows Credential Manager"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "Secret Service"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        "system keyring"
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match parse_args(&args) {
        Command::ConfigPaths => {
            print_config_paths();
            Ok(())
        }
        Command::Version => {
            println!("mist {}", mist_core::VERSION);
            Ok(())
        }
        Command::Help => {
            print_help();
            Ok(())
        }
        Command::Run => run_gui(),
    }
}

/// Launch the GUI.
fn run_gui() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Mist")
            // Wayland compositors identify windows by app_id. Without it a
            // client shows up with an empty class, which breaks window
            // rules, taskbar entries and `hyprctl clients` filtering.
            .with_app_id("com.wyldware.mist")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Mist",
        opts,
        Box::new(|cc| Ok(Box::new(MistApp::new(cc)))),
    )
}
