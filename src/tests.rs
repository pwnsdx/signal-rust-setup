use super::*;
use image::{GrayImage, Luma};
use qrcode::QrCode;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use tempfile::TempDir;

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

struct TestEnv {
    _guard: MutexGuard<'static, ()>,
    bin_dir: TempDir,
    home_dir: TempDir,
    old_path: Option<OsString>,
    old_home: Option<OsString>,
}

impl TestEnv {
    fn new() -> Self {
        let guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let bin_dir = TempDir::new().expect("temp bin dir");
        let home_dir = TempDir::new().expect("temp home dir");
        let old_path = env::var_os("PATH");
        let old_home = env::var_os("HOME");

        let this = Self {
            _guard: guard,
            bin_dir,
            home_dir,
            old_path,
            old_home,
        };

        this.set_path_with_system_bins();
        env::set_var("HOME", this.home_dir.path());
        this.clear_mock_env();
        this
    }

    fn set_path_with_system_bins(&self) {
        env::set_var(
            "PATH",
            format!("{}:/bin:/usr/bin:/usr/sbin", self.bin_dir.path().display()),
        );
    }

    fn set_path_minimal(&self) {
        env::set_var("PATH", format!("{}:/bin", self.bin_dir.path().display()));
    }

    fn clear_mock_env(&self) {
        let keys = [
            "MOCK_DOCKER_LOG",
            "MOCK_DOCKER_INFO_EXIT",
            "MOCK_DOCKER_INFO_FAILS",
            "MOCK_DOCKER_INFO_COUNTER_FILE",
            "MOCK_DOCKER_STDOUT",
            "MOCK_DOCKER_STDERR",
            "MOCK_DOCKER_REGISTER_EXIT",
            "MOCK_DOCKER_REGISTER_FAILS",
            "MOCK_DOCKER_COUNTER_FILE",
            "MOCK_DOCKER_VERIFY_EXIT",
            "MOCK_DOCKER_SETPIN_EXIT",
            "MOCK_DOCKER_LISTDEVICES_EXIT",
            "MOCK_DOCKER_ADDDEVICE_EXIT",
            "MOCK_DOCKER_RECEIVE_EXIT",
            "MOCK_DOCKER_SENDCONTACTS_EXIT",
            "MOCK_DOCKER_RUN_EXIT",
            "MOCK_DOCKER_DEFAULT_EXIT",
            "MOCK_SCREENCAPTURE_EXIT",
            "MOCK_SCREENCAPTURE_SLEEP",
            "MOCK_SCREENCAPTURE_FAIL_MULTI",
            "MOCK_SCREENSHOT_SOURCE",
            "MOCK_SP_FAIL",
            "MOCK_OPEN_LOG",
            "MOCK_OPEN_EXIT",
            "MOCK_PGREP_LOG",
            "MOCK_PGREP_MATCH",
            "MOCK_PGREP_EXIT",
            "MOCK_PGREP_FAILS",
            "MOCK_PGREP_COUNTER_FILE",
        ];

        for key in keys {
            env::remove_var(key);
        }
    }

    fn write_script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.bin_dir.path().join(name);
        let mut file = File::create(&path).expect("create script");
        file.write_all(body.as_bytes()).expect("write script");
        let mut perms = file
            .metadata()
            .expect("script metadata")
            .permissions()
            .to_owned();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("chmod script");
        path
    }

    fn set_var(&self, key: &str, value: &str) {
        env::set_var(key, value);
    }

    fn cfg(&self) -> Config {
        Config {
            account: "+10000000000".to_string(),
            data_dir: self.home_dir.path().join("signal-data"),
            image: "mock/signal-cli:latest".to_string(),
        }
    }

    fn log_path(&self, name: &str) -> PathBuf {
        self.home_dir.path().join(name)
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        if let Some(path) = &self.old_path {
            env::set_var("PATH", path);
        } else {
            env::remove_var("PATH");
        }

        if let Some(home) = &self.old_home {
            env::set_var("HOME", home);
        } else {
            env::remove_var("HOME");
        }
    }
}

fn install_mock_docker(env_ctx: &TestEnv) {
    env_ctx.write_script(
        "docker",
        r#"#!/bin/sh
set -eu

if [ -n "${MOCK_DOCKER_LOG:-}" ]; then
  echo "$@" >> "$MOCK_DOCKER_LOG"
fi

if [ "${1:-}" = "info" ]; then
  fails="${MOCK_DOCKER_INFO_FAILS:-0}"
  if [ "$fails" -gt 0 ] && [ -n "${MOCK_DOCKER_INFO_COUNTER_FILE:-}" ]; then
    count=0
    if [ -f "$MOCK_DOCKER_INFO_COUNTER_FILE" ]; then
      count=$(cat "$MOCK_DOCKER_INFO_COUNTER_FILE")
    fi
    count=$((count + 1))
    echo "$count" > "$MOCK_DOCKER_INFO_COUNTER_FILE"
    if [ "$count" -le "$fails" ]; then
      exit 1
    fi
  fi
  exit "${MOCK_DOCKER_INFO_EXIT:-0}"
fi

if [ "${1:-}" != "run" ]; then
  exit "${MOCK_DOCKER_DEFAULT_EXIT:-0}"
fi

cmd=""
for arg in "$@"; do
  case "$arg" in
    *register*) cmd="register" ;;
    *verify*) cmd="verify" ;;
    *setPin*) cmd="setPin" ;;
    *listDevices*) cmd="listDevices" ;;
    *addDevice*) cmd="addDevice" ;;
    *receive*) cmd="receive" ;;
    *sendContacts*) cmd="sendContacts" ;;
  esac
done

if [ -n "${MOCK_DOCKER_STDOUT:-}" ]; then
  printf "%s\n" "$MOCK_DOCKER_STDOUT"
fi

if [ -n "${MOCK_DOCKER_STDERR:-}" ]; then
  printf "%s\n" "$MOCK_DOCKER_STDERR" >&2
fi

if [ "$cmd" = "register" ]; then
  fails="${MOCK_DOCKER_REGISTER_FAILS:-0}"
  if [ "$fails" -gt 0 ] && [ -n "${MOCK_DOCKER_COUNTER_FILE:-}" ]; then
    count=0
    if [ -f "$MOCK_DOCKER_COUNTER_FILE" ]; then
      count=$(cat "$MOCK_DOCKER_COUNTER_FILE")
    fi
    count=$((count + 1))
    echo "$count" > "$MOCK_DOCKER_COUNTER_FILE"
    if [ "$count" -le "$fails" ]; then
      exit 1
    fi
  fi
fi

case "$cmd" in
  register) exit "${MOCK_DOCKER_REGISTER_EXIT:-0}" ;;
  verify) exit "${MOCK_DOCKER_VERIFY_EXIT:-0}" ;;
  setPin) exit "${MOCK_DOCKER_SETPIN_EXIT:-0}" ;;
  listDevices) exit "${MOCK_DOCKER_LISTDEVICES_EXIT:-0}" ;;
  addDevice) exit "${MOCK_DOCKER_ADDDEVICE_EXIT:-0}" ;;
  receive) exit "${MOCK_DOCKER_RECEIVE_EXIT:-0}" ;;
  sendContacts) exit "${MOCK_DOCKER_SENDCONTACTS_EXIT:-0}" ;;
esac

exit "${MOCK_DOCKER_RUN_EXIT:-0}"
"#,
    );
}

fn install_mock_screencapture(env_ctx: &TestEnv) {
    env_ctx.write_script(
        "screencapture",
        r#"#!/bin/sh
set -eu

if [ -n "${MOCK_SCREENCAPTURE_SLEEP:-}" ]; then
  sleep "$MOCK_SCREENCAPTURE_SLEEP"
fi

count=0
for arg in "$@"; do
  case "$arg" in
    -*) ;;
    *) count=$((count + 1)) ;;
  esac
done

if [ "${MOCK_SCREENCAPTURE_FAIL_MULTI:-0}" = "1" ] && [ "$count" -gt 1 ]; then
  exit 1
fi

if [ "${MOCK_SCREENCAPTURE_EXIT:-0}" -ne 0 ]; then
  exit "${MOCK_SCREENCAPTURE_EXIT}"
fi

for arg in "$@"; do
  case "$arg" in
    -*) ;;
    *)
      if [ -n "${MOCK_SCREENSHOT_SOURCE:-}" ]; then
        cp "$MOCK_SCREENSHOT_SOURCE" "$arg"
      else
        : > "$arg"
      fi
      ;;
  esac
done

exit 0
"#,
    );
}

fn install_mock_open(env_ctx: &TestEnv) {
    env_ctx.write_script(
        "open",
        r#"#!/bin/sh
set -eu
if [ -n "${MOCK_OPEN_LOG:-}" ]; then
  echo "$@" >> "$MOCK_OPEN_LOG"
fi
exit "${MOCK_OPEN_EXIT:-0}"
"#,
    );
}

fn install_mock_pgrep(env_ctx: &TestEnv) {
    env_ctx.write_script(
        "pgrep",
        r#"#!/bin/sh
set -eu
if [ -n "${MOCK_PGREP_LOG:-}" ]; then
  echo "$@" >> "$MOCK_PGREP_LOG"
fi

if [ -z "${MOCK_PGREP_MATCH:-}" ]; then
  fails="${MOCK_PGREP_FAILS:-0}"
  if [ "$fails" -gt 0 ] && [ -n "${MOCK_PGREP_COUNTER_FILE:-}" ]; then
    count=0
    if [ -f "$MOCK_PGREP_COUNTER_FILE" ]; then
      count=$(cat "$MOCK_PGREP_COUNTER_FILE")
    fi
    count=$((count + 1))
    echo "$count" > "$MOCK_PGREP_COUNTER_FILE"
    if [ "$count" -le "$fails" ]; then
      exit 1
    fi
  fi
  exit "${MOCK_PGREP_EXIT:-1}"
fi

fails="${MOCK_PGREP_FAILS:-0}"
if [ "$fails" -gt 0 ] && [ -n "${MOCK_PGREP_COUNTER_FILE:-}" ]; then
  count=0
  if [ -f "$MOCK_PGREP_COUNTER_FILE" ]; then
    count=$(cat "$MOCK_PGREP_COUNTER_FILE")
  fi
  count=$((count + 1))
  echo "$count" > "$MOCK_PGREP_COUNTER_FILE"
  if [ "$count" -le "$fails" ]; then
    exit 1
  fi
fi

for arg in "$@"; do
  if [ "$arg" = "$MOCK_PGREP_MATCH" ]; then
    exit 0
  fi
done

exit "${MOCK_PGREP_EXIT:-1}"
"#,
    );
}

fn install_mock_system_profiler(env_ctx: &TestEnv, output: &str) {
    let script = format!(
            "#!/bin/sh\nset -eu\nif [ \"${{MOCK_SP_FAIL:-0}}\" = \"1\" ]; then exit 1; fi\ncat <<'EOF'\n{output}\nEOF\n"
        );
    env_ctx.write_script("system_profiler", &script);
}

fn install_mock_signal_launchers(env_ctx: &TestEnv) {
    env_ctx.write_script("signal-desktop", "#!/bin/sh\nexit 0\n");
    env_ctx.write_script("signal", "#!/bin/sh\nexit 0\n");
}

fn write_blank_png(path: &Path, width: u32, height: u32) {
    let img = GrayImage::from_fn(width, height, |_x, _y| Luma([255]));
    img.save(path).expect("save blank png");
}

fn write_qr_png(path: &Path, data: &str) {
    let qr = QrCode::new(data.as_bytes()).expect("qr");
    let img = qr.render::<Luma<u8>>().module_dimensions(8, 8).build();
    img.save(path).expect("save qr png");
}

fn read_log(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn validate_account_accepts_international_format() {
    assert!(validate_account("+33612345678").is_ok());
    assert!(validate_account("33612345678").is_err());
}

#[test]
fn config_from_cli_requires_account_when_requested() {
    let cli = Cli::parse_from(["app", "list-devices"]);
    let err = config_from_cli(&cli, true).expect_err("expected missing account error");
    assert!(err.to_string().contains("--account is required"));
}

#[test]
fn config_from_cli_builds_config() {
    let cli = Cli::parse_from([
        "app",
        "--account",
        "+33612345678",
        "--data-dir",
        "/tmp/signal-data",
        "--image",
        "image:tag",
        "list-devices",
    ]);
    let cfg = config_from_cli(&cli, true).expect("config");
    assert_eq!(cfg.account, "+33612345678");
    assert_eq!(cfg.data_dir, PathBuf::from("/tmp/signal-data"));
    assert_eq!(cfg.image, "image:tag");
}

#[test]
fn main_and_wizard_test_stubs_are_callable() {
    run().expect("test run entrypoint");
    let cli = Cli::parse_from(["app", "wizard"]);
    cmd_wizard(&cli).expect("test wizard stub");
}

#[test]
fn config_from_cli_allows_empty_account_when_not_required() {
    let cli = Cli::parse_from(["app", "wizard"]);
    let cfg = config_from_cli(&cli, false).expect("config without account");
    assert_eq!(cfg.account, "");
}

#[test]
fn default_data_dir_uses_home_suffix() {
    let env_ctx = TestEnv::new();
    let dir = default_data_dir();
    assert!(dir.starts_with(env_ctx.home_dir.path()));
    assert!(dir.ends_with("signal-cli-data"));
}

#[test]
fn helper_formatters_and_hints_are_correct() {
    assert!(registration_failure_hint().contains("IP"));
    assert_eq!(format_watch_duration(1), "1 second");
    assert_eq!(format_watch_duration(59), "59 seconds");
    assert_eq!(format_watch_duration(60), "1 minute");
    assert_eq!(format_watch_duration(120), "2 minutes");
    assert_eq!(format_watch_duration(121), "2m 1s");
    assert_eq!(format_pin_for_display("12345678", 4), "1234-5678");
    assert_eq!(format_pin_for_display("123456", 0), "123456");
}

#[test]
fn generated_registration_pin_is_numeric_and_long() {
    let pin = generate_long_registration_lock_pin();
    assert_eq!(pin.len(), GENERATED_REGISTRATION_PIN_DIGITS);
    assert!(pin.chars().all(|c| c.is_ascii_digit()));
}

#[test]
fn image_transforms_keep_expected_dimensions_and_values() {
    let src = GrayImage::from_fn(10, 8, |x, y| Luma([((x + y) as u8) * 10]));
    let same = scale_luma_image(&src, 1.0);
    assert_eq!(same.dimensions(), src.dimensions());

    let scaled = scale_luma_image(&src, 0.5);
    assert_eq!(scaled.dimensions(), (5, 4));

    let resized = resize_luma_to_max_dimension(&src, 6);
    assert_eq!(resized.dimensions(), (6, 5));

    let threshold = threshold_luma_image(
        &GrayImage::from_fn(2, 1, |x, _| if x == 0 { Luma([100]) } else { Luma([200]) }),
        150,
        false,
    );
    assert_eq!(threshold.get_pixel(0, 0)[0], 0);
    assert_eq!(threshold.get_pixel(1, 0)[0], 255);

    let no_resize = resize_luma_to_max_dimension(&src, 20);
    assert_eq!(no_resize.dimensions(), src.dimensions());
}

#[test]
fn qr_decode_detects_valid_signal_uri() {
    let env_ctx = TestEnv::new();
    let path = env_ctx.home_dir.path().join("qr.png");
    let uri = "sgnl://linkdevice?uuid=test";
    write_qr_png(&path, uri);

    let decoded = decode_signal_qr_from_image(&path).expect("decode");
    assert_eq!(decoded, Some(uri.to_string()));
}

#[test]
fn qr_decode_returns_none_for_non_qr_image() {
    let env_ctx = TestEnv::new();
    let path = env_ctx.home_dir.path().join("blank.png");
    write_blank_png(&path, 64, 64);
    let decoded = decode_signal_qr_from_image(&path).expect("decode");
    assert_eq!(decoded, None);
}

#[test]
fn qr_rxing_and_rqrr_helpers_reject_non_signal_qr() {
    let env_ctx = TestEnv::new();
    let path = env_ctx.home_dir.path().join("non-signal-qr.png");
    write_qr_png(&path, "https://example.com");

    let rx = decode_signal_qr_with_rxing(&path).expect("rxing decode");
    assert_eq!(rx, None);

    let base = image::open(&path).expect("open image").to_luma8();
    let rqrr = decode_signal_qr_with_rqrr(&base);
    assert_eq!(rqrr, None);

    let multipass = decode_signal_qr_with_rqrr_multipass(&base);
    assert_eq!(multipass, None);
}

#[test]
fn qr_rqrr_helper_accepts_signal_qr() {
    let env_ctx = TestEnv::new();
    let path = env_ctx.home_dir.path().join("signal-rqrr.png");
    let uri = "sgnl://linkdevice?uuid=rqrr";
    write_qr_png(&path, uri);
    let base = image::open(&path).expect("open image").to_luma8();
    let decoded = decode_signal_qr_with_rqrr(&base);
    assert_eq!(decoded, Some(uri.to_string()));
}

#[test]
fn capture_screen_images_requires_output_paths() {
    let err = capture_screen_images(&[]).expect_err("expected empty output error");
    assert!(err.to_string().contains("no screenshot output path"));
}

#[test]
fn capture_screen_image_success_failure_and_timeout() {
    let env_ctx = TestEnv::new();
    install_mock_screencapture(&env_ctx);
    let src = env_ctx.home_dir.path().join("src.png");
    write_blank_png(&src, 32, 32);
    env_ctx.set_var("MOCK_SCREENSHOT_SOURCE", &src.display().to_string());

    let out = env_ctx.home_dir.path().join("out.png");
    capture_screen_image(&out).expect("capture success");
    assert!(out.exists());

    env_ctx.set_var("MOCK_SCREENCAPTURE_EXIT", "1");
    let err = capture_screen_image(&out).expect_err("expected capture failure");
    assert!(err.to_string().contains("screencapture failed"));
    env::remove_var("MOCK_SCREENCAPTURE_EXIT");

    env_ctx.set_var("MOCK_SCREENCAPTURE_SLEEP", "2");
    let err = capture_screen_image(&out).expect_err("expected timeout");
    assert!(err.to_string().contains("timed out"));
}

#[test]
fn detect_display_count_uses_system_profiler_output() {
    let env_ctx = TestEnv::new();
    install_mock_system_profiler(
        &env_ctx,
        "Displays:\n  Resolution: 1920 x 1080\n  Resolution: 2560 x 1440",
    );
    assert_eq!(detect_display_count(), 2);

    install_mock_system_profiler(&env_ctx, "Displays:\n  No resolution lines");
    assert_eq!(detect_display_count(), 1);
}

#[test]
fn capture_screens_for_attempt_uses_multi_display_then_falls_back() {
    let env_ctx = TestEnv::new();
    install_mock_screencapture(&env_ctx);
    let src = env_ctx.home_dir.path().join("src.png");
    write_blank_png(&src, 16, 16);
    env_ctx.set_var("MOCK_SCREENSHOT_SOURCE", &src.display().to_string());

    let paths = capture_screens_for_attempt(env_ctx.home_dir.path(), 1, 2).expect("multi");
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().all(|p| p.exists()));

    env_ctx.set_var("MOCK_SCREENCAPTURE_FAIL_MULTI", "1");
    let fallback = capture_screens_for_attempt(env_ctx.home_dir.path(), 2, 2).expect("fallback");
    assert_eq!(fallback.len(), 1);
    assert!(fallback[0].exists());
}

#[test]
fn command_exists_detects_present_and_missing_commands() {
    let env_ctx = TestEnv::new();
    env_ctx.write_script("mycmd", "#!/bin/sh\nexit 0\n");
    assert!(command_exists("mycmd"));
    assert!(!command_exists("cmd-does-not-exist"));
}

#[test]
fn docker_readiness_and_startup_paths() {
    let env_ctx = TestEnv::new();
    install_mock_docker(&env_ctx);
    install_mock_open(&env_ctx);

    assert!(docker_daemon_is_ready().expect("docker info"));
    ensure_docker_ready().expect("already ready should pass");

    env_ctx.set_var("MOCK_DOCKER_INFO_EXIT", "1");
    env_ctx.set_var("MOCK_OPEN_EXIT", "1");
    let err = ensure_docker_ready().expect_err("expected startup timeout/failure");
    assert!(err
        .to_string()
        .contains("could not be started automatically"));

    env_ctx.set_var("MOCK_OPEN_EXIT", "0");
    env_ctx.set_var("MOCK_DOCKER_INFO_FAILS", "1");
    env_ctx.set_var(
        "MOCK_DOCKER_INFO_COUNTER_FILE",
        &env_ctx
            .log_path("docker-info-counter")
            .display()
            .to_string(),
    );
    env_ctx.set_var("MOCK_DOCKER_INFO_EXIT", "0");
    ensure_docker_ready().expect("startup succeeds after one failure");
}

#[test]
fn ensure_docker_ready_fails_when_docker_missing() {
    let env_ctx = TestEnv::new();
    env_ctx.set_path_minimal();
    let err = ensure_docker_ready().expect_err("docker should be missing");
    assert!(err.to_string().contains("Docker is not installed"));
}

#[test]
fn try_start_docker_uses_open_on_macos() {
    let env_ctx = TestEnv::new();
    install_mock_open(&env_ctx);
    let log = env_ctx.log_path("open.log");
    env_ctx.set_var("MOCK_OPEN_LOG", &log.display().to_string());
    assert!(try_start_docker());
    let content = read_log(&log);
    assert!(content.contains("-a Docker"));
}

#[test]
fn try_start_docker_fallback_path_is_callable() {
    let env_ctx = TestEnv::new();
    env_ctx.set_path_minimal();
    let _ = try_start_docker();
}

#[test]
fn run_signal_cli_and_retries_behave_as_expected() {
    let env_ctx = TestEnv::new();
    install_mock_docker(&env_ctx);
    let log = env_ctx.log_path("docker.log");
    env_ctx.set_var("MOCK_DOCKER_LOG", &log.display().to_string());
    let cfg = env_ctx.cfg();

    let ok = run_signal_cli(&cfg, &["listDevices".to_string()], false).expect("run ok");
    assert!(ok);

    env_ctx.set_var("MOCK_DOCKER_LISTDEVICES_EXIT", "1");
    let soft_fail = run_signal_cli(&cfg, &["listDevices".to_string()], true).expect("soft");
    assert!(!soft_fail);
    let hard_fail =
        run_signal_cli(&cfg, &["listDevices".to_string()], false).expect_err("hard fail expected");
    assert!(hard_fail.to_string().contains("listDevices"));

    env_ctx.set_var("MOCK_DOCKER_REGISTER_EXIT", "1");
    let register_err = run_signal_cli(&cfg, &["register".to_string()], false)
        .expect_err("register hard fail expected");
    assert!(register_err.to_string().contains("register"));
    env_ctx.set_var("MOCK_DOCKER_REGISTER_EXIT", "0");

    env_ctx.set_var("MOCK_DOCKER_REGISTER_FAILS", "2");
    let counter = env_ctx.log_path("register-counter");
    env_ctx.set_var("MOCK_DOCKER_COUNTER_FILE", &counter.display().to_string());
    run_signal_cli_with_retries(
        &cfg,
        &[
            "register".to_string(),
            "--captcha".to_string(),
            "signalcaptcha://ok".to_string(),
        ],
        3,
        0,
        "registration",
    )
    .expect("retry succeeds");

    let count = fs::read_to_string(counter)
        .expect("counter")
        .trim()
        .parse::<u32>()
        .expect("parse counter");
    assert_eq!(count, 3);

    let zero = run_signal_cli_with_retries(&cfg, &["register".to_string()], 0, 0, "registration")
        .expect_err("attempts=0 should fail");
    assert!(zero.to_string().contains("attempts must be > 0"));
}

#[test]
fn run_signal_cli_retry_failure_returns_hint() {
    let env_ctx = TestEnv::new();
    install_mock_docker(&env_ctx);
    env_ctx.set_var("MOCK_DOCKER_REGISTER_EXIT", "1");
    let cfg = env_ctx.cfg();

    let err = run_signal_cli_with_retries(&cfg, &["register".to_string()], 2, 0, "registration")
        .expect_err("retry failure expected");
    assert!(err.to_string().contains("failed after 2 attempts"));
    assert!(err.to_string().contains("number/operator"));
}

#[test]
fn docker_ready_timeout_path_is_reported() {
    let env_ctx = TestEnv::new();
    install_mock_docker(&env_ctx);
    install_mock_open(&env_ctx);
    env_ctx.set_var("MOCK_DOCKER_INFO_EXIT", "1");
    env_ctx.set_var("MOCK_OPEN_EXIT", "0");

    let err = ensure_docker_ready().expect_err("expected docker startup timeout");
    assert!(err.to_string().contains("timed out"));
}

#[test]
fn run_signal_cli_output_and_error_classification_paths() {
    let env_ctx = TestEnv::new();
    install_mock_docker(&env_ctx);
    let cfg = env_ctx.cfg();

    env_ctx.set_var("MOCK_DOCKER_STDOUT", "{\"devices\":[{\"id\":2}]}");
    env_ctx.set_var("MOCK_DOCKER_STDERR", "INFO list");
    let ok = run_signal_cli(&cfg, &["listDevices".to_string()], false).expect("list ok");
    assert!(ok);

    env_ctx.set_var("MOCK_DOCKER_STDOUT", "not json");
    env::remove_var("MOCK_DOCKER_STDERR");
    let ok = run_signal_cli(&cfg, &["verify".to_string(), "123456".to_string()], false)
        .expect("verify ok");
    assert!(ok);

    env_ctx.set_var("MOCK_DOCKER_STDOUT", "null");
    let ok = run_signal_cli(&cfg, &["verify".to_string(), "123456".to_string()], false)
        .expect("verify ok null");
    assert!(ok);

    env_ctx.set_var("MOCK_DOCKER_STDOUT", "{}");
    let ok = run_signal_cli(&cfg, &["verify".to_string(), "123456".to_string()], false)
        .expect("verify ok empty obj");
    assert!(ok);

    env_ctx.set_var("MOCK_DOCKER_STDOUT", "{\"ok\":true}");
    let ok = run_signal_cli(&cfg, &["verify".to_string(), "123456".to_string()], false)
        .expect("verify ok obj");
    assert!(ok);

    env_ctx.set_var("MOCK_DOCKER_REGISTER_EXIT", "1");
    env_ctx.set_var(
        "MOCK_DOCKER_STDERR",
        "StatusCode: 502 (ExternalServiceFailureException)",
    );
    let err = run_signal_cli(&cfg, &["register".to_string()], false)
        .expect_err("register should be rate-limited");
    assert!(err.to_string().contains("rate limited"));

    env_ctx.set_var("MOCK_DOCKER_STDERR", "register failed");
    let err = run_signal_cli(&cfg, &["register".to_string()], false)
        .expect_err("register should be hard-fail");
    assert!(err.to_string().contains("register"));
    env_ctx.set_var("MOCK_DOCKER_REGISTER_EXIT", "0");

    env_ctx.set_var("MOCK_DOCKER_LISTDEVICES_EXIT", "1");
    env_ctx.set_var("MOCK_DOCKER_STDERR", "StatusCode: 429");
    let err = run_signal_cli(&cfg, &["listDevices".to_string()], false)
        .expect_err("listDevices should be rate-limited");
    assert!(err.to_string().contains("rate limited"));

    env_ctx.set_var("MOCK_DOCKER_STDERR", "plain failure");
    let err = run_signal_cli(&cfg, &["listDevices".to_string()], false)
        .expect_err("listDevices should be hard-fail");
    assert!(err.to_string().contains("listDevices"));
    env_ctx.set_var("MOCK_DOCKER_LISTDEVICES_EXIT", "0");

    env_ctx.set_var("MOCK_DOCKER_RUN_EXIT", "1");
    let err = run_signal_cli(&cfg, &[], false).expect_err("unknown command should fail");
    assert!(err.to_string().contains("unknown"));
}

#[test]
fn registration_and_device_commands_emit_expected_subcommands() {
    let env_ctx = TestEnv::new();
    install_mock_docker(&env_ctx);
    let log = env_ctx.log_path("docker.log");
    env_ctx.set_var("MOCK_DOCKER_LOG", &log.display().to_string());
    let cfg = env_ctx.cfg();

    register_with_mode(&cfg, "signalcaptcha://token", false).expect("register sms");
    register_with_mode(&cfg, "signalcaptcha://token", true).expect("register voice");
    verify_code(&cfg, "123456", Some("4321")).expect("verify with pin");
    verify_code(&cfg, "123456", None).expect("verify without pin");
    set_registration_lock_pin(&cfg, "12345678901234567890").expect("set pin");
    list_devices(&cfg).expect("list devices");

    let log_content = read_log(&log);
    assert!(log_content.contains("register"));
    assert!(log_content.contains("--voice"));
    assert!(log_content.contains("verify \"$SIGNAL_VERIFY_CODE\" --pin \"$SIGNAL_PIN\""));
    assert!(log_content.contains("verify 123456"));
    assert!(log_content.contains("setPin \"$SIGNAL_PIN\""));
    assert!(!log_content.contains("12345678901234567890"));
    assert!(!log_content.contains("--pin 4321"));
    assert!(log_content.contains("listDevices"));
}

#[test]
fn register_landline_runs_sms_then_voice() {
    let env_ctx = TestEnv::new();
    install_mock_docker(&env_ctx);
    let log = env_ctx.log_path("docker.log");
    env_ctx.set_var("MOCK_DOCKER_LOG", &log.display().to_string());
    let cfg = env_ctx.cfg();

    register_landline(&cfg, "signalcaptcha://token").expect("landline flow");
    let content = read_log(&log);
    let register_count = content.matches("register").count();
    assert!(register_count >= 2);
    assert!(content.contains("--voice"));

    env_ctx.set_var("MOCK_DOCKER_REGISTER_FAILS", "1");
    env_ctx.set_var(
        "MOCK_DOCKER_COUNTER_FILE",
        &env_ctx
            .log_path("landline-register-counter")
            .display()
            .to_string(),
    );
    register_landline(&cfg, "signalcaptcha://token").expect("landline flow with sms failure");
}

#[test]
fn link_from_uri_and_image_paths_work() {
    let env_ctx = TestEnv::new();
    install_mock_docker(&env_ctx);
    let log = env_ctx.log_path("docker.log");
    env_ctx.set_var("MOCK_DOCKER_LOG", &log.display().to_string());
    let cfg = env_ctx.cfg();

    let invalid =
        link_desktop_from_uri(&cfg, "https://example.com").expect_err("invalid URI should fail");
    assert!(invalid.to_string().contains("invalid URI"));

    let uri = "sgnl://linkdevice?uuid=test";
    link_desktop_from_uri(&cfg, uri).expect("link by URI");
    let content = read_log(&log);
    assert!(content.contains("addDevice --uri"));
    assert!(content.contains("receive --timeout"));
    assert!(content.contains("sendContacts"));
    assert!(content.contains("listDevices"));

    let missing = link_desktop_from_image(&cfg, Path::new("/tmp/no-such-file.png"))
        .expect_err("missing image should fail");
    assert!(missing.to_string().contains("screenshot file not found"));

    let img = env_ctx.home_dir.path().join("qr-link.png");
    write_qr_png(&img, uri);
    link_desktop_from_image(&cfg, &img).expect("link by image");
}

#[test]
fn live_link_scan_and_scan_loop_behaviors() {
    {
        let env_ctx = TestEnv::new();
        install_mock_docker(&env_ctx);
        install_mock_screencapture(&env_ctx);
        install_mock_pgrep(&env_ctx);
        let cfg = env_ctx.cfg();

        let qr = env_ctx.home_dir.path().join("qr.png");
        let uri = "sgnl://linkdevice?uuid=live";
        write_qr_png(&qr, uri);
        env_ctx.set_var("MOCK_SCREENSHOT_SOURCE", &qr.display().to_string());
        env_ctx.set_var("MOCK_PGREP_EXIT", "0");

        let scanned = scan_screen_for_signal_uri(0, 1).expect("scan success");
        assert_eq!(scanned, uri);

        link_desktop_live(&cfg, 1, 1).expect("live link");
        let invalid = link_desktop_live(&cfg, 0, 1).expect_err("invalid params");
        assert!(invalid.to_string().contains("must be > 0"));

        let blank = env_ctx.home_dir.path().join("blank.png");
        write_blank_png(&blank, 64, 64);
        env_ctx.set_var("MOCK_SCREENSHOT_SOURCE", &blank.display().to_string());
        let no_qr = scan_screen_for_signal_uri(0, 1).expect_err("no qr expected");
        assert!(no_qr
            .to_string()
            .contains("no valid Signal Desktop QR found"));
    }

    {
        let no_screencapture_env = TestEnv::new();
        install_mock_docker(&no_screencapture_env);
        install_mock_pgrep(&no_screencapture_env);
        no_screencapture_env.set_path_minimal();
        let err = link_desktop_live(&no_screencapture_env.cfg(), 1, 1)
            .expect_err("missing screencapture should fail");
        assert!(err.to_string().contains("screencapture is required"));
    }
}

#[test]
fn live_link_succeeds_even_when_desktop_auto_launch_fails() {
    let env_ctx = TestEnv::new();
    install_mock_docker(&env_ctx);
    install_mock_screencapture(&env_ctx);
    env_ctx.set_path_minimal();
    let cfg = env_ctx.cfg();

    let qr = env_ctx.home_dir.path().join("qr-manual.png");
    write_qr_png(&qr, "sgnl://linkdevice?uuid=manual-open");
    env_ctx.set_var("MOCK_SCREENSHOT_SOURCE", &qr.display().to_string());

    link_desktop_live(&cfg, 1, 1).expect("link should succeed without auto-launch");
}

#[test]
fn process_detection_and_signal_launch_paths() {
    {
        let env_ctx = TestEnv::new();
        install_mock_pgrep(&env_ctx);
        install_mock_open(&env_ctx);
        install_mock_signal_launchers(&env_ctx);

        env_ctx.set_var("MOCK_PGREP_MATCH", "Signal");
        assert!(process_running_exact("Signal"));
        assert!(process_running_fuzzy("Signal"));
        assert!(is_signal_desktop_running());

        env_ctx.set_var("MOCK_PGREP_MATCH", "");
        env_ctx.set_var("MOCK_PGREP_EXIT", "1");
        assert!(open_signal_desktop());

        let counter = env_ctx.log_path("pgrep-counter");
        env_ctx.set_var("MOCK_PGREP_MATCH", "Signal");
        env_ctx.set_var("MOCK_PGREP_FAILS", "1");
        env_ctx.set_var("MOCK_PGREP_COUNTER_FILE", &counter.display().to_string());
        assert!(open_signal_desktop());
    }

    {
        let env_ctx = TestEnv::new();
        install_mock_pgrep(&env_ctx);
        env_ctx.set_path_minimal();
        env_ctx.set_var("MOCK_PGREP_EXIT", "1");
        let no_launch = open_signal_desktop();
        assert!(!no_launch);
        assert!(!process_running_exact("Signal"));
        assert!(!process_running_fuzzy("Signal"));
    }
}

#[test]
fn process_detection_without_mocks_uses_sysinfo_snapshot() {
    let env_ctx = TestEnv::new();
    env_ctx.clear_mock_env();
    assert!(!process_running_exact("definitely-not-a-real-process-xyz"));
    assert!(!process_running_fuzzy("definitely-not-a-real-process-xyz"));
}

#[test]
fn process_mock_fails_without_counter_file_still_uses_match_value() {
    let env_ctx = TestEnv::new();
    env_ctx.set_var("MOCK_PGREP_FAILS", "1");
    env_ctx.set_var("MOCK_PGREP_MATCH", "Signal");
    assert!(process_running_exact("Signal"));
}

#[test]
fn run_post_link_sync_covers_failure_paths() {
    let env_ctx = TestEnv::new();
    install_mock_docker(&env_ctx);
    let cfg = env_ctx.cfg();
    env_ctx.set_var("MOCK_DOCKER_RECEIVE_EXIT", "1");
    env_ctx.set_var("MOCK_DOCKER_SENDCONTACTS_EXIT", "1");
    run_post_link_sync(&cfg);
}

#[test]
fn run_post_link_sync_covers_error_paths() {
    let env_ctx = TestEnv::new();
    env_ctx.set_path_minimal();
    run_post_link_sync(&env_ctx.cfg());
}

#[test]
fn scan_loop_sleep_branch_is_exercised() {
    let env_ctx = TestEnv::new();
    install_mock_screencapture(&env_ctx);
    let blank = env_ctx.home_dir.path().join("blank2.png");
    write_blank_png(&blank, 64, 64);
    env_ctx.set_var("MOCK_SCREENSHOT_SOURCE", &blank.display().to_string());
    let _ = scan_screen_for_signal_uri(1, 2);
}

#[test]
fn detect_display_count_handles_missing_and_failed_profiler() {
    {
        let env_ctx = TestEnv::new();
        env_ctx.set_path_minimal();
        assert_eq!(detect_display_count(), 1);
    }

    {
        let env_ctx = TestEnv::new();
        install_mock_system_profiler(&env_ctx, "Resolution: 1920 x 1080");
        env_ctx.set_var("MOCK_SP_FAIL", "1");
        assert_eq!(detect_display_count(), 1);
    }
}

#[test]
fn test_env_drop_handles_missing_original_vars() {
    let mut env_ctx = TestEnv::new();
    env_ctx.old_path = None;
    env_ctx.old_home = None;
    drop(env_ctx);
}

#[test]
fn link_desktop_interactive_test_stub_is_callable() {
    let env_ctx = TestEnv::new();
    let theme = ColorfulTheme::default();
    link_desktop_interactive(&env_ctx.cfg(), &theme, 1, 1).expect("interactive stub");
}

#[test]
fn browser_and_screen_settings_open_commands_are_issued() {
    let env_ctx = TestEnv::new();
    install_mock_open(&env_ctx);
    let log = env_ctx.log_path("open.log");
    env_ctx.set_var("MOCK_OPEN_LOG", &log.display().to_string());

    open_url_in_default_browser("https://example.com");
    open_screen_recording_settings();

    let content = read_log(&log);
    assert!(content.contains("https://example.com"));
    assert!(content.contains("Privacy_ScreenCapture"));
}

#[test]
fn browser_open_fallback_paths_are_callable_without_open_binary() {
    let env_ctx = TestEnv::new();
    env_ctx.set_path_minimal();
    open_url_in_default_browser("https://example.com");
    open_screen_recording_settings();
}

#[test]
fn captcha_token_extraction_handles_success_and_failure() {
    let token = docker::extract_signal_captcha_token_from_output(
        b"log line\nsignalcaptcha://token-value\n",
    )
    .expect("expected token");
    assert_eq!(token, "signalcaptcha://token-value");

    let err = docker::extract_signal_captcha_token_from_output(b"log line\nno token here")
        .expect_err("missing token should fail");
    assert!(err.to_string().contains("did not return a token"));
}

#[test]
fn test_cfg_stubs_return_expected_values() {
    let theme = ColorfulTheme::default();
    assert_eq!(
        get_captcha_token_for_wizard(&theme).expect("stub token"),
        "signalcaptcha://test-token"
    );
    assert_eq!(
        capture_captcha_token_subprocess().expect("subprocess stub"),
        "signalcaptcha://test-subprocess-token"
    );
    assert_eq!(
        capture_captcha_token(true).expect("webview stub"),
        "signalcaptcha://test-webview-token"
    );

    let selected =
        ensure_account_interactive(Some("+12345".to_string()), &theme).expect("account stub");
    assert_eq!(selected, "+12345");
    let generated = ensure_account_interactive(None, &theme).expect("default account");
    assert!(generated.starts_with('+'));
}
