pub mod jwt;
pub mod middleware;

use async_graphql::*;
use rootsignal_core::ServerDeps;
use std::sync::Arc;

use self::jwt::JwtService;

#[derive(Default)]
pub struct AuthMutation;

#[Object]
impl AuthMutation {
    /// Send a verification code via SMS to the given phone number.
    async fn send_verification_code(&self, ctx: &Context<'_>, phone: String) -> Result<bool> {
        tracing::info!("graphql.send_verification_code");
        let deps = ctx.data::<Arc<ServerDeps>>()?;

        // Validate phone format
        if !phone.starts_with('+') || phone.len() < 10 {
            return Err(Error::new(
                "Invalid phone number. Use E.164 format (e.g. +15551234567)",
            ));
        }

        // Check if this is a test identifier
        if deps.config.test_identifier_enabled && phone == "+1234567890" {
            return Ok(true);
        }

        // Validate Twilio is configured
        let account_sid = deps
            .config
            .twilio_account_sid
            .as_ref()
            .ok_or_else(|| Error::new("Twilio not configured"))?;
        let auth_token = deps
            .config
            .twilio_auth_token
            .as_ref()
            .ok_or_else(|| Error::new("Twilio not configured"))?;
        let service_sid = deps
            .config
            .twilio_verify_service_sid
            .as_ref()
            .ok_or_else(|| Error::new("Twilio not configured"))?;

        let twilio = twilio::TwilioService::new(twilio::TwilioOptions {
            account_sid: account_sid.clone(),
            auth_token: auth_token.clone(),
            service_id: service_sid.clone(),
        });

        twilio
            .send_otp(&phone)
            .await
            .map_err(|e| Error::new(format!("Failed to send verification code: {e}")))?;

        Ok(true)
    }

    /// Verify an OTP code and return a JWT token on success.
    async fn verify_code(&self, ctx: &Context<'_>, phone: String, code: String) -> Result<String> {
        let deps = ctx.data::<Arc<ServerDeps>>()?;

        // Check test identifier
        if deps.config.test_identifier_enabled && phone == "+1234567890" && code == "123456" {
            let jwt_service = ctx.data::<JwtService>()?;
            let is_admin = deps.config.admin_phone_numbers.contains(&phone);
            tracing::info!(
                phone = %phone,
                is_admin = is_admin,
                admin_phones = ?deps.config.admin_phone_numbers,
                "Test identifier login"
            );
            let token = jwt_service
                .create_token(phone, is_admin)
                .map_err(|e| Error::new(format!("Failed to create token: {e}")))?;
            return Ok(token);
        }

        // Validate Twilio is configured
        let account_sid = deps
            .config
            .twilio_account_sid
            .as_ref()
            .ok_or_else(|| Error::new("Twilio not configured"))?;
        let auth_token = deps
            .config
            .twilio_auth_token
            .as_ref()
            .ok_or_else(|| Error::new("Twilio not configured"))?;
        let service_sid = deps
            .config
            .twilio_verify_service_sid
            .as_ref()
            .ok_or_else(|| Error::new("Twilio not configured"))?;

        let twilio = twilio::TwilioService::new(twilio::TwilioOptions {
            account_sid: account_sid.clone(),
            auth_token: auth_token.clone(),
            service_id: service_sid.clone(),
        });

        twilio
            .verify_otp(&phone, &code)
            .await
            .map_err(|e| Error::new(format!("Verification failed: {e}")))?;

        // OTP verified — issue JWT
        let jwt_service = ctx.data::<JwtService>()?;
        let is_admin = deps.config.admin_phone_numbers.contains(&phone);
        let token = jwt_service
            .create_token(phone, is_admin)
            .map_err(|e| Error::new(format!("Failed to create token: {e}")))?;

        Ok(token)
    }
}
