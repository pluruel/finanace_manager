# SeaORM Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace direct sqlx usage in `server/` with SeaORM 1.x in a single PR while keeping all 87 backend tests and 124 frontend tests green and producing byte-equal output for the golden Excel import.

**Architecture:** Add a `migration/` workspace member crate that holds a single in-place-edited migration file (matching the existing rewrite-in-place SQL policy). Generate `server/src/entity/` from the migrated DB and check it in. Replace `PgPool` everywhere with `sea_orm::DatabaseConnection`. Use ORM-first APIs for CRUD/JOIN, and `Statement::from_sql_and_values` + `FromQueryResult` for views and complex aggregates. Replace the `#[sqlx::test]` macro with a hand-rolled ephemeral DB helper that calls `Migrator::up`.

**Tech Stack:** Rust 1.75+, axum 0.7, sea-orm 1.x, sea-orm-migration 1.x, postgres 17, rust_decimal, uuid, chrono.

**Reference spec:** `docs/superpowers/specs/2026-05-10-seaorm-migration-design.md`.

---

## File Structure

```
server/
  Cargo.toml                          # MODIFY — drop sqlx, add sea-orm + sea-orm-migration; declare workspace
  migrations/                         # DELETE (after T2 ports DDL)
    001_init.sql
  migration/                          # NEW workspace member crate
    Cargo.toml                        # NEW
    src/lib.rs                        # NEW — Migrator { migrations() }
    src/m20260510_000001_init.rs      # NEW — single in-place migration with up()/down()
  src/
    db.rs                             # MODIFY — DatabaseConnection
    main.rs                           # MODIFY — Migrator::up + AppState::db
    bin/test_import.rs                # MODIFY — DatabaseConnection
    api/mod.rs                        # MODIFY — router state type DatabaseConnection
    api/categories.rs                 # MODIFY — ORM-first CRUD
    api/transactions.rs               # MODIFY — ORM-first
    api/products.rs                   # MODIFY — ORM-first
    api/import.rs                     # MODIFY — ORM-first batch creation
    api/aliases.rs                    # MODIFY — ORM-first CRUD + review queue
    api/settlement.rs                 # MODIFY — raw SQL via FromQueryResult
    api/summary.rs                    # MODIFY — raw SQL via FromQueryResult
    api/income.rs                     # MODIFY — raw SQL via FromQueryResult
    api/merchant_stats.rs             # MODIFY — raw SQL via FromQueryResult
    api/price.rs                      # MODIFY — raw SQL via FromQueryResult
    api/export.rs                     # MODIFY — raw SQL via FromQueryResult
    import/pipeline.rs                # MODIFY — DatabaseTransaction + ActiveModel + OnConflict
    entity/mod.rs                     # NEW — auto-generated, re-exports
    entity/ledger_actor.rs            # NEW — auto-generated
    entity/category.rs                # NEW — auto-generated
    entity/merchant.rs                # NEW — auto-generated
    entity/product.rs                 # NEW — auto-generated
    entity/payment_method.rs          # NEW — auto-generated
    entity/alias.rs                   # NEW — auto-generated
    entity/import_batch.rs            # NEW — auto-generated
    entity/transaction_raw.rs         # NEW — auto-generated
    entity/transaction.rs             # NEW — auto-generated
    entity/prelude.rs                 # NEW — auto-generated
  tests/
    common/mod.rs                     # NEW — TestDb helper replacing #[sqlx::test]
    test_*.rs                         # MODIFY — switch macro + pool type
.sqlx/                                # DELETE in T8
sqlx.sh                               # DELETE in T8 (if present)
CLAUDE.md                             # MODIFY in T8
```

The plan keeps the project in a working build at every task boundary except T1→T3 (which intentionally land as one bisect range; build is allowed to be red between those internal commits, but the PR must be green at the end of every numbered Task).

---

## Task 1 — Workspace + dependency scaffolding

**Files:**
- Modify: `server/Cargo.toml`
- Create: `Cargo.toml` (workspace root, if not already a workspace)
- Create: `server/migration/Cargo.toml`
- Create: `server/migration/src/lib.rs` (stub)

- [ ] **Step 1: Confirm workspace state**

```bash
test -f /Users/juno/dev/finance_mananger/Cargo.toml && cat /Users/juno/dev/finance_mananger/Cargo.toml
```

Expected: a workspace `Cargo.toml` listing `server` as a member, OR no top-level `Cargo.toml` (single-crate layout). Note which case applies — Step 2 differs.

- [ ] **Step 2a (single-crate case): Create workspace `Cargo.toml`**

If no top-level `Cargo.toml` exists, create one at `/Users/juno/dev/finance_mananger/Cargo.toml`:

```toml
[workspace]
members = ["server", "server/migration"]
resolver = "2"
```

- [ ] **Step 2b (existing workspace case): Add `server/migration` member**

Edit the top-level `Cargo.toml` to add `"server/migration"` to the `members` array.

- [ ] **Step 3: Create `server/migration/Cargo.toml`**

```toml
[package]
name = "migration"
version = "0.1.0"
edition = "2021"
publish = false

[lib]
name = "migration"
path = "src/lib.rs"

[dependencies]
async-trait = "0.1"
sea-orm-migration = { version = "1", features = [
    "runtime-tokio-rustls",
    "sqlx-postgres",
] }
```

- [ ] **Step 4: Create `server/migration/src/lib.rs` stub**

```rust
pub use sea_orm_migration::prelude::*;

mod m20260510_000001_init;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20260510_000001_init::Migration)]
    }
}
```

- [ ] **Step 5: Create empty migration stub at `server/migration/src/m20260510_000001_init.rs`**

```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // T2 will fill this in.
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
```

- [ ] **Step 6: Modify `server/Cargo.toml` dependencies**

Replace the `# Database` block (lines 25–35 currently `sqlx = { version = "0.7", ... }`) with:

```toml
# Database
sea-orm = { version = "1", features = [
    "runtime-tokio-rustls",
    "sqlx-postgres",
    "with-uuid",
    "with-chrono",
    "with-rust_decimal",
    "with-json",
    "macros",
    "debug-print",
] }
```

Add to `[dependencies]`:

```toml
migration = { path = "migration" }
```

Add to `[dev-dependencies]`:

```toml
sea-orm = { version = "1", features = ["mock"] }   # for unit-test mocks if needed later
tokio = { version = "1", features = ["full", "test-util"] }
```

(Do NOT add `sqlx` back — direct sqlx usage is being deleted.)

- [ ] **Step 7: Verify the workspace compiles (migration crate only)**

```bash
cd /Users/juno/dev/finance_mananger && cargo build -p migration
```

Expected: builds cleanly. The `server` crate will be broken (still imports sqlx) — that is OK at this checkpoint. We will fix it in T2–T4.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml server/Cargo.toml server/migration/
git commit -m "feat(migration): scaffold sea-orm-migration crate and workspace wiring"
```

---

## Task 2 — Port DDL into the migration

**Files:**
- Modify: `server/migration/src/m20260510_000001_init.rs`
- Delete: `server/migrations/001_init.sql`

This task ports every DDL statement from `001_init.sql` into the SeaORM migration. Tables/PKs/FKs/indexes/CHECKs use the `SchemaManager` DSL. The `v_monthly_settlement` view and partial unique indexes (which the DSL doesn't express cleanly) are emitted with `execute_unprepared`.

- [ ] **Step 1: Read the source DDL**

```bash
cat /Users/juno/dev/finance_mananger/server/migrations/001_init.sql
```

Confirm the file matches the schema defined inline in this task. If anything differs from the snippets below, the implementer must port the actual file content (not these snippets).

- [ ] **Step 2: Define iden enums for every table**

Replace `m20260510_000001_init.rs` body with the iden block first. Each table gets a `#[derive(DeriveIden)]` enum whose first variant is `Table` and remaining variants are columns.

```rust
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
pub enum LedgerActors { Table, Id, OwnerId, Name }

#[derive(DeriveIden)]
pub enum Categories {
    Table, Id, OwnerId, ParentId, Name, Kind, ReviewState,
}

#[derive(DeriveIden)]
pub enum Merchants { Table, Id, OwnerId, Name, ReviewState }

#[derive(DeriveIden)]
pub enum Products {
    Table, Id, OwnerId, MerchantId, Name, ReviewState,
}

#[derive(DeriveIden)]
pub enum PaymentMethods {
    Table, Id, OwnerId, Name, ActorId, ReviewState,
}

#[derive(DeriveIden)]
pub enum Aliases {
    Table, Id, OwnerId, Scope, RawText, NormKey, TargetId,
}

#[derive(DeriveIden)]
pub enum ImportBatches {
    Table, Id, OwnerId, FileName, FileHash, Year, Month, RowCount, ImportedAt,
}

#[derive(DeriveIden)]
pub enum TransactionsRaw {
    Table, Id, OwnerId, ImportBatchId, RowIndex, GroupId, IsGroupHeader,
    OccurredOn, RawDateSerial, MerchantText, ActorText, CategoryText,
    TotalAmount, Memo, UnitPrice, Quantity, LineAmount,
    PaymentText, EvidenceText, Extras,
}

#[derive(DeriveIden)]
pub enum Transactions {
    Table, Id, OwnerId, RawId, GroupId, OccurredOn,
    MerchantId, ActorId, CategoryId, ProductId, PaymentMethodId,
    Amount, UnitPrice, Quantity, Memo,
}
```

- [ ] **Step 3: Implement `up()` — pgcrypto + ledger_actors + categories**

Append below the iden enums:

```rust
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let conn = m.get_connection();

        // pgcrypto for gen_random_uuid()
        conn.execute_unprepared("CREATE EXTENSION IF NOT EXISTS pgcrypto").await?;

        // ledger_actors
        m.create_table(
            Table::create()
                .table(LedgerActors::Table)
                .col(ColumnDef::new(LedgerActors::Id).uuid().not_null().primary_key()
                    .extra("DEFAULT gen_random_uuid()".to_owned()))
                .col(ColumnDef::new(LedgerActors::OwnerId).uuid().not_null())
                .col(ColumnDef::new(LedgerActors::Name).text().not_null())
                .index(Index::create()
                    .unique()
                    .name("ledger_actors_owner_name_uniq")
                    .col(LedgerActors::OwnerId)
                    .col(LedgerActors::Name))
                .to_owned()
        ).await?;

        // categories
        m.create_table(
            Table::create()
                .table(Categories::Table)
                .col(ColumnDef::new(Categories::Id).uuid().not_null().primary_key()
                    .extra("DEFAULT gen_random_uuid()".to_owned()))
                .col(ColumnDef::new(Categories::OwnerId).uuid().not_null())
                .col(ColumnDef::new(Categories::ParentId).uuid())
                .col(ColumnDef::new(Categories::Name).text().not_null())
                .col(ColumnDef::new(Categories::Kind).text().not_null()
                    .check(Expr::col(Categories::Kind).is_in(["income", "expense"])))
                .col(ColumnDef::new(Categories::ReviewState).text().not_null()
                    .default("pending")
                    .check(Expr::col(Categories::ReviewState).is_in(["pending", "confirmed"])))
                .foreign_key(ForeignKey::create()
                    .name("categories_parent_fk")
                    .from(Categories::Table, Categories::ParentId)
                    .to(Categories::Table, Categories::Id))
                .to_owned()
        ).await?;
```

- [ ] **Step 4: Implement `up()` — merchants + products + payment_methods**

```rust
        // merchants
        m.create_table(
            Table::create()
                .table(Merchants::Table)
                .col(ColumnDef::new(Merchants::Id).uuid().not_null().primary_key()
                    .extra("DEFAULT gen_random_uuid()".to_owned()))
                .col(ColumnDef::new(Merchants::OwnerId).uuid().not_null())
                .col(ColumnDef::new(Merchants::Name).text().not_null())
                .col(ColumnDef::new(Merchants::ReviewState).text().not_null().default("pending"))
                .index(Index::create()
                    .unique()
                    .name("merchants_owner_name_uniq")
                    .col(Merchants::OwnerId)
                    .col(Merchants::Name))
                .to_owned()
        ).await?;

        // products
        m.create_table(
            Table::create()
                .table(Products::Table)
                .col(ColumnDef::new(Products::Id).uuid().not_null().primary_key()
                    .extra("DEFAULT gen_random_uuid()".to_owned()))
                .col(ColumnDef::new(Products::OwnerId).uuid().not_null())
                .col(ColumnDef::new(Products::MerchantId).uuid())
                .col(ColumnDef::new(Products::Name).text().not_null())
                .col(ColumnDef::new(Products::ReviewState).text().not_null()
                    .default("pending")
                    .check(Expr::col(Products::ReviewState).is_in(["pending", "confirmed"])))
                .foreign_key(ForeignKey::create()
                    .name("products_merchant_fk")
                    .from(Products::Table, Products::MerchantId)
                    .to(Merchants::Table, Merchants::Id))
                .to_owned()
        ).await?;

        // payment_methods
        m.create_table(
            Table::create()
                .table(PaymentMethods::Table)
                .col(ColumnDef::new(PaymentMethods::Id).uuid().not_null().primary_key()
                    .extra("DEFAULT gen_random_uuid()".to_owned()))
                .col(ColumnDef::new(PaymentMethods::OwnerId).uuid().not_null())
                .col(ColumnDef::new(PaymentMethods::Name).text().not_null())
                .col(ColumnDef::new(PaymentMethods::ActorId).uuid())
                .col(ColumnDef::new(PaymentMethods::ReviewState).text().not_null()
                    .default("pending")
                    .check(Expr::col(PaymentMethods::ReviewState).is_in(["pending", "confirmed"])))
                .foreign_key(ForeignKey::create()
                    .name("payment_methods_actor_fk")
                    .from(PaymentMethods::Table, PaymentMethods::ActorId)
                    .to(LedgerActors::Table, LedgerActors::Id))
                .index(Index::create()
                    .unique()
                    .name("payment_methods_owner_name_uniq")
                    .col(PaymentMethods::OwnerId)
                    .col(PaymentMethods::Name))
                .to_owned()
        ).await?;
```

- [ ] **Step 5: Implement `up()` — aliases + partial unique indexes (raw SQL)**

```rust
        // aliases
        m.create_table(
            Table::create()
                .table(Aliases::Table)
                .col(ColumnDef::new(Aliases::Id).uuid().not_null().primary_key()
                    .extra("DEFAULT gen_random_uuid()".to_owned()))
                .col(ColumnDef::new(Aliases::OwnerId).uuid().not_null())
                .col(ColumnDef::new(Aliases::Scope).text().not_null()
                    .check(Expr::col(Aliases::Scope).is_in(
                        ["category", "merchant", "payment_method", "actor", "product"])))
                .col(ColumnDef::new(Aliases::RawText).text().not_null())
                .col(ColumnDef::new(Aliases::NormKey).text().not_null())
                .col(ColumnDef::new(Aliases::TargetId).uuid().not_null())
                .index(Index::create()
                    .unique()
                    .name("aliases_owner_scope_norm_uniq")
                    .col(Aliases::OwnerId)
                    .col(Aliases::Scope)
                    .col(Aliases::NormKey))
                .to_owned()
        ).await?;

        m.create_index(
            Index::create()
                .name("aliases_lookup_idx")
                .table(Aliases::Table)
                .col(Aliases::OwnerId)
                .col(Aliases::Scope)
                .col(Aliases::NormKey)
                .to_owned()
        ).await?;

        // Partial unique indexes — DSL does not support WHERE clauses cleanly, so raw SQL.
        conn.execute_unprepared(r#"
            CREATE UNIQUE INDEX categories_owner_name_root_uniq
              ON categories (owner_id, name) WHERE parent_id IS NULL;
            CREATE UNIQUE INDEX categories_owner_parent_name_uniq
              ON categories (owner_id, parent_id, name) WHERE parent_id IS NOT NULL;
            CREATE UNIQUE INDEX products_owner_merchant_name_uniq
              ON products (owner_id, merchant_id, name) WHERE merchant_id IS NOT NULL;
            CREATE UNIQUE INDEX products_owner_name_no_merchant_uniq
              ON products (owner_id, name) WHERE merchant_id IS NULL;
        "#).await?;
```

- [ ] **Step 6: Implement `up()` — import_batches + transactions_raw + transactions + indexes**

```rust
        // import_batches
        m.create_table(
            Table::create()
                .table(ImportBatches::Table)
                .col(ColumnDef::new(ImportBatches::Id).uuid().not_null().primary_key()
                    .extra("DEFAULT gen_random_uuid()".to_owned()))
                .col(ColumnDef::new(ImportBatches::OwnerId).uuid().not_null())
                .col(ColumnDef::new(ImportBatches::FileName).text().not_null())
                .col(ColumnDef::new(ImportBatches::FileHash).binary().not_null())
                .col(ColumnDef::new(ImportBatches::Year).integer().not_null())
                .col(ColumnDef::new(ImportBatches::Month).integer().not_null())
                .col(ColumnDef::new(ImportBatches::RowCount).integer().not_null())
                .col(ColumnDef::new(ImportBatches::ImportedAt).timestamp_with_time_zone()
                    .not_null().default(Expr::current_timestamp()))
                .index(Index::create()
                    .unique()
                    .name("import_batches_owner_hash_uniq")
                    .col(ImportBatches::OwnerId)
                    .col(ImportBatches::FileHash))
                .to_owned()
        ).await?;

        // transactions_raw
        m.create_table(
            Table::create()
                .table(TransactionsRaw::Table)
                .col(ColumnDef::new(TransactionsRaw::Id).uuid().not_null().primary_key()
                    .extra("DEFAULT gen_random_uuid()".to_owned()))
                .col(ColumnDef::new(TransactionsRaw::OwnerId).uuid().not_null())
                .col(ColumnDef::new(TransactionsRaw::ImportBatchId).uuid().not_null())
                .col(ColumnDef::new(TransactionsRaw::RowIndex).integer().not_null())
                .col(ColumnDef::new(TransactionsRaw::GroupId).uuid().not_null())
                .col(ColumnDef::new(TransactionsRaw::IsGroupHeader).boolean().not_null())
                .col(ColumnDef::new(TransactionsRaw::OccurredOn).date())
                .col(ColumnDef::new(TransactionsRaw::RawDateSerial).double())
                .col(ColumnDef::new(TransactionsRaw::MerchantText).text())
                .col(ColumnDef::new(TransactionsRaw::ActorText).text())
                .col(ColumnDef::new(TransactionsRaw::CategoryText).text())
                .col(ColumnDef::new(TransactionsRaw::TotalAmount).decimal_len(15, 2))
                .col(ColumnDef::new(TransactionsRaw::Memo).text())
                .col(ColumnDef::new(TransactionsRaw::UnitPrice).decimal_len(15, 4))
                .col(ColumnDef::new(TransactionsRaw::Quantity).decimal_len(15, 4))
                .col(ColumnDef::new(TransactionsRaw::LineAmount).decimal_len(15, 2))
                .col(ColumnDef::new(TransactionsRaw::PaymentText).text())
                .col(ColumnDef::new(TransactionsRaw::EvidenceText).text())
                .col(ColumnDef::new(TransactionsRaw::Extras).json_binary())
                .foreign_key(ForeignKey::create()
                    .name("transactions_raw_batch_fk")
                    .from(TransactionsRaw::Table, TransactionsRaw::ImportBatchId)
                    .to(ImportBatches::Table, ImportBatches::Id)
                    .on_delete(ForeignKeyAction::Cascade))
                .to_owned()
        ).await?;

        m.create_index(Index::create()
            .name("transactions_raw_date_idx")
            .table(TransactionsRaw::Table)
            .col(TransactionsRaw::OwnerId)
            .col(TransactionsRaw::OccurredOn)
            .to_owned()).await?;

        m.create_index(Index::create()
            .name("transactions_raw_group_idx")
            .table(TransactionsRaw::Table)
            .col(TransactionsRaw::OwnerId)
            .col(TransactionsRaw::GroupId)
            .to_owned()).await?;

        // transactions
        m.create_table(
            Table::create()
                .table(Transactions::Table)
                .col(ColumnDef::new(Transactions::Id).uuid().not_null().primary_key()
                    .extra("DEFAULT gen_random_uuid()".to_owned()))
                .col(ColumnDef::new(Transactions::OwnerId).uuid().not_null())
                .col(ColumnDef::new(Transactions::RawId).uuid().not_null())
                .col(ColumnDef::new(Transactions::GroupId).uuid().not_null())
                .col(ColumnDef::new(Transactions::OccurredOn).date().not_null())
                .col(ColumnDef::new(Transactions::MerchantId).uuid())
                .col(ColumnDef::new(Transactions::ActorId).uuid())
                .col(ColumnDef::new(Transactions::CategoryId).uuid())
                .col(ColumnDef::new(Transactions::ProductId).uuid())
                .col(ColumnDef::new(Transactions::PaymentMethodId).uuid())
                .col(ColumnDef::new(Transactions::Amount).decimal_len(15, 2).not_null())
                .col(ColumnDef::new(Transactions::UnitPrice).decimal_len(15, 4))
                .col(ColumnDef::new(Transactions::Quantity).decimal_len(15, 4))
                .col(ColumnDef::new(Transactions::Memo).text())
                .foreign_key(ForeignKey::create().name("transactions_raw_fk")
                    .from(Transactions::Table, Transactions::RawId)
                    .to(TransactionsRaw::Table, TransactionsRaw::Id)
                    .on_delete(ForeignKeyAction::Cascade))
                .foreign_key(ForeignKey::create().name("transactions_merchant_fk")
                    .from(Transactions::Table, Transactions::MerchantId)
                    .to(Merchants::Table, Merchants::Id))
                .foreign_key(ForeignKey::create().name("transactions_actor_fk")
                    .from(Transactions::Table, Transactions::ActorId)
                    .to(LedgerActors::Table, LedgerActors::Id))
                .foreign_key(ForeignKey::create().name("transactions_category_fk")
                    .from(Transactions::Table, Transactions::CategoryId)
                    .to(Categories::Table, Categories::Id))
                .foreign_key(ForeignKey::create().name("transactions_product_fk")
                    .from(Transactions::Table, Transactions::ProductId)
                    .to(Products::Table, Products::Id))
                .foreign_key(ForeignKey::create().name("transactions_payment_method_fk")
                    .from(Transactions::Table, Transactions::PaymentMethodId)
                    .to(PaymentMethods::Table, PaymentMethods::Id))
                .to_owned()
        ).await?;

        for (name, cols) in [
            ("transactions_date_idx", "owner_id, occurred_on DESC"),
            ("transactions_category_idx", "owner_id, category_id, occurred_on"),
            ("transactions_merchant_idx", "owner_id, merchant_id, occurred_on"),
            ("transactions_product_idx", "owner_id, product_id, occurred_on"),
            ("transactions_group_idx", "owner_id, group_id"),
        ] {
            conn.execute_unprepared(&format!(
                "CREATE INDEX {name} ON transactions ({cols})"
            )).await?;
        }
```

- [ ] **Step 7: Implement `up()` — v_monthly_settlement view (raw SQL)**

```rust
        conn.execute_unprepared(r#"
            CREATE VIEW v_monthly_settlement AS
            SELECT
              t.owner_id,
              date_trunc('month', t.occurred_on)::date AS month,
              COALESCE(-SUM(t.amount) FILTER (
                WHERE actor.name = '공동' AND c.kind = 'expense' AND c.name <> '차감'
              ), 0) AS recognized_expense,
              COALESCE(-SUM(t.amount) FILTER (WHERE c.name = '차감'), 0) AS deducted_amount,
              COALESCE(-SUM(t.amount) FILTER (
                WHERE actor.name = '공동' AND c.kind = 'expense' AND c.name <> '차감'
              ), 0)
              - COALESCE(-SUM(t.amount) FILTER (WHERE c.name = '차감'), 0) AS settlement_input
            FROM transactions t
            JOIN categories c        ON c.id     = t.category_id
            JOIN ledger_actors actor ON actor.id = t.actor_id
            GROUP BY t.owner_id, date_trunc('month', t.occurred_on);
        "#).await?;

        Ok(())
    }
```

- [ ] **Step 8: Implement `down()` — drop in reverse order**

```rust
    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let conn = m.get_connection();
        conn.execute_unprepared("DROP VIEW IF EXISTS v_monthly_settlement").await?;
        m.drop_table(Table::drop().table(Transactions::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(TransactionsRaw::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(ImportBatches::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(Aliases::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(PaymentMethods::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(Products::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(Merchants::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(Categories::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(LedgerActors::Table).to_owned()).await?;
        Ok(())
    }
}
```

- [ ] **Step 9: Smoke-test the migration against a scratch DB**

```bash
export TEST_DB="postgres://app:app@localhost:5432/fm_seaorm_smoke"
psql "postgres://app:app@localhost:5432/postgres" -c 'DROP DATABASE IF EXISTS fm_seaorm_smoke;'
psql "postgres://app:app@localhost:5432/postgres" -c 'CREATE DATABASE fm_seaorm_smoke;'

cd /Users/juno/dev/finance_mananger/server/migration
cargo run --bin migration -- up -u "$TEST_DB" 2>&1 | tail -20 || true
```

The `migration` crate as written has no binary, so the above will fail. Add a `migration/src/main.rs` for one-off CLI use:

```rust
use sea_orm_migration::cli;

#[async_std::main]
async fn main() {
    cli::run_cli(migration::Migrator).await;
}
```

…and update `migration/Cargo.toml`:

```toml
[[bin]]
name = "migration"
path = "src/main.rs"

[dependencies]
async-std = { version = "1", features = ["attributes", "tokio1"] }
```

Re-run the smoke test. Expected: prints `Migration 'm20260510_000001_init' has been applied`. Then verify schema:

```bash
psql "$TEST_DB" -c '\dt' | grep -E '(ledger_actors|categories|merchants|products|payment_methods|aliases|import_batches|transactions_raw|transactions)'
psql "$TEST_DB" -c '\d v_monthly_settlement'
```

Expected: 9 tables present, view definition prints.

- [ ] **Step 10: Delete the old SQL migration**

```bash
rm /Users/juno/dev/finance_mananger/server/migrations/001_init.sql
rmdir /Users/juno/dev/finance_mananger/server/migrations 2>/dev/null || true
```

- [ ] **Step 11: Commit**

```bash
git add server/migration/ server/migrations 2>/dev/null
git rm -f server/migrations/001_init.sql 2>/dev/null || true
git commit -m "feat(migration): port DDL from 001_init.sql to sea-orm migration"
```

---

## Task 3 — Generate `entity/` from the migrated DB

**Files:**
- Create: `server/src/entity/*.rs` (auto-generated)

- [ ] **Step 1: Install `sea-orm-cli` if absent**

```bash
which sea-orm-cli || cargo install sea-orm-cli@1
```

- [ ] **Step 2: Generate entities from the smoke DB**

```bash
cd /Users/juno/dev/finance_mananger/server
sea-orm-cli generate entity \
  -u "postgres://app:app@localhost:5432/fm_seaorm_smoke" \
  -o src/entity \
  --with-serde both \
  --serde-skip-deserializing-primary-key
```

Expected: `src/entity/{mod.rs, prelude.rs, ledger_actors.rs, categories.rs, merchants.rs, products.rs, payment_methods.rs, aliases.rs, import_batches.rs, transactions_raw.rs, transactions.rs}` written.

- [ ] **Step 3: Wire `entity` into `lib.rs`**

Modify `server/src/lib.rs` to add `pub mod entity;` (alphabetical order):

```rust
pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod domain;
pub mod entity;
pub mod error;
pub mod import;
```

- [ ] **Step 4: Inspect each generated entity file**

Open each file under `src/entity/`. Verify:
- `Model` struct has all expected columns with correct types (Uuid, Decimal, NaiveDate, etc.)
- `enum Relation` has variants for every FK in the DDL
- `categories::Relation` has a `SelfRef` variant for `parent_id` (sea-orm-cli usually emits this; if missing, add it manually following the standard pattern below)

If `categories.rs` is missing the self-relation, append:

```rust
impl Related<Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SelfRef.def()
    }
}
```

- [ ] **Step 5: Verify the entity layer compiles standalone**

```bash
cd /Users/juno/dev/finance_mananger
cargo check -p finance-manager 2>&1 | head -40
```

The crate-wide check will likely still fail because API modules still call sqlx. Filter:

```bash
cargo check -p finance-manager 2>&1 | grep -E "^error\[" | grep "src/entity" | head -20
```

Expected: zero entity-related errors. Errors elsewhere (api/*, import/*, db.rs) are expected and addressed by T4–T7.

- [ ] **Step 6: Commit**

```bash
git add server/src/entity/ server/src/lib.rs
git commit -m "feat(entity): generate sea-orm entities from migrated schema"
```

---

## Task 4 — Replace `PgPool` with `DatabaseConnection` and rebuild test infra

**Files:**
- Modify: `server/src/db.rs`
- Modify: `server/src/main.rs`
- Modify: `server/src/bin/test_import.rs`
- Modify: `server/src/api/mod.rs`
- Create: `server/tests/common/mod.rs`

This task replaces the connection plumbing only. Per-module SQL inside `api/*.rs` and `import/pipeline.rs` still references sqlx — that is intentional; they will be ported in T5–T7. To allow the crate to compile at the boundary of this task, we make `AppState` carry a `DatabaseConnection` and downcast to `&PgPool` via SeaORM's `get_postgres_connection_pool()` in the still-sqlx modules. This bridge is removed in T5–T7.

- [ ] **Step 1: Rewrite `server/src/db.rs`**

```rust
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
```

- [ ] **Step 2: Rewrite the migration call in `server/src/main.rs`**

Replace the `db::create_pool` + `sqlx::migrate!` block (currently lines 33–43) with:

```rust
    let db = db::create_db(&config.database_url).await?;
    tracing::info!("Database connected");

    migration::Migrator::up(&db, None).await?;
    tracing::info!("Migrations applied");

    let db = std::sync::Arc::new(db);
```

…and change `let app = api::router(pool, jwks)` → `let app = api::router(db, jwks)`.

Add `use migration::MigratorTrait;` to the top of `main.rs`.

- [ ] **Step 3: Update `server/src/api/mod.rs` router signature**

Change the import line from `use sqlx::PgPool;` to `use sea_orm::DatabaseConnection;` and the function signature:

```rust
pub fn router(db: Arc<DatabaseConnection>, jwks: Arc<JwksClient>) -> Router {
    // ... existing body, but at the bottom:
        .with_state(db.clone())
    // ... unchanged
```

- [ ] **Step 4: Bridge for not-yet-ported modules**

For every module under `src/api/` and `src/import/`, the existing handlers expect `State<Arc<PgPool>>`. Until T5–T7 port them, add a temporary helper that turns `&DatabaseConnection` into `&PgPool`:

In `src/db.rs`, append:

```rust
/// Temporary bridge for modules still using sqlx directly (T5–T7 will remove the callers).
pub fn pool_of(db: &sea_orm::DatabaseConnection) -> &sqlx::PgPool {
    db.get_postgres_connection_pool()
}
```

Note: this requires `sqlx` as a transitive type. SeaORM 1.x with the `sqlx-postgres` feature re-exports `sqlx::PgPool` via `sea_orm::SqlxPostgresConnector`. If `db.get_postgres_connection_pool()` does not exist in your SeaORM version, use the alternative:

```rust
pub fn pool_of(db: &sea_orm::DatabaseConnection) -> &sqlx::PgPool {
    use sea_orm::DatabaseBackend;
    match db {
        sea_orm::DatabaseConnection::SqlxPostgresPoolConnection(c) => c.pool(),
        _ => panic!("expected SqlxPostgresPoolConnection"),
    }
}
```

Then in each unported handler:

```rust
// Before
async fn handle_x(State(pool): State<Arc<PgPool>>, ...) -> ...

// After (transitional)
async fn handle_x(State(db): State<Arc<DatabaseConnection>>, ...) -> ... {
    let pool = crate::db::pool_of(&db);
    // ... existing body
```

This bridge is mechanical — apply it across `api/*.rs` and `import/*.rs` until the crate compiles.

- [ ] **Step 5: Add `sqlx` back as a *transitional* direct dep (TEMPORARY)**

To avoid relying on transitive resolution, add to `server/Cargo.toml`:

```toml
sqlx = { version = "0.7", features = [
    "runtime-tokio", "tls-rustls", "postgres", "uuid", "chrono", "rust_decimal", "json"
] }
```

Note: NO `migrate` feature, NO `macros` (we will not use `query!`/`query_as!` after T7). This sqlx dep is removed in T8.

- [ ] **Step 6: Verify the crate compiles end-to-end**

```bash
cd /Users/juno/dev/finance_mananger
cargo check -p finance-manager 2>&1 | tail -30
```

Expected: clean. Warnings about `pool_of` being unused once a module is ported are fine.

- [ ] **Step 7: Update `bin/test_import.rs`**

Read the file first:

```bash
sed -n '1,40p' /Users/juno/dev/finance_mananger/server/src/bin/test_import.rs
```

Replace any `sqlx::migrate!("./migrations").run(...)` call with:

```rust
use migration::MigratorTrait;
migration::Migrator::up(&db, None).await?;
```

Replace `PgPool` with `DatabaseConnection` if the binary uses pool connectivity directly. If the binary uses sqlx queries, keep them via the `pool_of` bridge until T7.

- [ ] **Step 8: Build the test-DB helper**

Create `server/tests/common/mod.rs`:

```rust
//! Test helper that replaces `#[sqlx::test(migrations = "./migrations")]`.
//!
//! Each test gets a fresh ephemeral database. The DB name is randomized per test
//! and dropped on the `Drop` of `TestDb`.

#![allow(dead_code)]

use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, Database, DatabaseConnection, ConnectionTrait, Statement};
use std::time::Duration;
use uuid::Uuid;

const ADMIN_DB_ENV: &str = "DATABASE_URL";

pub struct TestDb {
    pub db: DatabaseConnection,
    pub pool: sqlx::PgPool,        // bridge for modules not yet ported
    pub url: String,
    db_name: String,
    admin_url: String,
}

impl TestDb {
    pub async fn new() -> Self {
        let admin_url = std::env::var(ADMIN_DB_ENV)
            .expect("DATABASE_URL must be set for integration tests");
        let db_name = format!("fm_test_{}", Uuid::new_v4().simple());

        // Connect to the admin DB to issue CREATE DATABASE.
        let admin = Database::connect(&admin_url).await
            .expect("admin DB connect failed");
        admin.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            format!(r#"CREATE DATABASE "{db_name}""#),
        )).await.expect("CREATE DATABASE failed");
        admin.close().await.ok();

        let test_url = replace_db_name(&admin_url, &db_name);
        let mut opts = ConnectOptions::new(&test_url);
        opts.max_connections(5).connect_timeout(Duration::from_secs(8));
        let db = Database::connect(opts).await.expect("test DB connect failed");

        Migrator::up(&db, None).await.expect("Migrator::up failed");

        let pool = crate::db_bridge::pool_of(&db).clone();

        Self { db, pool, url: test_url, db_name, admin_url }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let admin_url = self.admin_url.clone();
        let db_name = self.db_name.clone();
        // Best-effort drop; ignore failures (test runner may already be tearing down).
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                if let Ok(admin) = Database::connect(&admin_url).await {
                    let _ = admin.execute(Statement::from_string(
                        sea_orm::DatabaseBackend::Postgres,
                        format!(r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#),
                    )).await;
                }
            });
        }).join().ok();
    }
}

fn replace_db_name(url: &str, new_name: &str) -> String {
    let (head, _old) = url.rsplit_once('/').expect("url missing /db");
    format!("{head}/{new_name}")
}
```

The reference to `crate::db_bridge::pool_of` requires the helper to live behind a public module path accessible to tests. Add to `server/src/lib.rs`:

```rust
pub mod db_bridge {
    pub use crate::db::pool_of;
}
```

- [ ] **Step 9: Migrate one test file as a pilot — `tests/test_normalize.rs`**

This test does not use the DB; only confirm it still compiles:

```bash
cargo test -p finance-manager --test test_normalize 2>&1 | tail -10
```

Expected: 4–8 tests pass.

- [ ] **Step 10: Migrate one DB-using test — `tests/test_import_integration.rs`**

For each `#[sqlx::test(migrations = "./migrations")] async fn name(pool: PgPool)`, rewrite as:

```rust
#[path = "common/mod.rs"]
mod common;

#[tokio::test]
async fn import_golden_transactions_count() {
    let t = common::TestDb::new().await;
    let pool: &sqlx::PgPool = &t.pool;   // body unchanged below
    // ... existing test body, replacing `&pool` arg name only if needed
}
```

The transitional pattern keeps test bodies almost untouched — only the function signature and macro change.

- [ ] **Step 11: Migrate every other test file the same way**

For each file in `server/tests/test_*.rs` (excluding `test_normalize.rs` and `test_xlsx_grouping.rs` which don't use the DB):
- Replace `use sqlx::PgPool;` import with the `common` mod include shown above
- Replace `#[sqlx::test(migrations = "./migrations")]` with `#[tokio::test]`
- Inside the body, replace the function arg `pool: PgPool` with a local `let t = common::TestDb::new().await; let pool = t.pool.clone();` (or `&t.pool` if the test only reads)

Keep the Excel fixture path and all assertions byte-for-byte the same.

- [ ] **Step 12: Run the full backend test suite**

```bash
export DATABASE_URL="postgres://app:app@localhost:5432/finance"
cd /Users/juno/dev/finance_mananger/server
cargo test -p finance-manager 2>&1 | tail -30
```

Expected: **87 passed; 0 failed**. The same count as before. If any test fails with a connection or migration error, fix the helper before proceeding.

- [ ] **Step 13: Commit**

```bash
git add server/Cargo.toml server/src/db.rs server/src/main.rs server/src/lib.rs \
        server/src/api/mod.rs server/src/bin/test_import.rs server/tests/
git commit -m "feat(db): switch runtime + tests to sea_orm DatabaseConnection (sqlx bridge for unported modules)"
```

---

## Task 5 — Port simple CRUD modules to ORM-first

**Files:**
- Modify: `server/src/api/categories.rs`
- Modify: `server/src/api/products.rs`
- Modify: `server/src/api/transactions.rs`
- Modify: `server/src/api/aliases.rs`
- Modify: `server/src/api/import.rs`

Each module gets the same treatment: drop `pool_of` bridge, take `State<Arc<DatabaseConnection>>` directly, replace each `sqlx::query!` with the appropriate `Entity::find()` / `ActiveModel` / `Statement::from_sql_and_values` form. Test after each module.

### 5.1 `categories.rs`

- [ ] **Step 1: Replace `handle_get_categories`**

```rust
use crate::entity::{categories, prelude::Categories};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

pub async fn handle_get_categories(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
) -> AppResult<Json<Vec<CategoryItem>>> {
    let rows = Categories::find()
        .filter(categories::Column::OwnerId.eq(user.sub))
        .order_by_asc(categories::Column::Kind)
        .order_by_asc(categories::Column::Name)
        .all(&*db)
        .await?;

    Ok(Json(rows.into_iter().map(|r| CategoryItem {
        id: r.id,
        name: r.name,
        kind: r.kind,
        review_state: r.review_state,
        parent_id: r.parent_id,
    }).collect()))
}
```

- [ ] **Step 2: Replace `handle_get_merchants`**

```rust
use crate::entity::{merchants, prelude::Merchants};

pub async fn handle_get_merchants(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
) -> AppResult<Json<Vec<MerchantItem>>> {
    let rows = Merchants::find()
        .filter(merchants::Column::OwnerId.eq(user.sub))
        .order_by_asc(merchants::Column::Name)
        .all(&*db)
        .await?;

    Ok(Json(rows.into_iter().map(|r| MerchantItem {
        id: r.id, name: r.name, review_state: r.review_state
    }).collect()))
}
```

- [ ] **Step 3: Replace `handle_get_payment_methods` (LEFT JOIN)**

```rust
use crate::entity::{payment_methods, ledger_actors, prelude::*};

pub async fn handle_get_payment_methods(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
) -> AppResult<Json<Vec<PaymentMethodItem>>> {
    let rows: Vec<(payment_methods::Model, Option<ledger_actors::Model>)> =
        PaymentMethods::find()
            .find_also_related(LedgerActors)
            .filter(payment_methods::Column::OwnerId.eq(user.sub))
            .order_by_asc(payment_methods::Column::Name)
            .all(&*db)
            .await?;

    Ok(Json(rows.into_iter().map(|(pm, actor)| PaymentMethodItem {
        id: pm.id,
        name: pm.name,
        actor_id: pm.actor_id,
        actor_name: actor.map(|a| a.name),
        review_state: pm.review_state,
    }).collect()))
}
```

- [ ] **Step 4: Replace `handle_patch_category_kind`**

```rust
use sea_orm::{ActiveModelTrait, ActiveValue::Set};

pub async fn handle_patch_category_kind(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Path(category_id): Path<Uuid>,
    Json(body): Json<PatchCategoryKindBody>,
) -> AppResult<Json<PatchCategoryKindResponse>> {
    if body.kind != "income" && body.kind != "expense" {
        return Err(AppError::BadRequest("kind must be 'income' or 'expense'".into()));
    }

    let cat = Categories::find()
        .filter(categories::Column::Id.eq(category_id))
        .filter(categories::Column::OwnerId.eq(user.sub))
        .one(&*db)
        .await?
        .ok_or_else(|| AppError::NotFound("category not found".into()))?;

    if cat.name == "차감" {
        return Err(AppError::Conflict(json!({
            "error": "protected_category",
            "message": "차감 is a protected category and cannot be re-typed",
        })));
    }

    let mut active: categories::ActiveModel = cat.into();
    active.kind = Set(body.kind.clone());
    active.update(&*db).await?;

    Ok(Json(PatchCategoryKindResponse { id: category_id, kind: body.kind }))
}
```

- [ ] **Step 5: Drop the `sqlx::PgPool` import and any `pool_of` bridge in this file**

Confirm the file no longer imports `sqlx`.

- [ ] **Step 6: Run the categories-touching tests**

```bash
cargo test -p finance-manager --test test_category_kind 2>&1 | tail -15
cargo test -p finance-manager --test test_m2_step_b 2>&1 | tail -15
```

Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add server/src/api/categories.rs
git commit -m "refactor(api): port categories.rs to sea-orm"
```

### 5.2 `products.rs`, `transactions.rs`, `aliases.rs`, `import.rs`

Apply the same patterns to the remaining CRUD modules. For each module:

- [ ] **Step 1 (per module): Read the file, list every `sqlx::query!` site**

```bash
grep -n "sqlx::query" /Users/juno/dev/finance_mananger/server/src/api/<module>.rs
```

- [ ] **Step 2 (per module): Map each query to one of these patterns**

| sqlx pattern | SeaORM replacement |
|---|---|
| `SELECT ... FROM t WHERE owner_id=$1 AND ...` | `T::find().filter(...).all(&*db)` |
| `SELECT ... FROM a JOIN b ...` | `A::find().find_also_related(B).filter(...).all(&*db)` |
| `INSERT ... RETURNING id` | `t::ActiveModel { ..., ..Default::default() }.insert(&*db).await?` |
| `INSERT ... ON CONFLICT DO NOTHING RETURNING id` | `T::insert(am).on_conflict(OnConflict::columns([...]).do_nothing().to_owned()).exec(&*db)` |
| `UPDATE ... WHERE id=$1` | `t::ActiveModel { id: Set(id), col: Set(v), ..Default::default() }.update(&*db)` |
| `DELETE FROM ... WHERE id=$1` | `T::delete_by_id(id).exec(&*db)` |
| Complex aggregate/CTE/window | `Statement::from_sql_and_values(DbBackend::Postgres, sql, vals)` + `FromQueryResult` |

- [ ] **Step 3 (per module): Replace each query, removing `pool_of` and `sqlx::PgPool` references**

- [ ] **Step 4 (per module): Run the corresponding test**

| Module | Test command |
|---|---|
| `products.rs` | `cargo test -p finance-manager --test test_m3` |
| `transactions.rs` | `cargo test -p finance-manager --test test_owner_isolation` |
| `aliases.rs` | `cargo test -p finance-manager --test test_m2_step_b` |
| `import.rs` | `cargo test -p finance-manager --test test_import_integration` |

Expected: all green per module.

- [ ] **Step 5 (per module): Commit**

```bash
git add server/src/api/<module>.rs
git commit -m "refactor(api): port <module>.rs to sea-orm"
```

### 5.3 Full-suite checkpoint

- [ ] **Run the full backend test suite**

```bash
cd /Users/juno/dev/finance_mananger/server && cargo test -p finance-manager 2>&1 | tail -10
```

Expected: **87 passed**.

---

## Task 6 — Port aggregate / view modules with raw SQL + `FromQueryResult`

**Files:**
- Modify: `server/src/api/settlement.rs`
- Modify: `server/src/api/summary.rs`
- Modify: `server/src/api/income.rs`
- Modify: `server/src/api/merchant_stats.rs`
- Modify: `server/src/api/price.rs`
- Modify: `server/src/api/export.rs`

These modules contain complex GROUP BY, FILTER, and view selects. Per the spec we keep them as raw SQL strings but route them through SeaORM for connection management and result mapping.

### 6.1 `settlement.rs`

- [ ] **Step 1: Replace the handler body**

```rust
use sea_orm::{DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};

#[derive(FromQueryResult)]
struct SettlementRow {
    recognized_expense: Decimal,
    deducted_amount: Decimal,
    settlement_input: Decimal,
}

pub async fn handle_get_settlement(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Path((year, month)): Path<(i32, i32)>,
) -> AppResult<Json<SettlementResponse>> {
    let row = SettlementRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"SELECT recognized_expense, deducted_amount, settlement_input
           FROM v_monthly_settlement
           WHERE owner_id = $1 AND month = make_date($2, $3, 1)"#,
        [user.sub.into(), year.into(), month.into()],
    ))
    .one(&*db)
    .await?;

    let (recognized_expense, deducted_amount, settlement_input) = match row {
        Some(r) => (r.recognized_expense, r.deducted_amount, r.settlement_input),
        None => (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO),
    };

    Ok(Json(SettlementResponse { year, month, recognized_expense, deducted_amount, settlement_input }))
}
```

- [ ] **Step 2: Drop sqlx imports from this file**

- [ ] **Step 3: Run the settlement tests**

```bash
cargo test -p finance-manager --test test_m2_step_a 2>&1 | tail -10
cargo test -p finance-manager --test test_income_split 2>&1 | tail -10
```

Expected: green.

- [ ] **Step 4: Commit**

```bash
git add server/src/api/settlement.rs
git commit -m "refactor(api): port settlement.rs to sea-orm raw-sql + FromQueryResult"
```

### 6.2 Apply the same pattern to `summary.rs`, `income.rs`, `merchant_stats.rs`, `price.rs`, `export.rs`

For each module:

- [ ] **Step 1: Read the existing query and define a `#[derive(FromQueryResult)]` row struct**

The struct's field names must match the SELECT column aliases exactly.

- [ ] **Step 2: Replace each `sqlx::query!(...).fetch_all(&*pool)` with**

```rust
RowStruct::find_by_statement(Statement::from_sql_and_values(
    DatabaseBackend::Postgres,
    r#"<SAME SQL AS BEFORE, USING $1/$2/...>"#,
    [val1.into(), val2.into(), /* ... */],
)).all(&*db).await?
```

`one()` for single-row, `all()` for multi-row.

- [ ] **Step 3: Drop sqlx imports and the `pool_of` bridge**

- [ ] **Step 4: Run the relevant tests**

| Module | Test command |
|---|---|
| `summary.rs` | `cargo test -p finance-manager --test test_m2_step_a` |
| `income.rs` | `cargo test -p finance-manager --test test_income_split` |
| `merchant_stats.rs` / `price.rs` | `cargo test -p finance-manager --test test_m3` |
| `export.rs` | `cargo test -p finance-manager --test test_m4_export` |

- [ ] **Step 5: Commit per module**

```bash
git add server/src/api/<module>.rs
git commit -m "refactor(api): port <module>.rs to sea-orm raw-sql + FromQueryResult"
```

### 6.3 Full-suite checkpoint

- [ ] **Run the full backend test suite**

```bash
cargo test -p finance-manager 2>&1 | tail -10
```

Expected: **87 passed**.

---

## Task 7 — Port `import/pipeline.rs`

**Files:**
- Modify: `server/src/import/pipeline.rs`
- Modify: `server/src/api/import.rs` (transaction creation)

This is the largest single file (630 lines) and the most subtle (race-safe alias merge, INSERT ... ON CONFLICT DO NOTHING with partial unique indexes, RETURNING fallback SELECT). The signature changes from `&mut PgConnection` to `&DatabaseTransaction`.

- [ ] **Step 1: Change pipeline signatures**

Top of `pipeline.rs`:

```rust
use anyhow::{Context, Result};
use rust_decimal::Decimal;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseTransaction, EntityTrait, QueryFilter, Statement, TransactionTrait,
    sea_query::OnConflict,
};
use uuid::Uuid;

use crate::entity::{
    aliases, categories, ledger_actors, merchants, payment_methods, products,
    transactions, transactions_raw,
    prelude::*,
};
use crate::domain::{IntegrityWarning, RawRow, TransactionRow, UnresolvedAlias};
use crate::import::normalize::to_norm_key;
```

Every function that took `conn: &mut PgConnection` now takes `txn: &DatabaseTransaction`.

- [ ] **Step 2: Port `upsert_category` (representative — apply same pattern to merchant/actor/payment_method)**

```rust
async fn upsert_category(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    raw_text: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(raw_text);

    // 1. Alias lookup.
    let existing = Aliases::find()
        .filter(aliases::Column::OwnerId.eq(owner_id))
        .filter(aliases::Column::Scope.eq("category"))
        .filter(aliases::Column::NormKey.eq(&norm))
        .one(txn)
        .await?;

    if let Some(row) = existing {
        return Ok((row.target_id, false));
    }

    let is_deduction = norm == "차감";
    let review_state = if is_deduction { "confirmed" } else { "pending" };

    const INCOME_KEYWORDS: &[&str] = &["급여", "수입", "회수", "환급", "보험금"];
    let kind = if INCOME_KEYWORDS.iter().any(|kw| norm.contains(kw)) {
        "income"
    } else {
        "expense"
    };

    // 2. ON CONFLICT DO NOTHING — partial unique index requires a raw INSERT
    //    because the SeaORM ON CONFLICT API does not let us specify a WHERE clause.
    //    Use raw SQL targeting the partial index.
    let new_id_opt: Option<Uuid> = txn.query_one(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO categories (owner_id, name, kind, review_state)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (owner_id, name) WHERE parent_id IS NULL DO NOTHING
           RETURNING id"#,
        [owner_id.into(), norm.clone().into(), kind.into(), review_state.into()],
    )).await?
        .map(|r| r.try_get::<Uuid>("", "id"))
        .transpose()?;

    let (cat_id, is_new) = match new_id_opt {
        Some(id) => (id, true),
        None => {
            let row = Categories::find()
                .filter(categories::Column::OwnerId.eq(owner_id))
                .filter(categories::Column::Name.eq(&norm))
                .filter(categories::Column::ParentId.is_null())
                .one(txn)
                .await?
                .context("category conflict-fallback SELECT failed")?;
            (row.id, false)
        }
    };

    // 3. Register alias.
    Aliases::insert(aliases::ActiveModel {
        owner_id: Set(owner_id),
        scope: Set("category".into()),
        raw_text: Set(raw_text.into()),
        norm_key: Set(norm),
        target_id: Set(cat_id),
        ..Default::default()
    })
    .on_conflict(OnConflict::columns([
        aliases::Column::OwnerId, aliases::Column::Scope, aliases::Column::NormKey,
    ]).do_nothing().to_owned())
    .exec(txn)
    .await?;

    Ok((cat_id, is_new))
}
```

- [ ] **Step 3: Port `upsert_merchant`, `upsert_actor`, `upsert_payment_method`**

These are simpler — they target full UNIQUE constraints, so SeaORM's `OnConflict` works directly without raw SQL.

```rust
async fn upsert_merchant(txn: &DatabaseTransaction, owner_id: Uuid, raw_text: &str)
    -> Result<(Uuid, bool)>
{
    let norm = to_norm_key(raw_text);

    if let Some(row) = Aliases::find()
        .filter(aliases::Column::OwnerId.eq(owner_id))
        .filter(aliases::Column::Scope.eq("merchant"))
        .filter(aliases::Column::NormKey.eq(&norm))
        .one(txn).await?
    {
        return Ok((row.target_id, false));
    }

    let result = Merchants::insert(merchants::ActiveModel {
        owner_id: Set(owner_id),
        name: Set(norm.clone()),
        review_state: Set("pending".into()),
        ..Default::default()
    })
    .on_conflict(OnConflict::columns([
        merchants::Column::OwnerId, merchants::Column::Name,
    ]).do_nothing().to_owned())
    .exec(txn).await;

    let (merch_id, is_new) = match result {
        Ok(r) => (r.last_insert_id, true),
        Err(sea_orm::DbErr::RecordNotInserted) => {
            let row = Merchants::find()
                .filter(merchants::Column::OwnerId.eq(owner_id))
                .filter(merchants::Column::Name.eq(&norm))
                .one(txn).await?
                .context("merchant conflict-fallback SELECT failed")?;
            (row.id, false)
        }
        Err(e) => return Err(e.into()),
    };

    Aliases::insert(aliases::ActiveModel {
        owner_id: Set(owner_id),
        scope: Set("merchant".into()),
        raw_text: Set(raw_text.into()),
        norm_key: Set(norm),
        target_id: Set(merch_id),
        ..Default::default()
    })
    .on_conflict(OnConflict::columns([
        aliases::Column::OwnerId, aliases::Column::Scope, aliases::Column::NormKey,
    ]).do_nothing().to_owned())
    .exec(txn).await?;

    Ok((merch_id, is_new))
}
```

Apply the same shape to `upsert_actor` (table `ledger_actors`) and `upsert_payment_method` (table `payment_methods`). For products, use the partial-index raw-SQL pattern from Step 2 (because the unique index is partial on `WHERE merchant_id IS NOT NULL`).

- [ ] **Step 4: Port `insert_raw` and `insert_transaction` to `ActiveModel::insert`**

```rust
async fn insert_raw(txn: &DatabaseTransaction, owner_id: Uuid, batch_id: Uuid, row: &RawRow)
    -> Result<Uuid>
{
    let am = transactions_raw::ActiveModel {
        owner_id: Set(owner_id),
        import_batch_id: Set(batch_id),
        row_index: Set(row.row_index),
        group_id: Set(row.group_id),
        is_group_header: Set(row.is_group_header),
        occurred_on: Set(row.occurred_on),
        raw_date_serial: Set(row.raw_date_serial),
        merchant_text: Set(row.merchant_text.clone()),
        actor_text: Set(row.actor_text.clone()),
        category_text: Set(row.category_text.clone()),
        total_amount: Set(row.total_amount),
        memo: Set(row.memo.clone()),
        unit_price: Set(row.unit_price),
        quantity: Set(row.quantity),
        line_amount: Set(row.line_amount),
        payment_text: Set(row.payment_text.clone()),
        evidence_text: Set(row.evidence_text.clone()),
        extras: Set(row.extras.clone()),
        ..Default::default()
    };
    let inserted = am.insert(txn).await?;
    Ok(inserted.id)
}

async fn insert_transaction(txn: &DatabaseTransaction, owner_id: Uuid, t: &TransactionRow)
    -> Result<Uuid>
{
    let am = transactions::ActiveModel {
        owner_id: Set(owner_id),
        raw_id: Set(t.raw_id),
        group_id: Set(t.group_id),
        occurred_on: Set(t.occurred_on),
        merchant_id: Set(t.merchant_id),
        actor_id: Set(t.actor_id),
        category_id: Set(t.category_id),
        product_id: Set(t.product_id),
        payment_method_id: Set(t.payment_method_id),
        amount: Set(t.amount),
        unit_price: Set(t.unit_price),
        quantity: Set(t.quantity),
        memo: Set(t.memo.clone()),
        ..Default::default()
    };
    let inserted = am.insert(txn).await?;
    Ok(inserted.id)
}
```

- [ ] **Step 5: Port `check_group_integrity` to raw SQL + `FromQueryResult`**

```rust
#[derive(sea_orm::FromQueryResult)]
struct IntegrityRow {
    group_id: Uuid,
    header_total: Option<Decimal>,
    lines_sum: Option<Decimal>,
}

pub async fn check_group_integrity(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    batch_id: Uuid,
) -> Result<Vec<IntegrityWarning>> {
    let rows = IntegrityRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
        SELECT
            g.group_id,
            g.header_total,
            COALESCE(-SUM(t.amount), 0) AS lines_sum
        FROM (
            SELECT group_id, total_amount AS header_total
            FROM transactions_raw
            WHERE is_group_header = true
              AND owner_id = $1
              AND import_batch_id = $2
        ) g
        LEFT JOIN transactions t ON t.group_id = g.group_id AND t.owner_id = $1
        GROUP BY g.group_id, g.header_total
        HAVING g.header_total <> COALESCE(-SUM(t.amount), 0)
        "#,
        [owner_id.into(), batch_id.into()],
    )).all(txn).await?;

    Ok(rows.into_iter().map(|r| IntegrityWarning {
        group_id: r.group_id,
        header_total: r.header_total.unwrap_or(Decimal::ZERO),
        lines_sum: r.lines_sum.unwrap_or(Decimal::ZERO),
    }).collect())
}
```

- [ ] **Step 6: Update `run_pipeline` signature and body**

Change `conn: &mut PgConnection` → `txn: &DatabaseTransaction`. Replace every `conn` argument inside the body with `txn`. The domain logic (sign flip, 보험 split, INCOME_KEYWORDS) is unchanged.

- [ ] **Step 7: Update the caller in `api/import.rs`**

```rust
use sea_orm::TransactionTrait;

let txn = db.begin().await?;

// import_batches insert
let batch_id = ImportBatches::insert(import_batches::ActiveModel {
    owner_id: Set(owner_id),
    file_name: Set(filename.into()),
    file_hash: Set(hash_vec.clone()),
    year: Set(year),
    month: Set(month),
    row_count: Set(row_count),
    ..Default::default()
})
.on_conflict(OnConflict::columns([
    import_batches::Column::OwnerId, import_batches::Column::FileHash,
]).do_nothing().to_owned())
.exec(&txn)
.await;

// match on RecordNotInserted to surface the 409 conflict path

let (count, integrity, unresolved) = run_pipeline(&txn, owner_id, batch_id, raw_rows).await?;

txn.commit().await?;
```

- [ ] **Step 8: Update test helpers in `tests/test_import_integration.rs` and `tests/test_m2_step_a.rs`**

Where the tests previously called `pool.begin()` and passed `&mut *tx` to `run_pipeline`, now use `t.db.begin().await?` and pass `&txn`.

Inline the helper from `test_import_integration.rs::run_import` to use SeaORM `TransactionTrait`. The body becomes:

```rust
let txn = db.begin().await?;
// ... ON CONFLICT logic via SeaORM ...
let (transactions_inserted, integrity_warnings, _) =
    run_pipeline(&txn, owner_id, batch_id, raw_rows).await?;
txn.commit().await?;
```

- [ ] **Step 9: Run the import and concurrency tests**

```bash
cargo test -p finance-manager --test test_import_integration 2>&1 | tail -10
cargo test -p finance-manager --test test_import_kind_heuristic 2>&1 | tail -10
cargo test -p finance-manager --test test_m2_concurrency 2>&1 | tail -10
cargo test -p finance-manager --test test_xlsx_grouping 2>&1 | tail -10
```

Expected: all green. The concurrency test specifically guards the race-safe alias merge — pay attention to its result.

- [ ] **Step 10: Run the full backend test suite**

```bash
cargo test -p finance-manager 2>&1 | tail -10
```

Expected: **87 passed**.

- [ ] **Step 11: Data-equivalence smoke against the golden xlsx**

```bash
export DATABASE_URL="postgres://app:app@localhost:5432/fm_smoke"
psql "postgres://app:app@localhost:5432/postgres" -c 'DROP DATABASE IF EXISTS fm_smoke;'
psql "postgres://app:app@localhost:5432/postgres" -c 'CREATE DATABASE fm_smoke;'

# Boot server (which auto-runs migration), then import the golden xlsx via the API
cargo run -p finance-manager --bin finance-manager &
SERVER_PID=$!
sleep 3
# (manual: POST the xlsx via curl with a test JWT — or use bin/test_import.rs)
cargo run -p finance-manager --bin test-import -- "../2026년 02월.xlsx"
kill $SERVER_PID 2>/dev/null

# Verify row counts and view results
psql "$DATABASE_URL" -c "SELECT COUNT(*) FROM transactions"           # expect 177
psql "$DATABASE_URL" -c "SELECT * FROM v_monthly_settlement"          # expect 1 row, deducted_amount=7500 etc.
psql "$DATABASE_URL" -c "
  SELECT amount FROM transactions t
  JOIN merchants m ON m.id=t.merchant_id
  JOIN products p ON p.id=t.product_id
  WHERE m.name='고덕방' AND p.name='아이스아메리카노'"               # expect 6 rows, all -3400
```

If any expectation fails, fix the pipeline before committing.

- [ ] **Step 12: Commit**

```bash
git add server/src/import/pipeline.rs server/src/api/import.rs server/tests/
git commit -m "refactor(import): port pipeline.rs to sea-orm DatabaseTransaction + ActiveModel"
```

---

## Task 8 — Cleanup, docs, final verification

**Files:**
- Modify: `server/Cargo.toml`
- Modify: `server/src/db.rs` (drop `pool_of` bridge)
- Modify: `server/src/lib.rs` (drop `db_bridge` module)
- Delete: `.sqlx/` directory (if present)
- Delete: `sqlx.sh` (if present)
- Delete: `server/tests/common/mod.rs::pool` field (no longer needed)
- Modify: `CLAUDE.md`

- [ ] **Step 1: Remove the sqlx direct dependency**

In `server/Cargo.toml`, delete the `sqlx = { version = "0.7", ... }` line that was added in T4 Step 5.

- [ ] **Step 2: Remove the `pool_of` bridge**

Delete the `pool_of` function from `server/src/db.rs` and the `db_bridge` re-export from `server/src/lib.rs`.

- [ ] **Step 3: Remove the `pool` field from `TestDb`**

Edit `server/tests/common/mod.rs`:

```rust
pub struct TestDb {
    pub db: DatabaseConnection,
    pub url: String,
    db_name: String,
    admin_url: String,
}
// remove pool field and the line `let pool = ...`
```

- [ ] **Step 4: Verify the crate builds without sqlx**

```bash
cd /Users/juno/dev/finance_mananger
cargo build -p finance-manager 2>&1 | tail -5
cargo tree -p finance-manager | grep -E "^[^├└]*sqlx " || echo "no direct sqlx — good"
```

Expected: clean build. `cargo tree` shows sqlx only as a transitive of sea-orm, not as a direct dep.

- [ ] **Step 5: Delete `.sqlx/` and `sqlx.sh`**

```bash
rm -rf /Users/juno/dev/finance_mananger/.sqlx
rm -rf /Users/juno/dev/finance_mananger/server/.sqlx
rm -f /Users/juno/dev/finance_mananger/sqlx.sh
rm -f /Users/juno/dev/finance_mananger/server/sqlx.sh
```

Some of these may not exist; that's fine.

- [ ] **Step 6: Run the full backend suite one more time**

```bash
cd /Users/juno/dev/finance_mananger/server
cargo test -p finance-manager 2>&1 | tail -10
```

Expected: **87 passed**.

- [ ] **Step 7: Run the frontend suite**

```bash
cd /Users/juno/dev/finance_mananger/web
npm test 2>&1 | tail -15
```

Expected: **124 passed**.

- [ ] **Step 8: Update `CLAUDE.md`**

Open `CLAUDE.md`. Apply these targeted edits:

(a) Backend stack section — replace the line:
> `DB: PostgreSQL 17, sqlx with compile-time query checking.`
with:
> `DB: PostgreSQL 17, SeaORM 1.x (entity-driven; raw SQL via FromQueryResult for views and complex aggregates).`

(b) Migration Policy section — replace the body with:
> `- The schema lives in a single migration file at server/migration/src/m20260510_000001_init.rs. When the schema changes, edit this file in place; do not accumulate migrations.`
> `- The dev workflow is to rerun Migrator::fresh against the dev DB after edits. Production runs Migrator::up at boot.`
> `- Entities live under server/src/entity/, generated by sea-orm-cli generate entity. Treat that directory as auto-generated and regenerate after every migration edit.`

(c) How to Run Tests section — keep the `cargo test -p server` command unchanged, but remove any mention of `SQLX_OFFLINE` or `cargo sqlx prepare`.

(d) Append a new bullet to "Cumulative Context (Documentation Agent)":

> `- 2026-05-10: sqlx → SeaORM 원샷 이관 완료. server/migration/ 신규 크레이트(단일 마이그레이션 in-place 정책 그대로 이식), src/entity/ sea-orm-cli 자동 생성. CRUD/JOIN 은 ORM-first(find_*_related, ActiveModel, OnConflict), 뷰·집계는 raw SQL + FromQueryResult. import/pipeline.rs 는 DatabaseTransaction + ActiveModel 로 재작성, race-safe alias merge 패턴 보존. 테스트 인프라는 #[sqlx::test] → tokio::test + tests/common/TestDb 헬퍼(per-test ephemeral DB + Migrator::up). .sqlx/ 캐시·sqlx.sh 폐기. 백엔드 87/87, 프런트 124/124. Spec/plan: docs/superpowers/{specs,plans}/2026-05-10-seaorm-migration*.`

- [ ] **Step 9: Final commit**

```bash
git add server/Cargo.toml server/src/db.rs server/src/lib.rs \
        server/tests/common/mod.rs CLAUDE.md
git rm -rf .sqlx server/.sqlx 2>/dev/null || true
git rm -f sqlx.sh server/sqlx.sh 2>/dev/null || true
git commit -m "chore(seaorm): drop sqlx direct dep, .sqlx cache, and sqlx.sh; update CLAUDE.md"
```

- [ ] **Step 10: Open the PR**

```bash
git push -u origin migrate_seaorm
gh pr create --title "Replace sqlx with SeaORM (one-shot migration)" --body "$(cat <<'EOF'
## Summary
- Add `migration/` workspace member with single in-place-edited migration (replaces `001_init.sql`).
- Generate `server/src/entity/` from migrated DB; check in.
- Replace `PgPool` with `sea_orm::DatabaseConnection` everywhere; ORM-first for CRUD/JOIN, raw SQL + `FromQueryResult` for views and aggregates.
- Rebuild integration test infra around `tests/common/TestDb` (ephemeral DB + `Migrator::up`); drop `#[sqlx::test]`.
- Delete `.sqlx/` cache, `sqlx.sh`, and direct `sqlx` dependency.

## Test plan
- [ ] `cargo test -p finance-manager` — 87 passed
- [ ] `npm test` — 124 passed
- [ ] Golden xlsx import: 177 transaction rows, group-sum integrity 0 violations
- [ ] `v_monthly_settlement` 2026-02 row matches pre-migration values
- [ ] 고덕방 아이스아메리카노 6 rows at -3400

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Verification gate (must all pass before merge)

- [ ] `cargo build -p finance-manager` clean
- [ ] `cargo test -p finance-manager` → **87 passed**
- [ ] `cd web && npm test` → **124 passed**
- [ ] `cargo tree -p finance-manager | grep -E "^[^├└]*sqlx "` returns no direct sqlx dependency
- [ ] `.sqlx/` and `sqlx.sh` removed from the working tree
- [ ] `001_init.sql` removed; `server/migration/src/m20260510_000001_init.rs` is the single source of truth
- [ ] Manual smoke: golden xlsx import yields 177 transactions and the expected `v_monthly_settlement` row
- [ ] CLAUDE.md updated with new architecture/migration policy and a new cumulative-context bullet
