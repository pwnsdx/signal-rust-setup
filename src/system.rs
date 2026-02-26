use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use sysinfo::{ProcessRefreshKind, RefreshKind, System};
use which::which;

pub fn command_exists(name: &str) -> bool {
    which(name).is_ok()
}

pub fn open_url_in_default_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        if command_exists("open") {
            let _ = Command::new("open").arg(url).status();
            return;
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if command_exists("xdg-open") {
            let _ = Command::new("xdg-open").arg(url).status();
            return;
        }
    }

    let _ = open::that(url);
}

pub fn open_screen_recording_settings() {
    #[cfg(target_os = "macos")]
    {
        let settings_url =
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
        if command_exists("open") {
            let _ = Command::new("open").arg(settings_url).status();
            return;
        }
        let _ = open::that(settings_url);
    }
}

fn process_name_to_string(name: &str) -> String {
    name.to_string()
}

fn process_cmd_to_string(cmd: &[String]) -> String {
    cmd.join(" ")
}

fn process_snapshot() -> System {
    let mut system = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    system.refresh_processes();
    system
}

fn mock_process_running(target: &str) -> Option<bool> {
    let has_mock = std::env::var_os("MOCK_PGREP_MATCH").is_some()
        || std::env::var_os("MOCK_PGREP_EXIT").is_some()
        || std::env::var_os("MOCK_PGREP_FAILS").is_some();
    if !has_mock {
        return None;
    }

    let fails = std::env::var("MOCK_PGREP_FAILS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);

    if fails > 0 {
        if let Ok(counter_path) = std::env::var("MOCK_PGREP_COUNTER_FILE") {
            let mut count = std::fs::read_to_string(&counter_path)
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok())
                .unwrap_or(0);
            count += 1;
            let _ = std::fs::write(&counter_path, count.to_string());
            if count <= fails {
                return Some(false);
            }
        }
    }

    let match_value = std::env::var("MOCK_PGREP_MATCH").unwrap_or_default();
    if !match_value.is_empty() {
        return Some(target == match_value);
    }

    let exit = std::env::var("MOCK_PGREP_EXIT")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(1);
    Some(exit == 0)
}

pub fn process_running_exact(name: &str) -> bool {
    if let Some(mocked) = mock_process_running(name) {
        return mocked;
    }

    let system = process_snapshot();
    system
        .processes()
        .values()
        .any(|process| process_name_to_string(process.name()) == name)
}

pub fn process_running_fuzzy(pattern: &str) -> bool {
    if let Some(mocked) = mock_process_running(pattern) {
        return mocked;
    }

    let needle = pattern.to_lowercase();
    let system = process_snapshot();
    system.processes().values().any(|process| {
        let name = process_name_to_string(process.name()).to_lowercase();
        let cmd = process_cmd_to_string(process.cmd()).to_lowercase();
        name.contains(&needle) || cmd.contains(&needle)
    })
}

pub fn is_signal_desktop_running() -> bool {
    process_running_exact("Signal")
        || process_running_exact("signal-desktop")
        || process_running_fuzzy("Signal.app")
        || process_running_fuzzy("signal-desktop")
}

pub fn open_signal_desktop() -> bool {
    if is_signal_desktop_running() {
        return true;
    }

    let mut launch_attempted = false;

    #[cfg(target_os = "macos")]
    {
        if command_exists("open")
            && Command::new("open")
                .args(["-a", "Signal"])
                .status()
                .is_ok_and(|s| s.success())
        {
            launch_attempted = true;
        }
        if command_exists("open")
            && Command::new("open")
                .args(["-a", "Signal Desktop"])
                .status()
                .is_ok_and(|s| s.success())
        {
            launch_attempted = true;
        }
        if command_exists("open")
            && Command::new("open")
                .arg("/Applications/Signal.app")
                .status()
                .is_ok_and(|s| s.success())
        {
            launch_attempted = true;
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if open::that("signal-desktop").is_ok() {
            launch_attempted = true;
        }
    }

    if Command::new("signal-desktop")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
    {
        launch_attempted = true;
    }

    if Command::new("signal")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
    {
        launch_attempted = true;
    }

    if !launch_attempted {
        return false;
    }

    let max_sleep_ms = crate::SIGNAL_LAUNCH_WAIT_MS.max(1);
    let mut sleep_ms = max_sleep_ms.min(120);

    for _ in 0..crate::SIGNAL_LAUNCH_WAIT_LOOPS {
        if is_signal_desktop_running() {
            return true;
        }
        thread::sleep(Duration::from_millis(sleep_ms));
        sleep_ms = (sleep_ms.saturating_mul(2)).min(max_sleep_ms);
    }

    launch_attempted
}
