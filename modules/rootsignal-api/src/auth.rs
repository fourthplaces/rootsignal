use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    http::request::Parts,
    response::{IntoResponse, Redirect, Response},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use rootsignal_common::Config;

use crate::AppState;

type HmacSha256 = Hmac<Sha256>;

const COOKIE_NAME: &str = "rs_session";
const SESSION_DURATION_SECS: i64 = 7 * 24 * 3600; // 7 days

/// Return the session signing secret. Prefers SESSION_SECRET env var;
/// falls back to admin_password (for dev compatibility).
pub fn session_secret(config: &Config) -> &str {
    if config.session_secret.is_empty() {
        &config.admin_password
    } else {
        &config.session_secret
    }
}

/// Authenticated admin session. Extract this in handlers that require auth.
/// If the session cookie is missing or invalid, returns a redirect to /admin/login.
pub struct AdminSession {
    pub phone: String,
}

impl FromRequestParts<Arc<AppState>> for AdminSession {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let app_state = state;

        let cookie_header = parts
            .headers
            .get(axum::http::header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let session_value = parse_cookie(cookie_header, COOKIE_NAME);

        if let Some(value) = session_value {
            if let Some(phone) = verify_session(&value, session_secret(&app_state.config)) {
                return Ok(AdminSession { phone });
            }
        }

        // Not authenticated — redirect to login
        Err(Redirect::to("/admin/login").into_response())
    }
}

/// Create a signed session cookie value: `phone|expiry|signature`
pub fn create_session(phone: &str, secret: &str) -> String {
    let expiry = chrono::Utc::now().timestamp() + SESSION_DURATION_SECS;
    let payload = format!("{phone}|{expiry}");
    let sig = sign(&payload, secret);
    format!("{payload}|{sig}")
}

/// Build the Set-Cookie header value.
/// In release builds, adds `Secure` flag to prevent transmission over HTTP.
pub fn session_cookie(phone: &str, secret: &str) -> String {
    let value = create_session(phone, secret);
    let secure = if cfg!(debug_assertions) { "" } else { "; Secure" };
    format!(
        "{COOKIE_NAME}={value}; Path=/admin; HttpOnly; SameSite=Lax; Max-Age={SESSION_DURATION_SECS}{secure}"
    )
}

/// Build a Set-Cookie header that clears the session.
pub fn clear_session_cookie() -> String {
    format!("{COOKIE_NAME}=; Path=/admin; HttpOnly; SameSite=Lax; Max-Age=0")
}

/// Verify a session cookie value. Returns the phone number if valid.
fn verify_session(value: &str, secret: &str) -> Option<String> {
    let parts: Vec<&str> = value.splitn(3, '|').collect();
    if parts.len() != 3 {
        return None;
    }

    let phone = parts[0];
    let expiry_str = parts[1];
    let sig = parts[2];

    // Verify signature
    let payload = format!("{phone}|{expiry_str}");
    let expected_sig = sign(&payload, secret);
    if !constant_time_eq(sig.as_bytes(), expected_sig.as_bytes()) {
        return None;
    }

    // Check expiry
    let expiry: i64 = expiry_str.parse().ok()?;
    if chrono::Utc::now().timestamp() > expiry {
        return None;
    }

    Some(phone.to_string())
}

fn sign(payload: &str, secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Constant-time comparison to prevent timing attacks.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Parse a specific cookie from the Cookie header string.
fn parse_cookie<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    for part in header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix(name) {
            if let Some(value) = value.strip_prefix('=') {
                return Some(value);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_session() {
        let secret = "test-secret-key";
        let phone = "+15551234567";
        let cookie_value = create_session(phone, secret);
        let result = verify_session(&cookie_value, secret);
        assert_eq!(result, Some(phone.to_string()));
    }

    #[test]
    fn rejects_tampered_session() {
        let secret = "test-secret-key";
        let cookie_value = create_session("+15551234567", secret);
        // Tamper with the phone number
        let tampered = cookie_value.replacen("+15551234567", "+15559999999", 1);
        assert_eq!(verify_session(&tampered, secret), None);
    }

    #[test]
    fn rejects_wrong_secret() {
        let cookie_value = create_session("+15551234567", "secret-a");
        assert_eq!(verify_session(&cookie_value, "secret-b"), None);
    }

    #[test]
    fn rejects_expired_session() {
        let phone = "+15551234567";
        let secret = "test-secret";
        // Manually create an expired session
        let expiry = chrono::Utc::now().timestamp() - 100;
        let payload = format!("{phone}|{expiry}");
        let sig = sign(&payload, secret);
        let value = format!("{payload}|{sig}");
        assert_eq!(verify_session(&value, secret), None);
    }

    #[test]
    fn parse_cookie_works() {
        assert_eq!(
            parse_cookie("rs_session=abc123; other=xyz", "rs_session"),
            Some("abc123")
        );
        assert_eq!(
            parse_cookie("other=xyz; rs_session=abc123", "rs_session"),
            Some("abc123")
        );
        assert_eq!(parse_cookie("other=xyz", "rs_session"), None);
    }
}
