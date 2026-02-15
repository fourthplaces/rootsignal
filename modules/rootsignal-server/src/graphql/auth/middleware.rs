use async_graphql::{Context, Error, Result};

use super::jwt::{Claims, JwtService};

/// Extract admin claims from the GraphQL context.
/// Returns an error if no valid admin JWT is present.
pub fn require_admin(ctx: &Context<'_>) -> Result<Claims> {
    let raw = ctx.data_opt::<Option<Claims>>();
    tracing::info!(has_claims_data = raw.is_some(), "require_admin check");

    let claims = raw
        .and_then(|c| c.as_ref())
        .ok_or_else(|| Error::new("Authentication required"))?;

    tracing::info!(
        phone = %claims.phone_number,
        is_admin = claims.is_admin,
        "Claims extracted"
    );

    if !claims.is_admin {
        return Err(Error::new("Admin access required"));
    }

    Ok(claims.clone())
}

/// Extract claims from an auth_token cookie value.
pub fn extract_claims(jwt_service: &JwtService, cookie_header: Option<&str>) -> Option<Claims> {
    let cookie_header = cookie_header?;

    // Parse the auth_token from the Cookie header
    let token = cookie_header
        .split(';')
        .map(|s| s.trim())
        .find(|s| s.starts_with("auth_token="))
        .and_then(|s| s.strip_prefix("auth_token="))?;

    jwt_service.verify_token(token).ok()
}
