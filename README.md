# signal-desktop-only

`signal-desktop-only` is an interactive Rust CLI that automates Signal account setup with `signal-cli` in Docker, then links Signal Desktop via QR detection.

## Purpose

This project exists to enable standalone Signal usage from a desktop without requiring a mobile phone app.
Signal does not currently provide an official desktop-only onboarding path, so this tool automates a practical workaround for that use case.

This project is not affiliated with Signal.

## Features

- Embedded captcha flow with automatic `signalcaptcha://` capture
- Registration modes:
  - SMS
  - Voice
  - Landline/SIP flow (SMS attempt, wait, then voice)
- Verification flow with optional existing registration lock PIN
- Mandatory generation and configuration of a long registration lock PIN
- Automatic Signal Desktop launch + live QR scanning
- Multi-display QR scanning support on macOS
- Post-link sync stabilization (`receive` + `sendContacts`) to reduce initial sync stalls
- Docker detection and daemon startup attempt

## Platform Support

- Full wizard (`cargo run -- wizard`) is optimized for macOS.
- Live QR scanning depends on macOS `screencapture`.
- Core `signal-cli` Docker commands (`register`, `verify`, `list-devices`, etc.) are portable as long as Docker is available.

## Requirements

- Docker Desktop / Docker Engine
- Rust toolchain (`cargo`, stable)
- Signal Desktop installed
- macOS Screen Recording permission for your terminal app (for live QR scan)

## Number And Security Requirements

- A dedicated phone number is required for this workflow.
- If you do not have a second number, temporary numbers from services such as [5Sim.net](https://5sim.net/) can be used (often a few cents, depending on country/operator).
- Always comply with local laws and provider/platform terms when using temporary numbers.
- Registration Lock PIN is mandatory in this tool because it helps protect against unauthorized re-registration if someone gains control of your number (for example SIM swap or operator-side abuse).
- Safety Number verification is strongly recommended:
  - compare your desktop Safety Number with your own trusted device (if you have one),
  - and verify Safety Numbers with your contacts, especially before sensitive conversations.

## Quick Start

```bash
git clone <your-repo-url>
cd signal-desktop-only
cargo run -- wizard
```

The wizard performs:

1. Account input (`+countrycode...`)
2. Captcha capture
3. Registration (SMS mode)
4. Verification
5. Registration lock PIN generation + `setPin`
6. Desktop launch + QR scan + link
7. Post-link sync finishing steps

## CLI Commands

Run full flow:

```bash
cargo run -- wizard
```

Get captcha token only:

```bash
cargo run -- captcha-token
```

Register:

```bash
cargo run -- register --account +33612345678 --token "signalcaptcha://..."
```

Voice registration:

```bash
cargo run -- register --account +33612345678 --token "signalcaptcha://..." --voice
```

Landline flow:

```bash
cargo run -- register --account +33612345678 --token "signalcaptcha://..." --landline
```

Verify:

```bash
cargo run -- verify --account +33612345678 123456
```

Verify with existing registration lock PIN:

```bash
cargo run -- verify --account +33612345678 123456 --pin 1234
```

Live desktop linking:

```bash
cargo run -- link-desktop-live --account +33612345678 --interval 2 --attempts 90
```

List linked devices:

```bash
cargo run -- list-devices --account +33612345678
```

## Data Storage

- Default data path: `~/signal-cli-data`
- You can override it with `--data-dir`.
- Docker volume mapping is handled by the tool.

Example:

```bash
cargo run -- wizard --data-dir /tmp/signal-data
```

## Troubleshooting

### `StatusCode: 502 (ExternalServiceFailureException)` on register

This is often temporary. If persistent:

- Retry with a fresh captcha token
- Try another IP/network (for example mobile hotspot)
- Try another operator/number (some routes can be blocked/rate-limited)

### Live scan appears stuck / QR not detected

- Ensure Signal Desktop pairing QR is visible and not obscured.
- On macOS, grant Screen Recording permission to your terminal app.
- On multi-display setups, place the QR clearly on one screen and keep it stable.

### Signal Desktop stuck on "Syncing contacts and groups"

Run a manual receive pass on the primary data and restart Desktop:

```bash
docker run --rm -i \
  --volume "$HOME/signal-cli-data:/var/lib/signal-cli" \
  --tmpfs /tmp:exec \
  registry.gitlab.com/packaging/signal-cli/signal-cli-native:latest \
  -a +YOUR_NUMBER \
  receive --timeout 30 --max-messages 200
```

### Linked desktop gets unlinked after inactivity (about 30 days)

Warning: linked Signal Desktop devices can be automatically unlinked after long inactivity (Signal currently documents `>30 days` without connecting to the Signal service, and this may change over time).

For this desktop-first setup, this is important because your Desktop app is a linked device.

How to avoid it (practical):

- Open Signal Desktop while online at least every 1-2 weeks so it can connect to Signal.
- Keep Signal Desktop updated.
- Verify link health periodically:

```bash
cargo run -- list-devices --account +YOUR_NUMBER
```

Check that your desktop device is present and that `Last seen` is recent.

If it already happened:

- Relink the desktop:

```bash
cargo run -- link-desktop-live --account +YOUR_NUMBER
```

- If needed, run the full flow again:

```bash
cargo run -- wizard
```

Reference: https://support.signal.org/hc/en-us/articles/360007320551-Linked-Devices

## Development

Format and checks:

```bash
cargo fmt
cargo check
cargo clippy --lib --bins -- -D warnings -D clippy::dbg_macro -D clippy::todo -D clippy::unwrap_used -D clippy::expect_used -D clippy::unimplemented -D clippy::panic
cargo test
```

Coverage (line coverage gate at 95%):

```bash
cargo llvm-cov --summary-only --lib --ignore-filename-regex '/target/llvm-cov-target/debug/build/.*/out/' --fail-under-lines 95
```

CI is configured in [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

MIT, see [`LICENSE`](LICENSE).
