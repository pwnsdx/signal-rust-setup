use anyhow::{anyhow, bail, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::errors::SignalSetupError;
use crate::system::command_exists;

pub fn ensure_docker_ready() -> Result<()> {
    if !command_exists("docker") {
        return Err(SignalSetupError::DockerNotInstalled.into());
    }

    if docker_daemon_is_ready()? {
        return Ok(());
    }

    println!("Docker is installed but daemon is not running. Attempting to start Docker...");
    if !try_start_docker() {
        return Err(SignalSetupError::DockerStartFailed.into());
    }

    let wait_pb = ProgressBar::new(crate::DOCKER_START_TIMEOUT_SECS);
    let wait_style = ProgressStyle::with_template(
        "{spinner:.green} [{bar:30.cyan/blue}] {pos}/{len}s waiting for Docker daemon...",
    )
    .unwrap_or_else(|_| ProgressStyle::default_bar())
    .progress_chars("=> ");
    wait_pb.set_style(wait_style);
    wait_pb.enable_steady_tick(Duration::from_millis(120));

    let start = Instant::now();
    let timeout = Duration::from_secs(crate::DOCKER_START_TIMEOUT_SECS);
    let mut sleep_ms = 150_u64;

    while start.elapsed() < timeout {
        if docker_daemon_is_ready()? {
            wait_pb.finish_with_message("Docker daemon is ready.");
            return Ok(());
        }

        let elapsed = start
            .elapsed()
            .as_secs()
            .min(crate::DOCKER_START_TIMEOUT_SECS);
        wait_pb.set_position(elapsed);
        thread::sleep(Duration::from_millis(sleep_ms));
        sleep_ms = (sleep_ms.saturating_mul(2)).min(1000);
    }

    wait_pb.abandon_with_message("Docker daemon did not become ready in time.");
    Err(SignalSetupError::DockerStartTimeout {
        seconds: crate::DOCKER_START_TIMEOUT_SECS,
    }
    .into())
}

pub fn docker_daemon_is_ready() -> Result<bool> {
    let status = Command::new("docker")
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to run docker info")?;
    Ok(status.success())
}

pub fn try_start_docker() -> bool {
    #[cfg(target_os = "macos")]
    {
        if command_exists("open")
            && Command::new("open")
                .args(["-a", "Docker"])
                .status()
                .is_ok_and(|s| s.success())
        {
            return true;
        }
        if open::that("/Applications/Docker.app").is_ok() || open::that("Docker").is_ok() {
            return true;
        }
        false
    }

    #[cfg(target_os = "linux")]
    {
        if command_exists("systemctl") {
            if let Ok(status) = Command::new("systemctl")
                .args(["--user", "start", "docker-desktop"])
                .status()
            {
                if status.success() {
                    return true;
                }
            }

            if let Ok(status) = Command::new("systemctl").args(["start", "docker"]).status() {
                if status.success() {
                    return true;
                }
            }
        }
        false
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

pub fn register_with_mode(cfg: &Config, token: &str, voice: bool) -> Result<()> {
    let mut args = vec![
        "register".to_string(),
        "--captcha".to_string(),
        token.to_string(),
    ];
    if voice {
        args.push("--voice".to_string());
    }

    run_signal_cli_with_retries(
        cfg,
        &args,
        crate::REGISTER_RETRY_ATTEMPTS,
        crate::REGISTER_RETRY_DELAY_SECS,
        "registration",
    )?;
    Ok(())
}

pub fn register_landline(cfg: &Config, token: &str) -> Result<()> {
    println!("Step 1/3: SMS registration attempt...");
    let sms_args = vec![
        "register".to_string(),
        "--captcha".to_string(),
        token.to_string(),
    ];
    let sms_ok = run_signal_cli(cfg, &sms_args, true)?;
    if !sms_ok {
        println!("SMS failed (expected for voice-only numbers). Continuing...");
    }

    println!("Step 2/3: waiting {} seconds...", crate::LANDLINE_WAIT_SECS);
    let wait_pb = ProgressBar::new(crate::LANDLINE_WAIT_SECS);
    let wait_style =
        ProgressStyle::with_template("{spinner:.green} [{bar:30.magenta/blue}] {pos}/{len}s")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=> ");
    wait_pb.set_style(wait_style);
    wait_pb.enable_steady_tick(Duration::from_millis(120));
    for _ in 0..crate::LANDLINE_WAIT_SECS {
        wait_pb.inc(1);
        thread::sleep(Duration::from_secs(1));
    }
    wait_pb.finish_with_message("Wait complete.");

    println!("Step 3/3: voice registration...");
    let voice_args = vec![
        "register".to_string(),
        "--voice".to_string(),
        "--captcha".to_string(),
        token.to_string(),
    ];
    run_signal_cli_with_retries(
        cfg,
        &voice_args,
        crate::REGISTER_RETRY_ATTEMPTS,
        crate::REGISTER_RETRY_DELAY_SECS,
        "voice registration",
    )?;
    Ok(())
}

pub fn run_signal_cli_with_retries(
    cfg: &Config,
    args: &[String],
    attempts: u32,
    delay_secs: u64,
    label: &str,
) -> Result<()> {
    if attempts == 0 {
        bail!("{label} attempts must be > 0")
    }

    for attempt in 1..=attempts {
        let ok = run_signal_cli(cfg, args, true)?;
        if ok {
            return Ok(());
        }

        if attempt < attempts {
            println!("{label} failed (attempt {attempt}/{attempts}). Retrying in {delay_secs}s...");
            thread::sleep(Duration::from_secs(delay_secs));
        }
    }

    bail!(
        "{label} failed after {attempts} attempts. {}",
        registration_failure_hint()
    )
}

pub fn verify_code(cfg: &Config, code: &str, pin: Option<&str>) -> Result<()> {
    if let Some(pin_value) = pin {
        run_signal_cli_with_stdin_secret(
            cfg,
            "verify",
            "read -r SIGNAL_VERIFY_CODE; read -r SIGNAL_PIN; signal-cli -o json -a \"$SIGNAL_ACCOUNT\" verify \"$SIGNAL_VERIFY_CODE\" --pin \"$SIGNAL_PIN\"",
            &format!("{code}\n{pin_value}\n"),
            false,
        )?;
    } else {
        let args = vec!["verify".to_string(), code.to_string()];
        run_signal_cli(cfg, &args, false)?;
    }
    Ok(())
}

pub fn set_registration_lock_pin(cfg: &Config, pin: &str) -> Result<()> {
    run_signal_cli_with_stdin_secret(
        cfg,
        "setPin",
        "read -r SIGNAL_PIN; signal-cli -o json -a \"$SIGNAL_ACCOUNT\" setPin \"$SIGNAL_PIN\"",
        &format!("{pin}\n"),
        false,
    )?;
    Ok(())
}

pub fn list_devices(cfg: &Config) -> Result<()> {
    let args = vec!["listDevices".to_string()];
    run_signal_cli(cfg, &args, false)?;
    Ok(())
}

pub fn run_signal_cli(cfg: &Config, args: &[String], allow_failure: bool) -> Result<bool> {
    fs::create_dir_all(&cfg.data_dir)
        .with_context(|| format!("failed to create data dir {}", cfg.data_dir.display()))?;

    let command_name = args.first().map(String::as_str).unwrap_or("unknown");
    let mut cmd = base_docker_run_cmd(cfg);
    cmd.arg(&cfg.image)
        .arg("-o")
        .arg("json")
        .arg("-a")
        .arg(&cfg.account)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd
        .output()
        .with_context(|| format!("failed to run signal-cli '{command_name}' command"))?;
    handle_signal_cli_output(command_name, output, allow_failure)
}

fn run_signal_cli_with_stdin_secret(
    cfg: &Config,
    command_name: &str,
    shell_script: &str,
    stdin_payload: &str,
    allow_failure: bool,
) -> Result<bool> {
    fs::create_dir_all(&cfg.data_dir)
        .with_context(|| format!("failed to create data dir {}", cfg.data_dir.display()))?;

    let mut cmd = base_docker_run_cmd(cfg);
    cmd.arg("--env")
        .arg(format!("SIGNAL_ACCOUNT={}", cfg.account))
        .arg("--entrypoint")
        .arg("sh")
        .arg(&cfg.image)
        .arg("-c")
        .arg(shell_script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to run signal-cli '{command_name}' command"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(stdin_payload.as_bytes())
            .with_context(|| format!("failed to send secret input to '{command_name}' command"))?;
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("failed to wait for signal-cli '{command_name}' command"))?;
    handle_signal_cli_output(command_name, output, allow_failure)
}

fn base_docker_run_cmd(cfg: &Config) -> Command {
    let volume = format!("{}:/var/lib/signal-cli", cfg.data_dir.display());
    let mut cmd = Command::new("docker");
    cmd.arg("run")
        .arg("--rm")
        .arg("-i")
        .arg("--volume")
        .arg(volume)
        .arg("--tmpfs")
        .arg("/tmp:exec");
    add_linux_user_mapping(&mut cmd);
    cmd
}

#[cfg(target_os = "linux")]
fn add_linux_user_mapping(cmd: &mut Command) {
    let uid = unsafe { libc::geteuid() };
    let gid = unsafe { libc::getegid() };
    cmd.arg("--user").arg(format!("{uid}:{gid}"));
}

#[cfg(not(target_os = "linux"))]
fn add_linux_user_mapping(_cmd: &mut Command) {}

fn handle_signal_cli_output(
    command_name: &str,
    output: std::process::Output,
    allow_failure: bool,
) -> Result<bool> {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        emit_signal_output(command_name, &stdout, &stderr, true);
        return Ok(true);
    }

    emit_signal_output(command_name, &stdout, &stderr, false);

    if allow_failure {
        return Ok(false);
    }

    if command_name == "register" {
        if is_rate_limited(&stdout, &stderr) {
            return Err(SignalSetupError::SignalCliRateLimited.into());
        }
        return Err(SignalSetupError::RegisterFailed.into());
    }

    if is_rate_limited(&stdout, &stderr) {
        return Err(SignalSetupError::SignalCliRateLimited.into());
    }

    Err(SignalSetupError::SignalCliCommandFailed {
        command: command_name.to_string(),
    }
    .into())
}

fn is_rate_limited(stdout: &str, stderr: &str) -> bool {
    let content = format!("{stdout}\n{stderr}");
    content.contains("ExternalServiceFailureException")
        || content.contains("StatusCode: 502")
        || content.contains("StatusCode: 429")
        || content.contains("RateLimit")
}

fn emit_signal_output(command_name: &str, stdout: &str, stderr: &str, success: bool) {
    let stdout_trimmed = stdout.trim();
    if !stdout_trimmed.is_empty() {
        if let Ok(json) = serde_json::from_str::<Value>(stdout_trimmed) {
            print_json_output(command_name, &json);
        } else {
            println!("{stdout_trimmed}");
        }
    }

    let stderr_trimmed = stderr.trim();
    if stderr_trimmed.is_empty() {
        return;
    }

    if success {
        // Keep useful info messages from signal-cli when successful.
        eprintln!("{stderr_trimmed}");
        return;
    }

    let first_meaningful = stderr_trimmed
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(stderr_trimmed);
    eprintln!("{first_meaningful}");
}

fn print_json_output(command_name: &str, json: &Value) {
    match command_name {
        "listDevices" => {
            if let Ok(pretty) = serde_json::to_string_pretty(json) {
                println!("{pretty}");
            } else {
                println!("{json}");
            }
        }
        _ => {
            if json.is_null() {
                return;
            }
            if let Some(obj) = json.as_object() {
                if obj.is_empty() {
                    return;
                }
            }
            if let Ok(pretty) = serde_json::to_string_pretty(json) {
                println!("{pretty}");
            } else {
                println!("{json}");
            }
        }
    }
}

fn registration_failure_hint() -> &'static str {
    "If this persists: the number/operator may be blocked, or your current IP may be rate-limited. Try another network/IP (for example mobile hotspot) or another number/operator."
}

pub fn extract_signal_captcha_token_from_output(output: &[u8]) -> Result<String> {
    let stdout = String::from_utf8_lossy(output);
    for line in stdout.lines().rev() {
        let token = line.trim();
        if token.starts_with("signalcaptcha://") {
            return Ok(token.to_string());
        }
    }
    Err(anyhow!("captcha-token subprocess did not return a token"))
}
