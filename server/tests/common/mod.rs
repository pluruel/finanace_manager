//! Test helper that replaces `#[sqlx::test(migrations = "./migrations")]`.
//!
//! Each test gets a fresh ephemeral database. The DB name is randomized per test
//! and dropped on the `Drop` of `TestDb`.

#![allow(dead_code)]

use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, Statement};
use std::time::Duration;
use uuid::Uuid;

const ADMIN_DB_ENV: &str = "DATABASE_URL";

pub struct TestDb {
    pub db: std::sync::Arc<DatabaseConnection>,
    pub pool: sqlx::PgPool,
    pub url: String,
    db_name: String,
    admin_url: String,
}

impl TestDb {
    pub async fn new() -> Self {
        let admin_url = std::env::var(ADMIN_DB_ENV)
            .expect("DATABASE_URL must be set for integration tests");
        let db_name = format!("fm_test_{}", Uuid::new_v4().simple());

        let admin = Database::connect(&admin_url)
            .await
            .expect("admin DB connect failed");
        admin
            .execute(Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                format!(r#"CREATE DATABASE "{db_name}""#),
            ))
            .await
            .expect("CREATE DATABASE failed");
        admin.close().await.ok();

        let test_url = replace_db_name(&admin_url, &db_name);
        let mut opts = ConnectOptions::new(&test_url);
        opts.max_connections(5)
            .connect_timeout(Duration::from_secs(8));
        let db = Database::connect(opts)
            .await
            .expect("test DB connect failed");

        Migrator::up(&db, None)
            .await
            .expect("Migrator::up failed");

        let pool = finance_manager::db::pool_of(&db).clone();
        let db = std::sync::Arc::new(db);

        Self {
            db,
            pool,
            url: test_url,
            db_name,
            admin_url,
        }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let admin_url = self.admin_url.clone();
        let db_name = self.db_name.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                if let Ok(admin) = Database::connect(&admin_url).await {
                    let _ = admin
                        .execute(Statement::from_string(
                            sea_orm::DatabaseBackend::Postgres,
                            format!(r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#),
                        ))
                        .await;
                }
            });
        })
        .join()
        .ok();
    }
}

fn replace_db_name(url: &str, new_name: &str) -> String {
    let (head, _old) = url.rsplit_once('/').expect("url missing /db");
    format!("{head}/{new_name}")
}
