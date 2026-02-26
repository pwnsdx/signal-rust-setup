#[cfg(not(test))]
use anyhow::Context;
use anyhow::{bail, Result};
use clap::Parser;
use dialoguer::theme::ColorfulTheme;
#[cfg(not(test))]
use dialoguer::{Confirm, Input, Select};
use rand::rngs::OsRng;
use rand::Rng;
#[cfg(not(test))]
use std::fs;
use std::path::Path;
#[cfg(not(test))]
use std::path::PathBuf;

pub mod captcha;
pub mod cli;
pub mod config;
pub mod docker;
pub mod errors;
pub mod qr;
pub mod system;

#[cfg(test)]
use cli::Cli;
#[cfg(not(test))]
use cli::{Cli, Commands};
use config::Config;

use captcha::{capture_captcha_token, get_captcha_token_for_wizard};
use config::{config_from_cli, ensure_account_interactive};
use docker::{
    ensure_docker_ready, list_devices, register_landline, register_with_mode, run_signal_cli,
    set_registration_lock_pin, verify_code,
};
use qr::{decode_signal_qr_from_image, scan_screen_for_signal_uri};
use system::{command_exists, open_screen_recording_settings, open_signal_desktop};

#[cfg(test)]
pub(crate) use captcha::capture_captcha_token_subprocess;
#[cfg(test)]
pub(crate) use config::{default_data_dir, validate_account};
#[cfg(test)]
pub(crate) use docker::{docker_daemon_is_ready, run_signal_cli_with_retries, try_start_docker};
#[cfg(test)]
pub(crate) use qr::{
    capture_screen_image, capture_screen_images, capture_screens_for_attempt,
    decode_signal_qr_with_rqrr, decode_signal_qr_with_rqrr_multipass, decode_signal_qr_with_rxing,
    detect_display_count, resize_luma_to_max_dimension, scale_luma_image, threshold_luma_image,
};
#[cfg(test)]
pub(crate) use system::{
    is_signal_desktop_running, open_url_in_default_browser, process_running_exact,
    process_running_fuzzy,
};

pub const DEFAULT_IMAGE: &str = "registry.gitlab.com/packaging/signal-cli/signal-cli-native:latest";
#[cfg(not(test))]
pub(crate) const CAPTCHA_URL: &str = "https://signalcaptchas.org/registration/generate.html";
pub const DEFAULT_SCAN_INTERVAL: u64 = 2;
pub const DEFAULT_SCAN_ATTEMPTS: u32 = 90;
pub(crate) const REGISTER_RETRY_ATTEMPTS: u32 = 3;
pub(crate) const REGISTER_RETRY_DELAY_SECS: u64 = 8;
#[cfg(not(test))]
pub(crate) const DOCKER_START_TIMEOUT_SECS: u64 = 90;
#[cfg(test)]
pub(crate) const DOCKER_START_TIMEOUT_SECS: u64 = 2;
pub(crate) const GENERATED_REGISTRATION_PIN_DIGITS: usize = 20;
pub(crate) const POST_LINK_SYNC_PASSES: u32 = 3;
pub(crate) const POST_LINK_RECEIVE_TIMEOUT_SECS: u64 = 12;
pub(crate) const POST_LINK_RECEIVE_MAX_MESSAGES: u32 = 100;
#[cfg(not(test))]
pub(crate) const SCREEN_CAPTURE_TIMEOUT_SECS: u64 = 12;
#[cfg(test)]
pub(crate) const SCREEN_CAPTURE_TIMEOUT_SECS: u64 = 1;
#[cfg(not(test))]
pub(crate) const QR_FAST_MAX_DIMENSION: u32 = 1600;
#[cfg(not(test))]
pub(crate) const QR_RXING_MAX_PIXELS: u64 = 3_000_000;
pub(crate) const MAX_DETECTED_DISPLAYS: usize = 6;
#[cfg(not(test))]
pub(crate) const LANDLINE_WAIT_SECS: u64 = 60;
#[cfg(test)]
pub(crate) const LANDLINE_WAIT_SECS: u64 = 1;
#[cfg(not(test))]
pub(crate) const SIGNAL_LAUNCH_WAIT_LOOPS: u32 = 12;
#[cfg(test)]
pub(crate) const SIGNAL_LAUNCH_WAIT_LOOPS: u32 = 2;
#[cfg(not(test))]
pub(crate) const SIGNAL_LAUNCH_WAIT_MS: u64 = 500;
#[cfg(test)]
pub(crate) const SIGNAL_LAUNCH_WAIT_MS: u64 = 1;

#[cfg(not(test))]
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let command = cli.command.clone().unwrap_or(Commands::Wizard);

    match command {
        Commands::Wizard => cmd_wizard(&cli),
        Commands::CaptchaToken { quiet } => {
            let token = capture_captcha_token(quiet)?;
            println!("{token}");
            Ok(())
        }
        Commands::Register {
            token,
            voice,
            landline,
        } => {
            let cfg = config_from_cli(&cli, true)?;
            ensure_docker_ready()?;
            if landline {
                register_landline(&cfg, &token)
            } else {
                register_with_mode(&cfg, &token, voice)
            }
        }
        Commands::Verify { code, pin } => {
            let cfg = config_from_cli(&cli, true)?;
            ensure_docker_ready()?;
            verify_code(&cfg, &code, pin.as_deref())
        }
        Commands::LinkDesktopLive { interval, attempts } => {
            let cfg = config_from_cli(&cli, true)?;
            ensure_docker_ready()?;
            link_desktop_live(&cfg, interval, attempts)
        }
        Commands::ListDevices => {
            let cfg = config_from_cli(&cli, true)?;
            ensure_docker_ready()?;
            list_devices(&cfg)
        }
    }
}

#[cfg(test)]
pub fn run() -> Result<()> {
    Ok(())
}

#[cfg(not(test))]
fn cmd_wizard(cli: &Cli) -> Result<()> {
    ensure_docker_ready()?;

    let theme = ColorfulTheme::default();
    let mut cfg = config_from_cli(cli, false)?;
    cfg.account = ensure_account_interactive(cli.account.clone(), &theme)?;

    fs::create_dir_all(&cfg.data_dir)
        .with_context(|| format!("failed to create data dir {}", cfg.data_dir.display()))?;

    println!("\n== Signal Setup Wizard ==");
    println!("Account : {}", cfg.account);
    println!("Data dir: {}", cfg.data_dir.display());
    println!("Image   : {}", cfg.image);

    println!("\nOpening captcha page in embedded browser...");
    let mut token = get_captcha_token_for_wizard(&theme)?;
    println!("Captcha token captured.");

    let modes = [
        "SMS",
        "Landline/SIP (SMS attempt, wait 60s, then voice)",
        "Voice only",
    ];
    let mode = Select::with_theme(&theme)
        .with_prompt("Registration mode")
        .items(&modes)
        .default(1)
        .interact()?;

    loop {
        let registration_result = match mode {
            0 => register_with_mode(&cfg, &token, false),
            1 => register_landline(&cfg, &token),
            2 => register_with_mode(&cfg, &token, true),
            _ => unreachable!(),
        };

        match registration_result {
            Ok(_) => break,
            Err(err) => {
                eprintln!("\nRegistration failed: {err}");
                eprintln!(
                    "If you saw StatusCode 502 (ExternalServiceFailureException), it is often temporary."
                );
                eprintln!("{}", registration_failure_hint());

                let retry_same = Confirm::with_theme(&theme)
                    .with_prompt("Retry registration with the same captcha token?")
                    .default(true)
                    .interact()?;
                if retry_same {
                    continue;
                }

                let regenerate = Confirm::with_theme(&theme)
                    .with_prompt("Generate a new captcha token and retry?")
                    .default(true)
                    .interact()?;
                if regenerate {
                    println!("\nOpening captcha page in embedded browser...");
                    token = get_captcha_token_for_wizard(&theme)?;
                    println!("New captcha token captured.");
                    continue;
                }

                return Err(err);
            }
        }
    }

    let code: String = Input::with_theme(&theme)
        .with_prompt("Verification code received by SMS/voice")
        .interact_text()?;

    let has_existing_pin = Confirm::with_theme(&theme)
        .with_prompt("Do you already have a registration lock PIN on this number?")
        .default(false)
        .interact()?;

    let existing_pin = if has_existing_pin {
        Some(
            Input::<String>::with_theme(&theme)
                .with_prompt("Existing registration lock PIN")
                .interact_text()?,
        )
    } else {
        None
    };

    verify_code(&cfg, &code, existing_pin.as_deref())?;
    println!("Registration verified.");

    let generated_pin = generate_long_registration_lock_pin();
    let pretty_generated_pin = format_pin_for_display(&generated_pin, 4);
    println!("\nIMPORTANT: Save this registration lock PIN now.");
    println!("Registration lock PIN: {pretty_generated_pin}");
    println!("Store it in a password manager. You will need it to re-register this number.");

    while !Confirm::with_theme(&theme)
        .with_prompt("Have you saved this PIN?")
        .default(false)
        .interact()?
    {
        println!("Please save it before continuing.");
        println!("Registration lock PIN: {pretty_generated_pin}");
    }

    set_registration_lock_pin(&cfg, &generated_pin)?;
    println!("Registration lock PIN configured.");

    let do_link = Confirm::with_theme(&theme)
        .with_prompt("Link Signal Desktop now?")
        .default(true)
        .interact()?;
    if !do_link {
        println!("Done. Registration completed without desktop linking.");
        return Ok(());
    }

    let interval = DEFAULT_SCAN_INTERVAL;
    let attempts = DEFAULT_SCAN_ATTEMPTS;
    println!("Using default QR scan settings: every {interval}s, max {attempts} attempts.");

    link_desktop_interactive(&cfg, &theme, interval, attempts)?;
    println!("\nSetup completed successfully.");
    Ok(())
}

#[cfg(test)]
fn cmd_wizard(_cli: &Cli) -> Result<()> {
    Ok(())
}

fn registration_failure_hint() -> &'static str {
    "If this persists: the number/operator may be blocked, or your current IP may be rate-limited. Try another network/IP (for example mobile hotspot) or another number/operator."
}

fn format_watch_duration(total_seconds: u64) -> String {
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;

    if minutes > 0 && seconds == 0 {
        if minutes == 1 {
            "1 minute".to_string()
        } else {
            format!("{minutes} minutes")
        }
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else if total_seconds == 1 {
        "1 second".to_string()
    } else {
        format!("{total_seconds} seconds")
    }
}

fn generate_long_registration_lock_pin() -> String {
    let mut rng = OsRng;
    let mut pin = String::with_capacity(GENERATED_REGISTRATION_PIN_DIGITS);

    for _ in 0..GENERATED_REGISTRATION_PIN_DIGITS {
        let digit = rng.gen_range(0_u8..10_u8);
        pin.push((b'0' + digit) as char);
    }

    pin
}

fn format_pin_for_display(pin: &str, chunk_size: usize) -> String {
    if chunk_size == 0 {
        return pin.to_string();
    }

    pin.chars()
        .collect::<Vec<_>>()
        .chunks(chunk_size)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

fn link_desktop_live(cfg: &Config, interval: u64, attempts: u32) -> Result<()> {
    if interval == 0 || attempts == 0 {
        bail!("interval and attempts must be > 0")
    }

    if !command_exists("screencapture") {
        #[cfg(target_os = "macos")]
        {
            bail!("screencapture is required (macOS)")
        }
    }

    if open_signal_desktop() {
        println!("Signal Desktop launch requested.");
    } else {
        println!("Could not auto-launch Signal Desktop. Open it manually.");
    }
    println!("Ensure the Signal Desktop pairing QR is visible on screen.");

    let watch_seconds = interval.saturating_mul(attempts as u64);
    let watch_text = format_watch_duration(watch_seconds);
    println!("Watching the screen for up to {watch_text}.");
    println!("Scanning every {interval}s (max {attempts} attempts)...");
    println!("If prompted, grant Screen Recording permission to this terminal app.");

    let uri = scan_screen_for_signal_uri(interval, attempts)?;
    println!("Valid QR detected. Linking device...");

    link_desktop_from_uri(cfg, &uri)
}

#[cfg(not(test))]
fn link_desktop_interactive(
    cfg: &Config,
    theme: &ColorfulTheme,
    interval: u64,
    attempts: u32,
) -> Result<()> {
    loop {
        match link_desktop_live(cfg, interval, attempts) {
            Ok(_) => return Ok(()),
            Err(err) => {
                eprintln!("\nLive QR scan failed: {err}");
                eprintln!(
                    "If you saw 'could not create image from display', grant Screen Recording permission to your terminal app in System Settings > Privacy & Security > Screen Recording."
                );

                if Confirm::with_theme(theme)
                    .with_prompt("Open Screen Recording settings now?")
                    .default(true)
                    .interact()?
                {
                    open_screen_recording_settings();
                }

                let options = [
                    "Retry live scan",
                    "Use screenshot file",
                    "Paste sgnl:// URI manually",
                    "Skip desktop linking",
                ];
                let next = Select::with_theme(theme)
                    .with_prompt("Choose next step")
                    .items(&options)
                    .default(0)
                    .interact()?;

                match next {
                    0 => continue,
                    1 => {
                        let path_input: String = Input::with_theme(theme)
                            .with_prompt("Path to screenshot file containing the Signal QR")
                            .interact_text()?;
                        let path = PathBuf::from(path_input);
                        link_desktop_from_image(cfg, &path)?;
                        return Ok(());
                    }
                    2 => {
                        let uri: String = Input::with_theme(theme)
                            .with_prompt("Paste full sgnl://linkdevice URI")
                            .interact_text()?;
                        link_desktop_from_uri(cfg, &uri)?;
                        return Ok(());
                    }
                    3 => {
                        println!("Skipping desktop linking for now.");
                        return Ok(());
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}

#[cfg(test)]
fn link_desktop_interactive(
    _cfg: &Config,
    _theme: &ColorfulTheme,
    _interval: u64,
    _attempts: u32,
) -> Result<()> {
    Ok(())
}

fn link_desktop_from_image(cfg: &Config, path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("screenshot file not found: {}", path.display())
    }

    let uri = decode_signal_qr_from_image(path)?.ok_or_else(|| {
        anyhow::anyhow!("no valid sgnl://linkdevice QR found in {}", path.display())
    })?;
    link_desktop_from_uri(cfg, &uri)
}

fn link_desktop_from_uri(cfg: &Config, uri: &str) -> Result<()> {
    if !uri.starts_with("sgnl://linkdevice") {
        bail!("invalid URI: expected sgnl://linkdevice...")
    }

    let args = vec![
        "addDevice".to_string(),
        "--uri".to_string(),
        uri.to_string(),
    ];
    run_signal_cli(cfg, &args, false)?;

    run_post_link_sync(cfg);

    println!("Linked devices:");
    list_devices(cfg)?;
    Ok(())
}

fn run_post_link_sync(cfg: &Config) {
    let total_wait = POST_LINK_SYNC_PASSES as u64 * POST_LINK_RECEIVE_TIMEOUT_SECS;
    println!("Finalizing initial contacts/groups sync from the primary device...");
    println!(
        "Keeping this process active helps avoid Signal Desktop staying on 'Syncing contacts and groups'."
    );
    println!("Sync window: up to {}s.", total_wait);

    let receive_args = vec![
        "receive".to_string(),
        "--timeout".to_string(),
        POST_LINK_RECEIVE_TIMEOUT_SECS.to_string(),
        "--max-messages".to_string(),
        POST_LINK_RECEIVE_MAX_MESSAGES.to_string(),
    ];

    for pass in 1..=POST_LINK_SYNC_PASSES {
        println!("Sync pass {pass}/{POST_LINK_SYNC_PASSES}: waiting for pending sync requests...");
        match run_signal_cli(cfg, &receive_args, true) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("Warning: receive pass {pass} failed.");
                eprintln!(
                    "Desktop may still complete sync after restart. See README troubleshooting for a manual docker receive command."
                );
                break;
            }
            Err(err) => {
                eprintln!("Warning: receive pass {pass} error: {err}");
                eprintln!(
                    "Desktop may still complete sync after restart. See README troubleshooting for a manual docker receive command."
                );
                break;
            }
        }
    }

    println!("Sending a contacts sync message to linked devices...");
    let send_contacts_args = vec!["sendContacts".to_string()];
    match run_signal_cli(cfg, &send_contacts_args, true) {
        Ok(true) => {
            println!("Contacts sync message sent.");
        }
        Ok(false) => {
            eprintln!("Warning: sendContacts failed.");
        }
        Err(err) => {
            eprintln!("Warning: sendContacts error: {err}");
        }
    }
}

#[cfg(test)]
mod tests;
