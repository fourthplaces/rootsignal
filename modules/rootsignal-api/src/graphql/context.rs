use async_graphql::{Context, ErrorExtensions, Guard, Result};

use crate::jwt::Claims;

/// Optional auth claims attached to the GraphQL context on each request.
/// None if no valid JWT cookie was present.
pub struct AuthContext(pub Option<Claims>);

/// Guard that requires a valid admin JWT.
/// Use with `#[graphql(guard = "AdminGuard")]` on admin queries/mutations.
pub struct AdminGuard;

impl Guard for AdminGuard {
    async fn check(&self, ctx: &Context<'_>) -> Result<()> {
        let auth = ctx.data_unchecked::<AuthContext>();
        match &auth.0 {
            Some(claims) if claims.is_admin => Ok(()),
            Some(_) => Err("Forbidden: admin access required".into()),
            None => Err(async_graphql::Error::new("Unauthenticated")
                .extend_with(|_, e| e.set("code", "UNAUTHENTICATED"))),
        }
    }
}
