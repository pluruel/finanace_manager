use sea_orm_migration::MigratorTrait;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let db = sea_orm::Database::connect(&database_url).await?;
    migration::Migrator::up(&db, None).await?;
    println!("Migrations applied successfully.");
    Ok(())
}
