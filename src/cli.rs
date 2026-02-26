use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Interactive Signal Docker + Desktop linker (Rust)"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(long, global = true)]
    pub account: Option<String>,

    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,

    #[arg(long, global = true, default_value = crate::DEFAULT_IMAGE)]
    pub image: String,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Full interactive flow: captcha -> register -> verify -> link desktop
    Wizard,

    /// Open captcha in a WebView and print captured signalcaptcha:// token
    CaptchaToken {
        #[arg(long, default_value_t = false)]
        quiet: bool,
    },

    /// Register account with a captcha token
    Register {
        #[arg(long)]
        token: String,

        #[arg(long, default_value_t = false)]
        voice: bool,

        #[arg(long, default_value_t = false)]
        landline: bool,
    },

    /// Verify registration code
    Verify {
        code: String,

        #[arg(long)]
        pin: Option<String>,
    },

    /// Open Signal Desktop, scan full-screen screenshots until QR is found, then link device
    LinkDesktopLive {
        #[arg(long, default_value_t = crate::DEFAULT_SCAN_INTERVAL)]
        interval: u64,

        #[arg(long, default_value_t = crate::DEFAULT_SCAN_ATTEMPTS)]
        attempts: u32,
    },

    /// List linked devices
    ListDevices,
}
