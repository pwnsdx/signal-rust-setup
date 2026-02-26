use anyhow::{bail, Result};
use dialoguer::theme::ColorfulTheme;
#[cfg(not(test))]
use dialoguer::Input;
use dirs::home_dir;
use std::path::PathBuf;

use crate::cli::Cli;

#[derive(Debug, Clone)]
pub struct Config {
    pub account: String,
    pub data_dir: PathBuf,
    pub image: String,
}

pub fn config_from_cli(cli: &Cli, require_account: bool) -> Result<Config> {
    let data_dir = cli.data_dir.clone().unwrap_or_else(default_data_dir);

    let account = match &cli.account {
        Some(v) => {
            validate_account(v)?;
            v.clone()
        }
        None if require_account => bail!("--account is required for this command"),
        None => String::new(),
    };

    Ok(Config {
        account,
        data_dir,
        image: cli.image.clone(),
    })
}

pub fn default_data_dir() -> PathBuf {
    match home_dir() {
        Some(mut p) => {
            p.push("signal-cli-data");
            p
        }
        None => PathBuf::from("signal-cli-data"),
    }
}

pub fn validate_account(account: &str) -> Result<()> {
    if !account.starts_with('+') {
        bail!("account must start with '+' in international format")
    }
    Ok(())
}

#[cfg(not(test))]
pub fn ensure_account_interactive(
    existing: Option<String>,
    theme: &ColorfulTheme,
) -> Result<String> {
    if let Some(value) = existing {
        validate_account(&value)?;
        return Ok(value);
    }

    loop {
        let value: String = Input::with_theme(theme)
            .with_prompt("Account number (international format, e.g. +33612345678)")
            .interact_text()?;
        if validate_account(&value).is_ok() {
            return Ok(value);
        }
        println!("Invalid format. Account must start with '+'.");
    }
}

#[cfg(test)]
pub fn ensure_account_interactive(
    existing: Option<String>,
    _theme: &ColorfulTheme,
) -> Result<String> {
    match existing {
        Some(value) => {
            validate_account(&value)?;
            Ok(value)
        }
        None => Ok("+10000000000".to_string()),
    }
}
