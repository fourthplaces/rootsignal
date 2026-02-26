use std::sync::Arc;

use async_graphql::{Context, ErrorExtensions, Guard, Result};

use crate::jwt::Claims;
use rootsignal_common::Config;

/// Optional auth claims attached to the GraphQL context on each request.
/// None if no valid JWT cookie was present.
pub struct AuthContext(pub Option<Claims>);

/// Guard that requires a valid admin JWT.
/// Use with `#[graphql(guard = "AdminGuard")]` on admin queries/mutations.
///
/// Re-verifies admin status against the current `ADMIN_NUMBERS` config on
/// every request, so removing a phone and restarting the server immediately
/// revokes admin access even for unexpired tokens.
pub struct AdminGuard;

impl Guard for AdminGuard {
    async fn check(&self, ctx: &Context<'_>) -> Result<()> {
        let auth = ctx.data_unchecked::<AuthContext>();
        match &auth.0 {
            Some(claims) if claims.is_admin => {
                let config = ctx.data_unchecked::<Arc<Config>>();
                if config.admin_numbers.contains(&claims.phone_number) {
                    Ok(())
                } else {
                    Err("Forbidden: admin access revoked".into())
                }
            }
            Some(_) => Err("Forbidden: admin access required".into()),
            None => Err(async_graphql::Error::new("Unauthenticated")
                .extend_with(|_, e| e.set("code", "UNAUTHENTICATED"))),
        }
    }
}
