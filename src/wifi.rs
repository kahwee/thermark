//! Guest Wi‑Fi stickers: standard `WIFI:…` QR payload + human-readable SSID.
//!
//! Phone cameras understand:
//! ```text
//! WIFI:T:WPA;S:<ssid>;P:<password>;;
//! ```
//! Scan → join network. The sticker text shows the **network name** clearly;
//! the password stays in the QR (optional cleartext with care).

use crate::errors::{Error, Result};
use crate::geometry::LabelPx;
use crate::label::{QrLabelOptions, TextSide, make_qr_label_opts};
use image::GrayImage;

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
    fn tag(self) -> &'static str {
        match self {
            Self::Wpa => "WPA",
            Self::Wep => "WEP",
            Self::Nopass => "nopass",
        }
    }
}

/// Escape special characters in WIFI QR fields (`\`, `;`, `,`, `"`).
pub fn escape_wifi_field(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | ';' | ',' | '"' => {
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
    if matches!(security, WifiSecurity::Wpa | WifiSecurity::Wep) && password.is_empty() {
        return Err(Error::msg(
            "password required for WPA/WEP (use --security nopass for open networks)",
        ));
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
    pub text_side: TextSide,
    pub font_path: Option<std::path::PathBuf>,
    pub font_name: Option<String>,
    pub font_size: Option<f32>,
    pub border: bool,
}

/// Render a 50×30-class Wi‑Fi sticker: join QR + clear network name.
pub fn make_wifi_label(opts: &WifiLabelOptions) -> Result<GrayImage> {
    let payload = wifi_qr_payload(&opts.ssid, &opts.password, opts.security, opts.hidden)?;
    let side = if opts.show_password {
        wifi_side_text(&opts.ssid, Some(&opts.password))
    } else {
        wifi_side_text(&opts.ssid, None)
    };
    make_qr_label_opts(&QrLabelOptions {
        url: payload,
        side_text: side,
        label: opts.label,
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
        assert!(p.ends_with(";;") || p.ends_with(";"));
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
    fn rejects_empty_ssid() {
        assert!(wifi_qr_payload("", "x", WifiSecurity::Wpa, false).is_err());
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
