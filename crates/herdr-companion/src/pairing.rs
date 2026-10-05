//! Pairing a phone by QR code. The code encodes the web app's URL with the
//! token in the fragment (`#token=…`): browsers never send a fragment to the
//! server, so the token stays out of request lines, logs, and `Referer`.
//! Anyone who sees the code can use the companion, so it is shown only on
//! an interactive terminal.

use crate::Result;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use qrcode::{QrCode, render::unicode::Dense1x2};

/// The link a phone opens to reach the web app already signed in.
pub fn pairing_url(base: &str, token: &str) -> String {
    let token = utf8_percent_encode(token, NON_ALPHANUMERIC);
    format!("{}/#token={token}", base.trim_end_matches('/'))
}

/// `text` as a QR code drawn with half-block characters, two modules per
/// character cell. Light modules are drawn as blocks, so the code reads
/// correctly on the dark background most terminals use.
pub fn render_qr(text: &str) -> Result<String> {
    let code = QrCode::new(text.as_bytes())?;
    Ok(code
        .render::<Dense1x2>()
        .dark_color(Dense1x2::Light)
        .light_color(Dense1x2::Dark)
        .quiet_zone(true)
        .build())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_token_rides_in_the_fragment() {
        assert_eq!(
            pairing_url("https://box.ts.net/", "abc123"),
            "https://box.ts.net/#token=abc123"
        );
        // Characters that would end or split the fragment are escaped.
        assert_eq!(
            pairing_url("http://h:1", "a b&c#d"),
            "http://h:1/#token=a%20b%26c%23d"
        );
    }

    #[test]
    fn renders_a_square_code_with_a_quiet_zone() {
        let qr = render_qr(&pairing_url("https://box.ts.net", &"f".repeat(64))).unwrap();
        let lines: Vec<&str> = qr.lines().collect();
        let width = lines[0].chars().count();
        assert!(lines.iter().all(|line| line.chars().count() == width));
        // Two modules per text row: the module grid is as tall as it is wide.
        assert!(
            (lines.len() * 2).abs_diff(width) <= 1,
            "{} rows for {width} columns",
            lines.len()
        );
        // The quiet zone is light, which this renderer draws as full blocks.
        assert!(lines[0].chars().all(|ch| ch == '█'));
    }

    #[test]
    fn refuses_text_too_long_for_a_code() {
        assert!(matches!(
            render_qr(&"x".repeat(8000)),
            Err(crate::Error::Qr(_))
        ));
    }
}
