use anyhow::Result;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use std::time::Duration;

pub async fn create_db(database_url: &str) -> Result<DatabaseConnection> {
    let mut opt = ConnectOptions::new(database_url);
    opt.max_connections(10)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(60 * 5))
        .max_lifetime(Duration::from_secs(60 * 30));
    let db = Database::connect(opt).await?;
    Ok(db)
}

/// Temporary bridge for modules still using sqlx directly (T5–T7 will remove the callers,
/// T8 will remove this function).
pub fn pool_of(db: &DatabaseConnection) -> &sqlx::PgPool {
    db.get_postgres_connection_pool()
}
