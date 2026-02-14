// https://dev.to/hackmamba/how-to-build-a-one-time-passwordotp-verification-api-with-rust-and-twilio-22il

use std::collections::HashMap;

pub mod models;
use reqwest::{header, Client};

use crate::models::{OTPResponse, OTPVerifyResponse};
use serde_json::Value;

/// Check if a string is a valid email address
fn is_email(identifier: &str) -> bool {
    identifier.contains('@') && identifier.contains('.')
}

/// Check if a string is a valid phone number (E.164 format)
fn is_phone_number(identifier: &str) -> bool {
    identifier.starts_with('+') && identifier.len() >= 10
}

#[derive(Debug, Clone)]
pub struct TwilioOptions {
    pub account_sid: String,
    pub auth_token: String,
    pub service_id: String,
}

#[derive(Debug, Clone)]
pub struct TwilioService {
    options: TwilioOptions,
}

impl TwilioService {
    pub fn new(options: TwilioOptions) -> Self {
        Self { options }
    }

    pub async fn send_otp(
        self: &TwilioService,
        recipient: &str,
    ) -> Result<OTPResponse, &'static str> {
        let account_sid = self.options.account_sid.clone();
        let auth_token = self.options.auth_token.clone();
        let service_id = self.options.service_id.clone();

        // Validate recipient format
        let channel = if is_email(recipient) {
            "email"
        } else if is_phone_number(recipient) {
            "sms"
        } else {
            eprintln!("Invalid recipient format: {}", recipient);
            eprintln!("Expected: email (user@example.com) or E.164 phone (+1234567890)");
            return Err("Invalid recipient format");
        };

        let url = format!(
            "https://verify.twilio.com/v2/Services/{serv_id}/Verifications",
            serv_id = service_id
        );

        let mut headers = header::HeaderMap::new();
        headers.insert(
            "Content-Type",
            "application/x-www-form-urlencoded"
                .parse()
                .expect("Header value should parse correctly"),
        );

        let mut form_body: HashMap<&str, String> = HashMap::new();
        form_body.insert("To", recipient.to_string());
        form_body.insert("Channel", channel.to_string());

        let client = Client::new();
        let res = client
            .post(url)
            .basic_auth(account_sid, Some(auth_token))
            .headers(headers)
            .form(&form_body)
            .send()
            .await;

        match res {
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    let error_body = response.text().await.unwrap_or_default();
                    eprintln!("Twilio error ({}): {}", status, error_body);

                    // Parse error to provide more helpful messages
                    if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&error_body) {
                        if let Some(code) = error_json.get("code").and_then(|c| c.as_i64()) {
                            match code {
                                60200 => {
                                    // Invalid parameter - often means email channel not enabled
                                    if channel == "email" {
                                        eprintln!("Email channel may not be enabled on your Twilio Verify Service.");
                                        eprintln!("Enable it at: https://console.twilio.com/us1/develop/verify/services");
                                        return Err("Email verification not enabled");
                                    }
                                    return Err("Invalid parameter");
                                }
                                60203 => {
                                    eprintln!("Maximum check attempts reached");
                                    return Err("Too many verification attempts");
                                }
                                60202 => {
                                    eprintln!("Maximum send attempts reached");
                                    return Err("Too many send attempts");
                                }
                                _ => return Err("Twilio returned an error"),
                            }
                        }
                    }

                    return Err("Twilio returned an error");
                }

                let result = response.json::<OTPResponse>().await;
                match result {
                    Ok(data) => Ok(data),
                    Err(e) => {
                        eprintln!("Failed to parse Twilio response: {}", e);
                        Err("Error parsing OTP response")
                    }
                }
            }
            Err(e) => {
                eprintln!("Request to Twilio failed: {}", e);
                Err("Error sending OTP")
            }
        }
    }

    pub async fn verify_otp(&self, recipient: &str, code: &str) -> Result<(), &'static str> {
        let account_sid = self.options.account_sid.clone();
        let auth_token = self.options.auth_token.clone();
        let service_id = self.options.service_id.clone();

        let url = format!(
            "https://verify.twilio.com/v2/Services/{serv_id}/VerificationCheck",
            serv_id = service_id,
        );

        let mut headers = header::HeaderMap::new();
        headers.insert(
            "Content-Type",
            "application/x-www-form-urlencoded"
                .parse()
                .expect("Header value should parse correctly"),
        );

        let mut form_body: HashMap<&str, &str> = HashMap::new();
        form_body.insert("To", recipient);
        form_body.insert("Code", code);

        let client = Client::new();
        let res = client
            .post(url)
            .basic_auth(account_sid, Some(auth_token))
            .headers(headers)
            .form(&form_body)
            .send()
            .await;

        match res {
            Ok(response) => {
                let data = response.json::<OTPVerifyResponse>().await;
                match data {
                    Ok(result) => {
                        if result.status == "approved" {
                            Ok(())
                        } else {
                            Err("Error verifying OTP")
                        }
                    }
                    Err(_) => Err("Error verifying OTP"),
                }
            }
            Err(_) => Err("Error verifying OTP"),
        }
    }

    pub async fn fetch_ice_servers(&self) -> Result<Value, &'static str> {
        let account_sid = self.options.account_sid.clone();
        let auth_token = self.options.auth_token.clone();

        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Tokens.json",
            account_sid
        );

        let client = Client::new();
        let response = client
            .post(url)
            .basic_auth(account_sid, Some(auth_token))
            .form(&HashMap::<&str, &str>::new())
            .send()
            .await;

        match response {
            Ok(resp) => {
                if !resp.status().is_success() {
                    return Err("Twilio returned an error when fetching ICE servers");
                }

                resp.json::<Value>()
                    .await
                    .map_err(|_| "Failed to parse Twilio ICE server response")
            }
            Err(_) => Err("Error fetching ICE servers from Twilio"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_email() {
        // Valid emails
        assert!(is_email("user@example.com"));
        assert!(is_email("admin@test.org"));
        assert!(is_email("test.user@domain.co.uk"));

        // Invalid emails
        assert!(!is_email("user@example")); // No TLD
        assert!(!is_email("userexample.com")); // No @
        assert!(!is_email("+1234567890")); // Phone number
    }

    #[test]
    fn test_is_phone_number() {
        // Valid E.164 phone numbers
        assert!(is_phone_number("+1234567890"));
        assert!(is_phone_number("+15551234567"));
        assert!(is_phone_number("+44123456789"));

        // Invalid phone numbers
        assert!(!is_phone_number("1234567890")); // Missing +
        assert!(!is_phone_number("+123")); // Too short
        assert!(!is_phone_number("user@example.com")); // Email
    }
}
