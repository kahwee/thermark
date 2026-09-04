//! Guest Wi‑Fi stickers: standard `WIFI:…` QR payload + human-readable SSID.
//!
//! Phone cameras understand:
//! ```text
//! WIFI:T:WPA;S:<ssid>;P:<password>;;
//! ```
//! Scan → join network. The sticker text shows the **network name** clearly;
//! the password stays in the QR (optional cleartext with care).

use crate::errors::{Error, Result};
use crate::geometry::{LabelPx, SafeArea};
use crate::label::{QrLabelOptions, TextSide, make_qr_label_opts};
use image::GrayImage;

/// Maximum SSID length. The 802.11 limit is 32 **bytes**, not characters.
pub const SSID_MAX_BYTES: usize = 32;

/// Security type for the WIFI QR payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum WifiSecurity {
    /// WPA/WPA2/WPA3 personal (most home routers).
    #[default]
    #[value(name = "wpa", alias = "wpa2", alias = "wpa3")]
    Wpa,
    /// Legacy WEP (rare).
    #[value(name = "wep")]
    Wep,
    /// Open network (no password).
    #[value(name = "nopass", alias = "open", alias = "none")]
    Nopass,
}

impl WifiSecurity {
    /// Whether this security mode needs a password in the QR payload.
    pub const fn requires_password(self) -> bool {
        matches!(self, Self::Wpa | Self::Wep)
    }

    fn tag(self) -> &'static str {
        match self {
            Self::Wpa => "WPA",
            Self::Wep => "WEP",
            Self::Nopass => "nopass",
        }
    }
}

/// Escape the characters reserved in WIFI QR fields: `\`, `;`, `,`, `"`, `:`.
///
/// `:` matters because it separates a field's key from its value. Most phone
/// parsers split on the first one and cope, but a strict reader will mis-parse
/// a password containing a colon, so escape it per the specification.
pub fn escape_wifi_field(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | ';' | ',' | '"' | ':' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Build the standard WIFI QR payload phones understand for one-tap join.
///
/// Format: `WIFI:T:<type>;S:<ssid>;P:<password>;H:<hidden>;;`
/// (password omitted when empty / open network.)
pub fn wifi_qr_payload(
    ssid: &str,
    password: &str,
    security: WifiSecurity,
    hidden: bool,
) -> Result<String> {
    let ssid = ssid.trim();
    if ssid.is_empty() {
        return Err(Error::msg("Wi‑Fi SSID must not be empty"));
    }
    if security.requires_password() && password.is_empty() {
        return Err(Error::msg(
            "password required for WPA/WEP. Pass --password … or set THERMARK_WIFI_PASSWORD \
             (keeps the secret out of shell history). Open network: --security nopass",
        ));
    }
    // The 802.11 limit is 32 *bytes*, not characters: a 17-character Japanese
    // SSID is 43 bytes and invalid, while a 32-character ASCII one is fine.
    if ssid.len() > SSID_MAX_BYTES {
        return Err(Error::msg(format!(
            "SSID is {n} bytes (Wi‑Fi max is {SSID_MAX_BYTES}); shorten the network name",
            n = ssid.len()
        )));
    }

    let mut s = String::from("WIFI:");
    s.push_str("T:");
    s.push_str(security.tag());
    s.push(';');
    s.push_str("S:");
    s.push_str(&escape_wifi_field(ssid));
    s.push(';');
    if !password.is_empty() && !matches!(security, WifiSecurity::Nopass) {
        s.push_str("P:");
        s.push_str(&escape_wifi_field(password));
        s.push(';');
    }
    if hidden {
        s.push_str("H:true;");
    }
    s.push(';');
    Ok(s)
}

/// Human-readable side text: network name large and clear (password optional).
pub fn wifi_side_text(ssid: &str, show_password: Option<&str>) -> String {
    let ssid = ssid.trim();
    match show_password {
        Some(pw) if !pw.is_empty() => format!("Wi‑Fi\n{ssid}\n{pw}"),
        _ => format!("Wi‑Fi\n{ssid}\nScan to join"),
    }
}

/// Options for a guest Wi‑Fi sticker.
#[derive(Debug, Clone)]
pub struct WifiLabelOptions {
    pub ssid: String,
    pub password: String,
    pub security: WifiSecurity,
    pub hidden: bool,
    /// If true, print password in cleartext under the SSID (less secure).
    pub show_password: bool,
    pub label: LabelPx,
    pub safe: SafeArea,
    pub text_side: TextSide,
    pub font_path: Option<std::path::PathBuf>,
    pub font_name: Option<String>,
    pub font_size: Option<f32>,
    pub border: bool,
}

/// Render a 50×30-class Wi‑Fi sticker: join QR + clear network name.
pub fn make_wifi_label(opts: &WifiLabelOptions) -> Result<GrayImage> {
    let payload = wifi_qr_payload(&opts.ssid, &opts.password, opts.security, opts.hidden)?;
    // Keep open-network semantics at the public renderer boundary too. CLI
    // callers normalize this already, but library callers may carry a stale
    // password alongside `Nopass`; it must never appear on that label.
    let shown_password =
        (opts.show_password && opts.security.requires_password()).then_some(opts.password.as_str());
    let side = wifi_side_text(&opts.ssid, shown_password);
    make_qr_label_opts(&QrLabelOptions {
        url: payload,
        side_text: side,
        label: opts.label,
        safe: opts.safe,
        text_side: opts.text_side,
        border: opts.border,
        font_path: opts.font_path.clone(),
        font_name: opts.font_name.clone(),
        font_size: opts.font_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_wpa_roundtrip_shape() {
        let p = wifi_qr_payload("Cafe-Guest", "s3cret!", WifiSecurity::Wpa, false).unwrap();
        assert!(p.starts_with("WIFI:T:WPA;S:Cafe-Guest;P:"));
        assert!(p.contains("P:s3cret!;"));
        // `ends_with(";;") || ends_with(";")` was tautological — the second
        // clause subsumes the first, so it asserted nothing.
        assert!(
            p.ends_with(";;"),
            "payload must be terminated with ';;': {p}"
        );
    }

    #[test]
    fn escapes_special_chars_in_ssid() {
        let p = wifi_qr_payload(r"Net;Work", "x", WifiSecurity::Wpa, false).unwrap();
        assert!(p.contains(r"S:Net\;Work;"), "{p}");
    }

    #[test]
    fn open_network_omits_password() {
        let p = wifi_qr_payload("OpenNet", "", WifiSecurity::Nopass, false).unwrap();
        assert!(!p.contains("P:"), "{p}");
        assert!(p.contains("T:nopass"));
    }

    #[test]
    fn open_network_label_never_renders_a_stale_password() {
        let font_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts/DejaVuSans.ttf");
        let baseline = WifiLabelOptions {
            ssid: "OpenNet".into(),
            password: String::new(),
            security: WifiSecurity::Nopass,
            hidden: false,
            show_password: false,
            label: LabelPx {
                width_px: 384,
                height_px: 240,
            },
            safe: SafeArea::B1,
            text_side: TextSide::Right,
            font_path: Some(font_path),
            font_name: None,
            font_size: None,
            border: false,
        };
        let expected = make_wifi_label(&baseline).unwrap();
        let actual = make_wifi_label(&WifiLabelOptions {
            password: "must-not-appear".into(),
            show_password: true,
            ..baseline
        })
        .unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn rejects_empty_ssid() {
        assert!(wifi_qr_payload("", "x", WifiSecurity::Wpa, false).is_err());
    }

    #[test]
    fn rejects_ssid_over_32_bytes() {
        let long = "a".repeat(33);
        let err = wifi_qr_payload(&long, "x", WifiSecurity::Wpa, false).unwrap_err();
        assert!(err.to_string().contains("32"), "{err}");

        // 12 characters, 36 bytes: over the real limit though under 32 chars.
        let multibyte = "ネットワーク名前あいうえお"
            .chars()
            .take(12)
            .collect::<String>();
        assert!(multibyte.chars().count() < 32 && multibyte.len() > 32);
        assert!(wifi_qr_payload(&multibyte, "x", WifiSecurity::Wpa, false).is_err());

        // Exactly 32 bytes is allowed.
        assert!(wifi_qr_payload(&"a".repeat(32), "x", WifiSecurity::Wpa, false).is_ok());
    }

    #[test]
    fn escapes_colon_in_password() {
        let p = wifi_qr_payload("Net", "pa:ss", WifiSecurity::Wpa, false).unwrap();
        assert!(p.contains(r"P:pa\:ss;"), "{p}");
    }

    #[test]
    fn side_text_hides_password_by_default() {
        let t = wifi_side_text("HomeLAN", None);
        assert!(t.contains("HomeLAN"));
        assert!(t.contains("Scan to join"));
        assert!(!t.contains("secret"));
    }

    #[test]
    fn side_text_can_show_password() {
        let t = wifi_side_text("HomeLAN", Some("hunter2"));
        assert!(t.contains("HomeLAN"));
        assert!(t.contains("hunter2"));
    }
}
