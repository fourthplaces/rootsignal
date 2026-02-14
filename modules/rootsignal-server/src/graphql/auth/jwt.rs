use anyhow::Result;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

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

    pub fn create_token(&self, phone_number: String, is_admin: bool) -> Result<String> {
        let now = chrono::Utc::now();
        let exp = now + chrono::Duration::hours(24);

        let claims = Claims {
            sub: phone_number.clone(),
            phone_number,
            is_admin,
            exp: exp.timestamp(),
            iat: now.timestamp(),
            iss: self.issuer.clone(),
            jti: uuid::Uuid::new_v4().to_string(),
        };

        encode(&Header::default(), &claims, &self.encoding_key).map_err(Into::into)
    }

    pub fn verify_token(&self, token: &str) -> Result<Claims> {
        let mut validation = Validation::default();
        validation.set_issuer(&[&self.issuer]);

        decode::<Claims>(token, &self.decoding_key, &validation)
            .map(|data| data.claims)
            .map_err(Into::into)
    }
}
