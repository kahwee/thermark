//! Advisory stderr output: foot-gun warnings and post-failure hints.
//!
//! Nothing here changes behaviour or exit codes — it only explains.

use anyhow::{Result, bail};
use std::path::Path;

/// Resolve the Wi‑Fi password: CLI flag, else `THERMARK_WIFI_PASSWORD`.
pub fn resolve_wifi_password(flag: String) -> Result<String> {
    if !flag.is_empty() {
        eprintln!(
            "tip: password is on the command line (shell history). \
             Prefer:  THERMARK_WIFI_PASSWORD='…' thermark wifi --ssid \"…\""
        );
        return Ok(flag);
    }
    match std::env::var("THERMARK_WIFI_PASSWORD") {
        Ok(p) if !p.trim().is_empty() => Ok(p),
        _ => bail!(
            "Wi‑Fi password required.\n  \
             THERMARK_WIFI_PASSWORD='…' thermark wifi --ssid \"Network\"\n  \
             or: thermark wifi --ssid \"Network\" --password '…'\n  \
             open network: --security nopass"
        ),
    }
}

/// Paths under `fixtures/` are public product demos — block secret Wi‑Fi saves.
pub fn guard_wifi_save_path(path: &Path) -> Result<()> {
    if path.components().any(|c| c.as_os_str() == "fixtures") {
        bail!(
            "refusing to save a Wi‑Fi sticker under fixtures/ \
             (that path is committed product demos — real credentials must not land there).\n  \
             Use:  --save local/prints/home-wifi.png\n  \
             or omit --save (writes under /tmp). See local/README.md"
        );
    }
    Ok(())
}

/// File extensions that usually hold photographs rather than line art.
const PHOTO_EXTENSIONS: &[&str] = &["jpg", "jpeg", "webp", "heic", "tif", "tiff"];

/// Warnings for common print foot-guns (learned from real use).
pub fn warn_print_limits(image: &Path, label: &Option<String>, no_fill: bool, dither: bool) {
    let ext = image
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let is_photo = PHOTO_EXTENSIONS.contains(&ext.as_str());

    if label.is_none() {
        match image::image_dimensions(image) {
            Ok((w, h)) if w > 384 || h > 280 => eprintln!(
                "warning: no --label set; printing raw {w}×{h} px. \
                 Wrong size / tiny sticker? use --label 50x30"
            ),
            Ok(_) => {}
            Err(_) => eprintln!(
                "tip: pass --label 50x30 so the image is scaled to a full physical sticker"
            ),
        }
    }

    if is_photo && !dither {
        eprintln!(
            "tip: this looks like a photo file (.{ext}). \
             Try --dither --no-fill --margin 16 -d 3 for cleaner midtones"
        );
    }
    if is_photo && !no_fill && label.is_some() {
        eprintln!("tip: default --fill may crop the photo; use --no-fill to fit the whole image");
    }
}

/// Substring → hint pairs appended to stderr after a failure.
const ERROR_HINTS: &[(&[&str], &str)] = &[
    (
        &["ble", "bluetooth", "connect", "transport"],
        "tip: quit any official label app (one BLE client only). \
         Full name: `thermark scan` then -a \"B1-…\". Exact match by default; --fuzzy if needed.",
    ),
    (
        &["cover", "lackpaper", "no paper"],
        "tip: close the lid fully; load labels with 2–5 mm sticking out of the exit slot",
    ),
    (
        &["timeout"],
        "tip: run `thermark doctor --use-config` for lid/paper/BLE readiness",
    ),
    (
        &["image width", "too wide"],
        "tip: --label 50x30 scales to the sticker; max width is 384 px on B1",
    ),
];

/// Extra stderr tips after a failure (does not change the error itself).
pub fn emit_error_tips(err: &anyhow::Error) {
    let text = format!("{err:#}").to_ascii_lowercase();
    for (needles, hint) in ERROR_HINTS {
        if needles.iter().any(|n| text.contains(n)) {
            eprintln!("{hint}");
        }
    }
    if text.contains("password") && text.contains("wifi") {
        eprintln!("tip: THERMARK_WIFI_PASSWORD=… avoids leaving the secret in shell history");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_path_is_refused() {
        assert!(guard_wifi_save_path(Path::new("fixtures/x.png")).is_err());
        assert!(guard_wifi_save_path(Path::new("a/fixtures/x.png")).is_err());
        assert!(guard_wifi_save_path(Path::new("local/prints/x.png")).is_ok());
    }
}
