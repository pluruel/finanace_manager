use clap::{Parser, Subcommand};
use sea_orm::Database;
use sea_orm_migration::MigratorTrait;

/// sea-orm migration CLI for finance-manager.
///
/// Mirrors the subcommands provided by `sea-orm-migration::cli::run_cli`:
/// up, down, status, fresh, refresh, reset, generate.
/// (The `generate` subcommand is intentionally omitted here — new migrations
/// are authored by hand following the project's single-file migration policy.)
#[derive(Parser)]
#[command(version, about = "finance-manager database migration tool")]
struct Cli {
    /// Database URL (falls back to DATABASE_URL env var)
    #[arg(short = 'u', long, env = "DATABASE_URL")]
    database_url: String,

    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Apply all pending migrations (default when no subcommand given)
    Up {
        /// Number of migrations to apply (default: all)
        #[arg(short = 'n', long)]
        num: Option<u32>,
    },
    /// Revert the last N migrations
    Down {
        /// Number of migrations to revert (default: 1)
        #[arg(short = 'n', long, default_value = "1")]
        num: u32,
    },
    /// Show migration status
    Status,
    /// Drop all tables, then re-apply every migration
    Fresh,
    /// Revert all migrations, then re-apply them
    Refresh,
    /// Revert all migrations
    Reset,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    let db = Database::connect(&cli.database_url)
        .await
        .expect("Failed to connect to the database");

    match cli.command {
        None | Some(Cmd::Up { num: None }) => {
            migration::Migrator::up(&db, None)
                .await
                .expect("Migration up failed");
        }
        Some(Cmd::Up { num: Some(n) }) => {
            migration::Migrator::up(&db, Some(n))
                .await
                .expect("Migration up failed");
        }
        Some(Cmd::Down { num }) => {
            migration::Migrator::down(&db, Some(num))
                .await
                .expect("Migration down failed");
        }
        Some(Cmd::Status) => {
            migration::Migrator::status(&db)
                .await
                .expect("Migration status failed");
        }
        Some(Cmd::Fresh) => {
            migration::Migrator::fresh(&db)
                .await
                .expect("Migration fresh failed");
        }
        Some(Cmd::Refresh) => {
            migration::Migrator::refresh(&db)
                .await
                .expect("Migration refresh failed");
        }
        Some(Cmd::Reset) => {
            migration::Migrator::reset(&db)
                .await
                .expect("Migration reset failed");
        }
    }
}
