use thiserror::Error;

#[derive(Debug, Error)]
pub enum SignalSetupError {
    #[error("Docker is not installed. Install Docker Desktop/Engine and retry.")]
    DockerNotInstalled,

    #[error("Docker is installed but could not be started automatically. Start Docker manually and retry.")]
    DockerStartFailed,

    #[error("Docker start timed out after {seconds} seconds. Open Docker Desktop and retry.")]
    DockerStartTimeout { seconds: u64 },

    #[error("signal-cli 'register' command failed")]
    RegisterFailed,

    #[error("signal-cli '{command}' command failed")]
    SignalCliCommandFailed { command: String },

    #[error("signal-cli rate limited request (StatusCode 429/502). Try again with a fresh captcha and network/IP change if needed.")]
    SignalCliRateLimited,
}
