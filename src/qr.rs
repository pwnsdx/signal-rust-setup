use anyhow::{bail, Context, Result};
use image::imageops::FilterType;
use image::{GrayImage, Luma};
use indicatif::{ProgressBar, ProgressStyle};
use rqrr::PreparedImage;
use rxing::{helpers as rxing_helpers, BarcodeFormat};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;
use xcap::Monitor;

use crate::system::command_exists;

pub fn scan_screen_for_signal_uri(interval: u64, attempts: u32) -> Result<String> {
    let temp_dir = tempdir().context("failed to create temporary directory")?;
    let display_count = detect_display_count();
    let pb = ProgressBar::new(attempts as u64);
    let style = ProgressStyle::with_template(
        "{spinner:.green} [{elapsed_precise}] [{bar:30.cyan/blue}] {pos}/{len} {msg}",
    )
    .unwrap_or_else(|_| ProgressStyle::default_bar())
    .progress_chars("=> ");
    pb.set_style(style);
    pb.enable_steady_tick(Duration::from_millis(120));
    pb.set_message(format!(
        "Preparing first screen capture ({display_count} display(s))..."
    ));

    for attempt in 1..=attempts {
        pb.set_message(format!("Attempt {attempt}/{attempts}: capturing screen..."));
        let screenshot_paths =
            capture_screens_for_attempt(temp_dir.path(), attempt, display_count)?;

        pb.set_message(format!("Attempt {attempt}/{attempts}: decoding QR..."));
        for screenshot_path in screenshot_paths {
            if let Some(uri) = decode_signal_qr_from_image(&screenshot_path)? {
                pb.finish_with_message(format!("QR detected on attempt {attempt}."));
                return Ok(uri);
            }
        }

        pb.inc(1);
        pb.set_message(format!(
            "Attempt {attempt}/{attempts}: no valid Signal QR yet."
        ));
        if attempt < attempts {
            thread::sleep(Duration::from_secs(interval));
        }
    }

    pb.abandon_with_message("No valid QR found before timeout.");
    bail!("no valid Signal Desktop QR found after {attempts} attempts")
}

#[cfg(not(test))]
pub fn decode_signal_qr_from_image(path: &Path) -> Result<Option<String>> {
    let base = image::open(path)
        .with_context(|| format!("failed to open image {}", path.display()))?
        .to_luma8();

    let fast = resize_luma_to_max_dimension(&base, crate::QR_FAST_MAX_DIMENSION);
    if let Some(uri) = decode_signal_qr_with_rxing_luma(&fast) {
        return Ok(Some(uri));
    }
    if let Some(uri) = decode_signal_qr_with_rqrr_fastpass(&fast) {
        return Ok(Some(uri));
    }

    let pixel_count = (base.width() as u64).saturating_mul(base.height() as u64);

    if pixel_count <= crate::QR_RXING_MAX_PIXELS {
        if let Some(uri) = decode_signal_qr_with_rxing_luma(&base) {
            return Ok(Some(uri));
        }
        if let Some(uri) = decode_signal_qr_with_rqrr_multipass(&base) {
            return Ok(Some(uri));
        }
    } else {
        let upscaled_fast = scale_luma_image(&fast, 1.15);
        if let Some(uri) = decode_signal_qr_with_rxing_luma(&upscaled_fast) {
            return Ok(Some(uri));
        }
        if let Some(uri) = decode_signal_qr_with_rqrr_fastpass(&upscaled_fast) {
            return Ok(Some(uri));
        }
    }

    Ok(None)
}

#[cfg(test)]
pub fn decode_signal_qr_from_image(path: &Path) -> Result<Option<String>> {
    if let Some(uri) = decode_signal_qr_with_rxing(path)? {
        return Ok(Some(uri));
    }

    let base = image::open(path)
        .with_context(|| format!("failed to open image {}", path.display()))?
        .to_luma8();
    Ok(decode_signal_qr_with_rqrr(&base))
}

#[cfg(not(test))]
pub fn decode_signal_qr_with_rqrr_multipass(image: &GrayImage) -> Option<String> {
    let scales = [1.0_f32, 0.85, 1.2];
    for scale in scales {
        let candidate = scale_luma_image(image, scale);

        if let Some(uri) = decode_signal_qr_with_rqrr(&candidate) {
            return Some(uri);
        }

        for threshold in [110_u8, 140_u8, 170_u8] {
            let binary = threshold_luma_image(&candidate, threshold, false);
            if let Some(uri) = decode_signal_qr_with_rqrr(&binary) {
                return Some(uri);
            }

            let inverted = threshold_luma_image(&candidate, threshold, true);
            if let Some(uri) = decode_signal_qr_with_rqrr(&inverted) {
                return Some(uri);
            }
        }
    }

    None
}

#[cfg(not(test))]
fn decode_signal_qr_with_rqrr_fastpass(image: &GrayImage) -> Option<String> {
    if let Some(uri) = decode_signal_qr_with_rqrr(image) {
        return Some(uri);
    }

    for threshold in [128_u8, 160_u8] {
        let binary = threshold_luma_image(image, threshold, false);
        if let Some(uri) = decode_signal_qr_with_rqrr(&binary) {
            return Some(uri);
        }
    }

    None
}

#[cfg(test)]
pub fn decode_signal_qr_with_rqrr_multipass(image: &GrayImage) -> Option<String> {
    decode_signal_qr_with_rqrr(image)
}

pub fn decode_signal_qr_with_rxing(path: &Path) -> Result<Option<String>> {
    let base = image::open(path)
        .with_context(|| format!("failed to open image {}", path.display()))?
        .to_luma8();
    Ok(decode_signal_qr_with_rxing_luma(&base))
}

fn decode_signal_qr_with_rxing_luma(image: &GrayImage) -> Option<String> {
    let decode_result = rxing_helpers::detect_in_luma(
        image.as_raw().clone(),
        image.width(),
        image.height(),
        Some(BarcodeFormat::QR_CODE),
    );
    let Ok(result) = decode_result else {
        return None;
    };

    let text = result.getText().trim();
    if text.starts_with("sgnl://linkdevice") {
        return Some(text.to_string());
    }

    None
}

pub fn decode_signal_qr_with_rqrr(image: &GrayImage) -> Option<String> {
    let mut prepared = PreparedImage::prepare(image.clone());
    let grids = prepared.detect_grids();

    for grid in grids {
        if let Ok((_meta, content)) = grid.decode() {
            if content.starts_with("sgnl://linkdevice") {
                return Some(content);
            }
        }
    }

    None
}

pub fn scale_luma_image(image: &GrayImage, scale: f32) -> GrayImage {
    if (scale - 1.0).abs() < f32::EPSILON {
        return image.clone();
    }

    let width = ((image.width() as f32) * scale).round().max(1.0) as u32;
    let height = ((image.height() as f32) * scale).round().max(1.0) as u32;
    image::imageops::resize(image, width, height, FilterType::Nearest)
}

pub fn resize_luma_to_max_dimension(image: &GrayImage, max_dimension: u32) -> GrayImage {
    let width = image.width();
    let height = image.height();
    let current_max = width.max(height);

    if current_max <= max_dimension || current_max == 0 {
        return image.clone();
    }

    let scale = (max_dimension as f32) / (current_max as f32);
    scale_luma_image(image, scale)
}

pub fn threshold_luma_image(image: &GrayImage, threshold: u8, invert: bool) -> GrayImage {
    let mut out = GrayImage::new(image.width(), image.height());

    for (x, y, pixel) in image.enumerate_pixels() {
        let source = pixel[0];
        let bit = if source >= threshold { 255 } else { 0 };
        let value = if invert { 255 - bit } else { bit };
        out.put_pixel(x, y, Luma([value]));
    }

    out
}

pub fn capture_screen_image(path: &Path) -> Result<()> {
    capture_screen_images(&[path.to_path_buf()])
}

pub fn capture_screen_images(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        bail!("no screenshot output path provided");
    }

    let mut child = Command::new("screencapture")
        .arg("-x")
        .args(paths)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to run screencapture")?;

    let timeout = Duration::from_secs(crate::SCREEN_CAPTURE_TIMEOUT_SECS);
    let poll_every = Duration::from_millis(100);
    let start = Instant::now();

    loop {
        if let Some(status) = child
            .try_wait()
            .context("failed while waiting for screencapture")?
        {
            if status.success() {
                return Ok(());
            }
            bail!("screencapture failed (check Screen Recording permissions)");
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "screencapture timed out after {}s (check Screen Recording permissions and active desktop session)",
                crate::SCREEN_CAPTURE_TIMEOUT_SECS
            );
        }

        thread::sleep(poll_every);
    }
}

pub fn detect_display_count() -> usize {
    if command_exists("system_profiler") {
        let output = Command::new("system_profiler")
            .arg("SPDisplaysDataType")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut displays = stdout
                    .lines()
                    .filter(|line| line.trim_start().starts_with("Resolution:"))
                    .count();

                if displays == 0 {
                    displays = 1;
                }
                return displays.min(crate::MAX_DETECTED_DISPLAYS);
            }
        }
    }

    if let Ok(monitors) = Monitor::all() {
        let count = monitors.len();
        if count > 0 {
            return count.min(crate::MAX_DETECTED_DISPLAYS);
        }
    }

    1
}

pub fn capture_screens_for_attempt(
    base_dir: &Path,
    attempt: u32,
    display_count: usize,
) -> Result<Vec<PathBuf>> {
    let mut multi_paths = Vec::new();

    if display_count > 1 {
        for display_idx in 1..=display_count {
            multi_paths.push(base_dir.join(format!("screen-{attempt}-display-{display_idx}.png")));
        }

        if capture_screen_images(&multi_paths).is_ok() {
            return Ok(multi_paths);
        }

        if let Ok(paths) = capture_screens_with_xcap(base_dir, attempt) {
            if !paths.is_empty() {
                return Ok(paths);
            }
        }
    } else {
        #[cfg(not(target_os = "macos"))]
        {
            if let Ok(paths) = capture_screens_with_xcap(base_dir, attempt) {
                if !paths.is_empty() {
                    return Ok(paths);
                }
            }
        }
    }

    let single_path = base_dir.join(format!("screen-{attempt}.png"));
    capture_screen_image(&single_path)?;
    Ok(vec![single_path])
}

fn capture_screens_with_xcap(base_dir: &Path, attempt: u32) -> Result<Vec<PathBuf>> {
    let monitors = Monitor::all().context("failed to enumerate displays with xcap")?;
    if monitors.is_empty() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for (idx, monitor) in monitors.into_iter().enumerate() {
        let image = monitor
            .capture_image()
            .context("failed to capture display with xcap")?;
        let path = base_dir.join(format!("screen-{attempt}-display-{}.png", idx + 1));
        image
            .save(&path)
            .with_context(|| format!("failed to save screenshot {}", path.display()))?;
        paths.push(path);
    }

    Ok(paths)
}
