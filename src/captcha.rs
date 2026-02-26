use anyhow::Result;
#[cfg(not(test))]
use anyhow::{anyhow, bail, Context};
use dialoguer::theme::ColorfulTheme;
#[cfg(not(test))]
use dialoguer::Input;
#[cfg(not(test))]
use std::process::{Command, Stdio};

#[cfg(not(test))]
use crate::docker::extract_signal_captcha_token_from_output;
#[cfg(not(test))]
use crate::system::open_url_in_default_browser;

#[cfg(not(test))]
pub fn get_captcha_token_for_wizard(theme: &ColorfulTheme) -> Result<String> {
    match capture_captcha_token_subprocess() {
        Ok(token) => Ok(token),
        Err(err) => {
            eprintln!("Embedded captcha capture failed: {err}");
            eprintln!("Falling back to browser + manual token paste.");
            open_url_in_default_browser(crate::CAPTCHA_URL);
            let pasted: String = Input::with_theme(theme)
                .with_prompt("Paste signalcaptcha:// token")
                .interact_text()?;
            if pasted.starts_with("signalcaptcha://") {
                Ok(pasted)
            } else {
                bail!("invalid captcha token format")
            }
        }
    }
}

#[cfg(test)]
pub fn get_captcha_token_for_wizard(_theme: &ColorfulTheme) -> Result<String> {
    Ok("signalcaptcha://test-token".to_string())
}

#[cfg(not(test))]
pub fn capture_captcha_token_subprocess() -> Result<String> {
    let exe = std::env::current_exe().context("failed to resolve current executable path")?;
    let output = Command::new(exe)
        .arg("captcha-token")
        .arg("--quiet")
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .context("failed to spawn captcha-token subprocess")?;

    if !output.status.success() {
        bail!(
            "captcha-token subprocess failed with status {}",
            output
                .status
                .code()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
    }

    extract_signal_captcha_token_from_output(&output.stdout)
}

#[cfg(test)]
pub fn capture_captcha_token_subprocess() -> Result<String> {
    Ok("signalcaptcha://test-subprocess-token".to_string())
}

#[cfg(not(test))]
pub fn capture_captcha_token(quiet: bool) -> Result<String> {
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::platform::run_return::EventLoopExtRunReturn;
    use tao::window::WindowBuilder;
    use wry::WebViewBuilder;

    let mut event_loop = EventLoopBuilder::<String>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("Signal Captcha")
        .build(&event_loop)
        .context("failed to create captcha window")?;

    let webview = WebViewBuilder::new(&window)
        .with_url(crate::CAPTCHA_URL)
        .with_navigation_handler(move |url: String| {
            if url.starts_with("signalcaptcha://") {
                let _ = proxy.send_event(url);
                return false;
            }
            true
        })
        .build()
        .context("failed to build captcha webview")?;

    if !quiet {
        eprintln!("Solve the captcha in the opened window.");
        eprintln!("The window closes automatically when signalcaptcha:// is captured.");
    }

    let mut captured: Option<String> = None;
    event_loop.run_return(|event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(token) => {
                captured = Some(token);
                window.set_visible(false);
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });

    drop(webview);
    drop(window);
    drop(event_loop);

    captured.ok_or_else(|| anyhow!("captcha window was closed before token capture"))
}

#[cfg(test)]
pub fn capture_captcha_token(_quiet: bool) -> Result<String> {
    Ok("signalcaptcha://test-webview-token".to_string())
}
