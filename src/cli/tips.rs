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
             or omit --save to print without saving. See local/README.md"
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

const BLE_HINT: &str = "tip: quit any official label app (one BLE client only). \
    Full name: `thermark scan` then -a \"B1-…\". Exact match by default; --fuzzy if needed.";
const USB_HINT: &str = "tip: run `thermark ports` to verify the serial device path, then retry \
    with `--conn usb --addr <path>`";
const MEDIA_HINT: &str =
    "tip: close the lid fully; load labels with 2–5 mm sticking out of the exit slot";
const TIMEOUT_HINT: &str =
    "tip: run `thermark doctor --use-config` for lid/paper/connection readiness";
const IMAGE_WIDTH_HINT: &str =
    "tip: pass the correct --label WxH so thermark scales to the selected printer profile";
const PASSWORD_HINT: &str =
    "tip: THERMARK_WIFI_PASSWORD=… avoids leaving the secret in shell history";

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn first_word(text: &str, words: &[&str]) -> Option<usize> {
    words
        .iter()
        .flat_map(|word| text.match_indices(word))
        .filter_map(|(index, word)| {
            let before = text[..index].chars().next_back();
            let after = text[index + word.len()..].chars().next();
            let bounded = before.is_none_or(|ch| !ch.is_ascii_alphanumeric())
                && after.is_none_or(|ch| !ch.is_ascii_alphanumeric());
            bounded.then_some(index)
        })
        .min()
}

/// Select recovery hints from a fully formatted error chain.
///
/// Real transport errors identify themselves as BLE or serial. Requiring that
/// explicit marker prevents a generic `connect` or `transport` context from
/// sending a USB user toward BLE-only recovery steps.
fn hints_for_error(text: &str) -> Vec<&'static str> {
    let text = text.to_ascii_lowercase();
    let mut hints = Vec::new();

    let ble_at = first_word(&text, &["ble", "bluetooth"]);
    let usb_at = first_word(&text, &["serial", "usb"]);
    let transport_hint = match (ble_at, usb_at) {
        (Some(ble), Some(usb)) if ble <= usb => {
            (!text.contains("without bluetooth support")).then_some(BLE_HINT)
        }
        (_, Some(_)) => (!text.contains("without usb serial support")).then_some(USB_HINT),
        (Some(_), None) => (!text.contains("without bluetooth support")).then_some(BLE_HINT),
        _ => None,
    };
    if let Some(hint) = transport_hint {
        hints.push(hint);
    }

    if contains_any(&text, &["cover", "lackpaper", "no paper"]) {
        hints.push(MEDIA_HINT);
    }
    if text.contains("timeout") {
        hints.push(TIMEOUT_HINT);
    }
    if contains_any(&text, &["image width", "too wide"]) {
        hints.push(IMAGE_WIDTH_HINT);
    }
    if text.contains("password") && text.contains("wifi") {
        hints.push(PASSWORD_HINT);
    }

    hints
}

/// Extra stderr tips after a failure (does not change the error itself).
pub fn emit_error_tips(err: &anyhow::Error) {
    for hint in hints_for_error(&format!("{err:#}")) {
        eprintln!("{hint}");
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

    #[test]
    fn transport_hints_follow_the_named_transport() {
        assert_eq!(
            hints_for_error("BLE connect: transport unavailable"),
            [BLE_HINT]
        );
        assert_eq!(
            hints_for_error("open serial /dev/cu.usb: transport: permission denied"),
            [USB_HINT]
        );
        assert_eq!(
            hints_for_error(
                "BLE connect: transport unavailable; a matching serial endpoint may exist"
            ),
            [BLE_HINT]
        );
        assert!(hints_for_error("transport: connection reset").is_empty());
        assert!(hints_for_error("transport unavailable").is_empty());
        assert!(hints_for_error("this binary was built without USB serial support").is_empty());
        assert!(hints_for_error("this binary was built without Bluetooth support").is_empty());
    }

    #[test]
    fn image_width_hint_is_profile_neutral() {
        let hints = hints_for_error("image width 400px exceeds printer max 96px");
        assert_eq!(hints, [IMAGE_WIDTH_HINT]);
        assert!(!hints[0].contains("384"));
        assert!(!hints[0].contains("B1"));
    }

    #[test]
    fn non_transport_guidance_is_preserved() {
        assert_eq!(hints_for_error("cover open"), [MEDIA_HINT]);
        assert_eq!(
            hints_for_error("timeout waiting for response"),
            [TIMEOUT_HINT]
        );
        assert_eq!(hints_for_error("WiFi password required"), [PASSWORD_HINT]);
    }
}
