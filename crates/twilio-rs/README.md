# twilio-rs

A Rust client library for Twilio's Verify API and ICE server provisioning.

## Features

- ✅ **OTP Verification** - Send OTP codes via SMS or Email
- ✅ **Automatic Channel Detection** - Detects email vs phone number format
- ✅ **Input Validation** - Validates E.164 phone format and email addresses
- ✅ **Enhanced Error Messages** - Helpful error messages for common issues
- ✅ **ICE Server Provisioning** - For WebRTC applications

## Installation

```toml
[dependencies]
twilio = { path = "../twilio-rs" }
```

## Usage

### Configuration

```rust
use twilio::{TwilioService, TwilioOptions};

let service = TwilioService::new(TwilioOptions {
    account_sid: "your_account_sid".to_string(),
    auth_token: "your_auth_token".to_string(),
    service_id: "your_verify_service_id".to_string(),
});
```

### Send OTP

```rust
// Send OTP via SMS
let response = service.send_otp("+1234567890").await?;

// Send OTP via email
let response = service.send_otp("user@example.com").await?;
```

The channel (SMS or email) is automatically determined based on the recipient format.

### Verify OTP

```rust
let result = service.verify_otp("+1234567890", "123456").await;
match result {
    Ok(()) => println!("Verification successful"),
    Err(e) => println!("Verification failed: {}", e),
}
```

### Fetch ICE Servers

For WebRTC TURN/STUN server credentials:

```rust
let ice_servers = service.fetch_ice_servers().await?;
// Returns JSON with ICE server configuration
```

## Environment Variables

| Variable                      | Description                          | Format      |
| ----------------------------- | ------------------------------------ | ----------- |
| `TWILIO_ACCOUNT_SID`          | Your Twilio Account SID              | `ACxxxxxx`  |
| `TWILIO_AUTH_TOKEN`           | Your Twilio Auth Token               | String      |
| `TWILIO_VERIFY_SERVICE_SID`   | Your Twilio Verify Service SID       | `VAxxxxxx`  |

⚠️ **Important:** The Verify Service SID starts with `VA`, not `AC`. Don't confuse it with your Account SID!

## API Reference

### TwilioService

| Method                        | Description                |
| ----------------------------- | -------------------------- |
| `send_otp(recipient)`         | Send OTP to phone or email |
| `verify_otp(recipient, code)` | Verify OTP code            |
| `fetch_ice_servers()`         | Get TURN/STUN credentials  |

## Format Requirements

### Phone Numbers
Must be in **E.164 format**:
- ✅ `+15551234567` (US)
- ✅ `+442012345678` (UK)
- ❌ `5551234567` (missing `+`)
- ❌ `(555) 123-4567` (not E.164)

### Email Addresses
Standard email format with `@` and domain:
- ✅ `user@example.com`
- ✅ `admin@company.co.uk`
- ❌ `user@example` (no TLD)
- ❌ `userexample.com` (no `@`)

## Troubleshooting

### Error 60200: "Invalid parameter" with email

**Cause:** Email channel not enabled on your Twilio Verify Service.

**Solution:**
1. Go to [Twilio Verify Services](https://console.twilio.com/us1/develop/verify/services)
2. Click on your Verify Service
3. Select **"Email"** in the left sidebar
4. Click **"Enable Email Channel"**
5. Configure SendGrid integration if prompted
6. Save settings

### Error 60202/60203: "Too many attempts"

**Cause:** Twilio rate-limits verification requests.

**Solution:** Wait before retrying. The rate limit resets after a few minutes.

### Wrong Service SID

**Problem:** Using Account SID (`ACxxxxxx`) instead of Verify Service SID (`VAxxxxxx`).

**Solution:**
```rust
// ❌ Wrong
service_id: "ACxxxxxxxx"  // Account SID

// ✅ Correct
service_id: "VAxxxxxxxx"  // Verify Service SID
```

Find your Verify Service SID at: https://console.twilio.com/us1/develop/verify/services

### Invalid recipient format

**Cause:** Phone number not in E.164 format or invalid email.

**Solution:**
- Phones must start with `+` followed by country code
- Emails must contain `@` and a domain

## Dependencies

- `reqwest` - HTTP client
- `serde` / `serde_json` - JSON serialization

## License

MIT
