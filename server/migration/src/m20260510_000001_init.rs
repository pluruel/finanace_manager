use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
pub enum LedgerActors {
    Table,
    Id,
    OwnerId,
    Name,
}

#[derive(DeriveIden)]
pub enum Categories {
    Table,
    Id,
    OwnerId,
    ParentId,
    Name,
    Kind,
    ReviewState,
}

#[derive(DeriveIden)]
pub enum Merchants {
    Table,
    Id,
    OwnerId,
    Name,
    ReviewState,
}

#[derive(DeriveIden)]
pub enum Products {
    Table,
    Id,
    OwnerId,
    MerchantId,
    Name,
    ReviewState,
}

#[derive(DeriveIden)]
pub enum PaymentMethods {
    Table,
    Id,
    OwnerId,
    Name,
    ActorId,
    ReviewState,
}

#[derive(DeriveIden)]
pub enum Aliases {
    Table,
    Id,
    OwnerId,
    Scope,
    RawText,
    NormKey,
    TargetId,
}

#[derive(DeriveIden)]
pub enum ImportBatches {
    Table,
    Id,
    OwnerId,
    FileName,
    FileHash,
    Year,
    Month,
    RowCount,
    ImportedAt,
}

#[derive(DeriveIden)]
pub enum TransactionsRaw {
    Table,
    Id,
    OwnerId,
    ImportBatchId,
    RowIndex,
    GroupId,
    IsGroupHeader,
    OccurredOn,
    RawDateSerial,
    MerchantText,
    ActorText,
    CategoryText,
    TotalAmount,
    Memo,
    UnitPrice,
    Quantity,
    LineAmount,
    PaymentText,
    EvidenceText,
    Extras,
}

#[derive(DeriveIden)]
pub enum Transactions {
    Table,
    Id,
    OwnerId,
    RawId,
    GroupId,
    OccurredOn,
    MerchantId,
    ActorId,
    CategoryId,
    ProductId,
    PaymentMethodId,
    Amount,
    UnitPrice,
    Quantity,
    Memo,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let conn = m.get_connection();

        // pgcrypto for gen_random_uuid()
        conn.execute_unprepared("CREATE EXTENSION IF NOT EXISTS pgcrypto")
            .await?;

        // ledger_actors
        m.create_table(
            Table::create()
                .table(LedgerActors::Table)
                .col(
                    ColumnDef::new(LedgerActors::Id)
                        .uuid()
                        .not_null()
                        .primary_key()
                        .extra("DEFAULT gen_random_uuid()".to_owned()),
                )
                .col(ColumnDef::new(LedgerActors::OwnerId).uuid().not_null())
                .col(ColumnDef::new(LedgerActors::Name).text().not_null())
                .index(
                    Index::create()
                        .unique()
                        .name("ledger_actors_owner_name_uniq")
                        .col(LedgerActors::OwnerId)
                        .col(LedgerActors::Name),
                )
                .to_owned(),
        )
        .await?;

        // categories
        m.create_table(
            Table::create()
                .table(Categories::Table)
                .col(
                    ColumnDef::new(Categories::Id)
                        .uuid()
                        .not_null()
                        .primary_key()
                        .extra("DEFAULT gen_random_uuid()".to_owned()),
                )
                .col(ColumnDef::new(Categories::OwnerId).uuid().not_null())
                .col(ColumnDef::new(Categories::ParentId).uuid())
                .col(ColumnDef::new(Categories::Name).text().not_null())
                .col(
                    ColumnDef::new(Categories::Kind)
                        .text()
                        .not_null()
                        .check(Expr::col(Categories::Kind).is_in(["income", "expense"])),
                )
                .col(
                    ColumnDef::new(Categories::ReviewState)
                        .text()
                        .not_null()
                        .default("pending")
                        .check(
                            Expr::col(Categories::ReviewState)
                                .is_in(["pending", "confirmed"]),
                        ),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("categories_parent_fk")
                        .from(Categories::Table, Categories::ParentId)
                        .to(Categories::Table, Categories::Id),
                )
                .to_owned(),
        )
        .await?;

        // merchants
        m.create_table(
            Table::create()
                .table(Merchants::Table)
                .col(
                    ColumnDef::new(Merchants::Id)
                        .uuid()
                        .not_null()
                        .primary_key()
                        .extra("DEFAULT gen_random_uuid()".to_owned()),
                )
                .col(ColumnDef::new(Merchants::OwnerId).uuid().not_null())
                .col(ColumnDef::new(Merchants::Name).text().not_null())
                .col(
                    ColumnDef::new(Merchants::ReviewState)
                        .text()
                        .not_null()
                        .default("pending"),
                )
                .index(
                    Index::create()
                        .unique()
                        .name("merchants_owner_name_uniq")
                        .col(Merchants::OwnerId)
                        .col(Merchants::Name),
                )
                .to_owned(),
        )
        .await?;

        // products
        m.create_table(
            Table::create()
                .table(Products::Table)
                .col(
                    ColumnDef::new(Products::Id)
                        .uuid()
                        .not_null()
                        .primary_key()
                        .extra("DEFAULT gen_random_uuid()".to_owned()),
                )
                .col(ColumnDef::new(Products::OwnerId).uuid().not_null())
                .col(ColumnDef::new(Products::MerchantId).uuid())
                .col(ColumnDef::new(Products::Name).text().not_null())
                .col(
                    ColumnDef::new(Products::ReviewState)
                        .text()
                        .not_null()
                        .default("pending")
                        .check(
                            Expr::col(Products::ReviewState)
                                .is_in(["pending", "confirmed"]),
                        ),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("products_merchant_fk")
                        .from(Products::Table, Products::MerchantId)
                        .to(Merchants::Table, Merchants::Id),
                )
                .to_owned(),
        )
        .await?;

        // payment_methods
        m.create_table(
            Table::create()
                .table(PaymentMethods::Table)
                .col(
                    ColumnDef::new(PaymentMethods::Id)
                        .uuid()
                        .not_null()
                        .primary_key()
                        .extra("DEFAULT gen_random_uuid()".to_owned()),
                )
                .col(ColumnDef::new(PaymentMethods::OwnerId).uuid().not_null())
                .col(ColumnDef::new(PaymentMethods::Name).text().not_null())
                .col(ColumnDef::new(PaymentMethods::ActorId).uuid())
                .col(
                    ColumnDef::new(PaymentMethods::ReviewState)
                        .text()
                        .not_null()
                        .default("pending")
                        .check(
                            Expr::col(PaymentMethods::ReviewState)
                                .is_in(["pending", "confirmed"]),
                        ),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("payment_methods_actor_fk")
                        .from(PaymentMethods::Table, PaymentMethods::ActorId)
                        .to(LedgerActors::Table, LedgerActors::Id),
                )
                .index(
                    Index::create()
                        .unique()
                        .name("payment_methods_owner_name_uniq")
                        .col(PaymentMethods::OwnerId)
                        .col(PaymentMethods::Name),
                )
                .to_owned(),
        )
        .await?;

        // aliases
        m.create_table(
            Table::create()
                .table(Aliases::Table)
                .col(
                    ColumnDef::new(Aliases::Id)
                        .uuid()
                        .not_null()
                        .primary_key()
                        .extra("DEFAULT gen_random_uuid()".to_owned()),
                )
                .col(ColumnDef::new(Aliases::OwnerId).uuid().not_null())
                .col(
                    ColumnDef::new(Aliases::Scope)
                        .text()
                        .not_null()
                        .check(Expr::col(Aliases::Scope).is_in([
                            "category",
                            "merchant",
                            "payment_method",
                            "actor",
                            "product",
                        ])),
                )
                .col(ColumnDef::new(Aliases::RawText).text().not_null())
                .col(ColumnDef::new(Aliases::NormKey).text().not_null())
                .col(ColumnDef::new(Aliases::TargetId).uuid().not_null())
                .index(
                    Index::create()
                        .unique()
                        .name("aliases_owner_scope_norm_uniq")
                        .col(Aliases::OwnerId)
                        .col(Aliases::Scope)
                        .col(Aliases::NormKey),
                )
                .to_owned(),
        )
        .await?;

        m.create_index(
            Index::create()
                .name("aliases_lookup_idx")
                .table(Aliases::Table)
                .col(Aliases::OwnerId)
                .col(Aliases::Scope)
                .col(Aliases::NormKey)
                .to_owned(),
        )
        .await?;

        // Partial unique indexes — DSL does not support WHERE clauses, so raw SQL.
        conn.execute_unprepared(
            r#"
            CREATE UNIQUE INDEX categories_owner_name_root_uniq
              ON categories (owner_id, name) WHERE parent_id IS NULL;
            CREATE UNIQUE INDEX categories_owner_parent_name_uniq
              ON categories (owner_id, parent_id, name) WHERE parent_id IS NOT NULL;
            CREATE UNIQUE INDEX products_owner_merchant_name_uniq
              ON products (owner_id, merchant_id, name) WHERE merchant_id IS NOT NULL;
            CREATE UNIQUE INDEX products_owner_name_no_merchant_uniq
              ON products (owner_id, name) WHERE merchant_id IS NULL;
        "#,
        )
        .await?;

        // import_batches
        m.create_table(
            Table::create()
                .table(ImportBatches::Table)
                .col(
                    ColumnDef::new(ImportBatches::Id)
                        .uuid()
                        .not_null()
                        .primary_key()
                        .extra("DEFAULT gen_random_uuid()".to_owned()),
                )
                .col(ColumnDef::new(ImportBatches::OwnerId).uuid().not_null())
                .col(ColumnDef::new(ImportBatches::FileName).text().not_null())
                .col(ColumnDef::new(ImportBatches::FileHash).binary().not_null())
                .col(ColumnDef::new(ImportBatches::Year).integer().not_null())
                .col(ColumnDef::new(ImportBatches::Month).integer().not_null())
                .col(ColumnDef::new(ImportBatches::RowCount).integer().not_null())
                .col(
                    ColumnDef::new(ImportBatches::ImportedAt)
                        .timestamp_with_time_zone()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .index(
                    Index::create()
                        .unique()
                        .name("import_batches_owner_hash_uniq")
                        .col(ImportBatches::OwnerId)
                        .col(ImportBatches::FileHash),
                )
                .to_owned(),
        )
        .await?;

        // transactions_raw
        m.create_table(
            Table::create()
                .table(TransactionsRaw::Table)
                .col(
                    ColumnDef::new(TransactionsRaw::Id)
                        .uuid()
                        .not_null()
                        .primary_key()
                        .extra("DEFAULT gen_random_uuid()".to_owned()),
                )
                .col(ColumnDef::new(TransactionsRaw::OwnerId).uuid().not_null())
                .col(
                    ColumnDef::new(TransactionsRaw::ImportBatchId)
                        .uuid()
                        .not_null(),
                )
                .col(ColumnDef::new(TransactionsRaw::RowIndex).integer().not_null())
                .col(ColumnDef::new(TransactionsRaw::GroupId).uuid().not_null())
                .col(
                    ColumnDef::new(TransactionsRaw::IsGroupHeader)
                        .boolean()
                        .not_null(),
                )
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
                .foreign_key(
                    ForeignKey::create()
                        .name("transactions_raw_batch_fk")
                        .from(TransactionsRaw::Table, TransactionsRaw::ImportBatchId)
                        .to(ImportBatches::Table, ImportBatches::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

        m.create_index(
            Index::create()
                .name("transactions_raw_date_idx")
                .table(TransactionsRaw::Table)
                .col(TransactionsRaw::OwnerId)
                .col(TransactionsRaw::OccurredOn)
                .to_owned(),
        )
        .await?;

        m.create_index(
            Index::create()
                .name("transactions_raw_group_idx")
                .table(TransactionsRaw::Table)
                .col(TransactionsRaw::OwnerId)
                .col(TransactionsRaw::GroupId)
                .to_owned(),
        )
        .await?;

        // transactions
        m.create_table(
            Table::create()
                .table(Transactions::Table)
                .col(
                    ColumnDef::new(Transactions::Id)
                        .uuid()
                        .not_null()
                        .primary_key()
                        .extra("DEFAULT gen_random_uuid()".to_owned()),
                )
                .col(ColumnDef::new(Transactions::OwnerId).uuid().not_null())
                .col(ColumnDef::new(Transactions::RawId).uuid().not_null())
                .col(ColumnDef::new(Transactions::GroupId).uuid().not_null())
                .col(ColumnDef::new(Transactions::OccurredOn).date().not_null())
                .col(ColumnDef::new(Transactions::MerchantId).uuid())
                .col(ColumnDef::new(Transactions::ActorId).uuid())
                .col(ColumnDef::new(Transactions::CategoryId).uuid())
                .col(ColumnDef::new(Transactions::ProductId).uuid())
                .col(ColumnDef::new(Transactions::PaymentMethodId).uuid())
                .col(
                    ColumnDef::new(Transactions::Amount)
                        .decimal_len(15, 2)
                        .not_null(),
                )
                .col(ColumnDef::new(Transactions::UnitPrice).decimal_len(15, 4))
                .col(ColumnDef::new(Transactions::Quantity).decimal_len(15, 4))
                .col(ColumnDef::new(Transactions::Memo).text())
                .foreign_key(
                    ForeignKey::create()
                        .name("transactions_raw_fk")
                        .from(Transactions::Table, Transactions::RawId)
                        .to(TransactionsRaw::Table, TransactionsRaw::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("transactions_merchant_fk")
                        .from(Transactions::Table, Transactions::MerchantId)
                        .to(Merchants::Table, Merchants::Id),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("transactions_actor_fk")
                        .from(Transactions::Table, Transactions::ActorId)
                        .to(LedgerActors::Table, LedgerActors::Id),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("transactions_category_fk")
                        .from(Transactions::Table, Transactions::CategoryId)
                        .to(Categories::Table, Categories::Id),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("transactions_product_fk")
                        .from(Transactions::Table, Transactions::ProductId)
                        .to(Products::Table, Products::Id),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("transactions_payment_method_fk")
                        .from(Transactions::Table, Transactions::PaymentMethodId)
                        .to(PaymentMethods::Table, PaymentMethods::Id),
                )
                .to_owned(),
        )
        .await?;

        // transactions indexes — use raw SQL because the DSL doesn't support DESC direction
        // on a single column easily (transactions_date_idx needs occurred_on DESC).
        for (name, cols) in [
            ("transactions_date_idx", "owner_id, occurred_on DESC"),
            ("transactions_category_idx", "owner_id, category_id, occurred_on"),
            ("transactions_merchant_idx", "owner_id, merchant_id, occurred_on"),
            ("transactions_product_idx", "owner_id, product_id, occurred_on"),
            ("transactions_group_idx", "owner_id, group_id"),
        ] {
            conn.execute_unprepared(&format!("CREATE INDEX {name} ON transactions ({cols})"))
                .await?;
        }

        // v_monthly_settlement view — FILTER aggregates require raw SQL
        conn.execute_unprepared(
            r#"
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
        "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let conn = m.get_connection();
        conn.execute_unprepared("DROP VIEW IF EXISTS v_monthly_settlement")
            .await?;
        m.drop_table(Table::drop().table(Transactions::Table).to_owned())
            .await?;
        m.drop_table(Table::drop().table(TransactionsRaw::Table).to_owned())
            .await?;
        m.drop_table(Table::drop().table(ImportBatches::Table).to_owned())
            .await?;
        m.drop_table(Table::drop().table(Aliases::Table).to_owned())
            .await?;
        m.drop_table(Table::drop().table(PaymentMethods::Table).to_owned())
            .await?;
        m.drop_table(Table::drop().table(Products::Table).to_owned())
            .await?;
        m.drop_table(Table::drop().table(Merchants::Table).to_owned())
            .await?;
        m.drop_table(Table::drop().table(Categories::Table).to_owned())
            .await?;
        m.drop_table(Table::drop().table(LedgerActors::Table).to_owned())
            .await?;
        Ok(())
    }
}
