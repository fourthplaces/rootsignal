//! Runs pending SQLx migrations against the database.
//!
//! Migrations are embedded at compile time, so no migration files
//! are needed at runtime. Used as a Docker entrypoint step before
//! starting the server.

use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    println!("Running database migrations...");

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await?;

    sqlx::migrate!("../../migrations").run(&pool).await?;

    println!("Migrations completed successfully.");

    Ok(())
}
