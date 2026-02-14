use async_graphql::ErrorExtensions;

/// Create a NOT_FOUND GraphQL error.
pub fn not_found(msg: impl std::fmt::Display) -> async_graphql::Error {
    async_graphql::Error::new(format!("not found: {msg}")).extend_with(|_, e| {
        e.set("code", "NOT_FOUND");
    })
}

/// Create an INTERNAL GraphQL error (hides internal details).
pub fn internal(msg: impl std::fmt::Display) -> async_graphql::Error {
    tracing::error!("internal error: {msg}");
    async_graphql::Error::new("internal error").extend_with(|_, e| {
        e.set("code", "INTERNAL");
    })
}

/// Create a BAD_REQUEST GraphQL error.
pub fn bad_request(msg: impl std::fmt::Display) -> async_graphql::Error {
    async_graphql::Error::new(format!("invalid input: {msg}")).extend_with(|_, e| {
        e.set("code", "BAD_REQUEST");
    })
}
