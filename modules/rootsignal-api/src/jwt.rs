use anyhow::Result;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const TOKEN_DURATION_SECS: i64 = 24 * 3600; // 24 hours
const COOKIE_NAME: &str = "auth_token";

/// JWT Claims stored in the token.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub phone_number: String,
    pub is_admin: bool,
    pub exp: i64,
    pub iat: i64,
    pub iss: String,
    pub jti: String,
}

/// JWT service for creating and verifying tokens.
#[derive(Clone)]
pub struct JwtService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    issuer: String,
}

impl JwtService {
    pub fn new(secret: &str, issuer: String) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
            issuer,
        }
    }

    /// Create a JWT token. The `sub` claim is a deterministic UUID derived
    /// from the phone number hash, so we don't need a persistent user table.
    pub fn create_token(&self, phone_number: &str, is_admin: bool) -> Result<String> {
        let now = chrono::Utc::now();
        let exp = now + chrono::Duration::seconds(TOKEN_DURATION_SECS);
        let member_id = phone_to_uuid(phone_number);

        let claims = Claims {
            sub: member_id.to_string(),
            phone_number: phone_number.to_string(),
            is_admin,
            exp: exp.timestamp(),
            iat: now.timestamp(),
            iss: self.issuer.clone(),
            jti: Uuid::new_v4().to_string(),
        };

        encode(&Header::default(), &claims, &self.encoding_key).map_err(Into::into)
    }

    /// Verify and decode a JWT token. Returns claims if valid and not expired.
    pub fn verify_token(&self, token: &str) -> Result<Claims> {
        let mut validation = Validation::default();
        validation.set_issuer(&[&self.issuer]);

        decode::<Claims>(token, &self.decoding_key, &validation)
            .map(|data| data.claims)
            .map_err(Into::into)
    }
}

/// Derive a deterministic UUID v5-style ID from a phone number.
fn phone_to_uuid(phone: &str) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(b"rootsignal-member:");
    hasher.update(phone.as_bytes());
    let hash = hasher.finalize();
    // Use first 16 bytes of SHA-256 as UUID
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    // Set version 4 bits (we're faking it, but it's a valid UUID format)
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// Build a Set-Cookie header that sets the JWT token.
pub fn jwt_cookie(token: &str) -> String {
    let secure = if cfg!(debug_assertions) {
        ""
    } else {
        "; Secure"
    };
    format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={TOKEN_DURATION_SECS}{secure}"
    )
}

/// Build a Set-Cookie header that clears the JWT cookie.
pub fn clear_jwt_cookie() -> String {
    format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

/// Parse the auth_token cookie value from a Cookie header string.
pub fn parse_auth_cookie(header: &str) -> Option<&str> {
    for part in header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix(COOKIE_NAME) {
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

    fn test_service() -> JwtService {
        JwtService::new("test-secret-key", "rootsignal".to_string())
    }

    #[test]
    fn roundtrip_token() {
        let svc = test_service();
        let token = svc.create_token("+15551234567", true).unwrap();
        let claims = svc.verify_token(&token).unwrap();
        assert_eq!(claims.phone_number, "+15551234567");
        assert!(claims.is_admin);
        assert_eq!(claims.iss, "rootsignal");
    }

    #[test]
    fn deterministic_member_id() {
        let svc = test_service();
        let t1 = svc.create_token("+15551234567", true).unwrap();
        let t2 = svc.create_token("+15551234567", true).unwrap();
        let c1 = svc.verify_token(&t1).unwrap();
        let c2 = svc.verify_token(&t2).unwrap();
        assert_eq!(c1.sub, c2.sub);
    }

    #[test]
    fn different_phones_different_ids() {
        let svc = test_service();
        let t1 = svc.create_token("+15551234567", true).unwrap();
        let t2 = svc.create_token("+15559999999", true).unwrap();
        let c1 = svc.verify_token(&t1).unwrap();
        let c2 = svc.verify_token(&t2).unwrap();
        assert_ne!(c1.sub, c2.sub);
    }

    #[test]
    fn rejects_invalid_token() {
        let svc = test_service();
        assert!(svc.verify_token("garbage").is_err());
    }

    #[test]
    fn rejects_wrong_secret() {
        let svc1 = JwtService::new("secret-a", "rootsignal".to_string());
        let svc2 = JwtService::new("secret-b", "rootsignal".to_string());
        let token = svc1.create_token("+15551234567", false).unwrap();
        assert!(svc2.verify_token(&token).is_err());
    }

    #[test]
    fn token_expiry_is_24h() {
        let svc = test_service();
        let token = svc.create_token("+15551234567", false).unwrap();
        let claims = svc.verify_token(&token).unwrap();
        let expires_in = claims.exp - claims.iat;
        assert_eq!(expires_in, 24 * 3600);
    }

    #[test]
    fn parse_cookie() {
        assert_eq!(
            parse_auth_cookie("auth_token=abc123; other=xyz"),
            Some("abc123")
        );
        assert_eq!(
            parse_auth_cookie("other=xyz; auth_token=abc123"),
            Some("abc123")
        );
        assert_eq!(parse_auth_cookie("other=xyz"), None);
    }
}
