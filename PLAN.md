# Finance Manager — Initial Implementation Plan

## Context

The user has been entering monthly household ledger data into separate Excel files (`YYYY년 MM월.xlsx`), and Excel will remain the input medium going forward. This app is a **viewer that integrates and analyzes data across multiple months**. Three core values:

1. Import monthly `.xlsx` files into PostgreSQL and accumulate them.
2. **Normalize categories / merchants / products** — fold spelling variants like "이 마트" / "이마트", "외식_점심" / "외식 점심", "조닌끼안티" / "조닌 끼안티" into a single entity.
3. **Price tracking** — map rows where the memo (column F `내용`) is filled into product line items, building a unit-price time series. Rows without a memo fall back to per-merchant statistics.

The server is Rust (axum), the DB is PostgreSQL 17, and the frontend is Next.js 15 App Router. Authentication is delegated to external `auth.junodevs.com` (EdDSA JWT, MSA). Downstream DBs hold only the `sub` (uuid) and must not replicate user information (per the `MSA_INTEGRATION.md` contract).

### Excel Structure — Key Findings (measured on `2026년 02월.xlsx`)

One Excel row is not necessarily one transaction. **A single receipt may decompose into a multi-line group** of rows.

- **Header row**: column E `지출(합계)` holds the receipt total. `unit_price × quantity = 지출(매수)` is filled simultaneously, meaning the header is itself a line.
- **Child rows**: a row immediately following a header that shares the same `occurred_on` and has column E empty is a child of the same group. Only column G (`단가` / unit price), H (`개수` / quantity), I (`지출(매수)` / line amount), and F (`내용` / product name) are filled. A child's `merchant_text` may differ from the header's. The header total equals the sum of header + child `line_amount` values.

| Bucket | Feb count | Handling |
|---|---|---|
| Total groups | 256 | |
| Multi-line groups | 7 (2.7%) | 2–17 lines per receipt. Golden case for unit-price tracking |
| Single-line groups (with memo) | 82 | Unit-price tracking possible (product mapping) |
| Single-line groups (no memo) | 167 | No product mapping; only merchant / category sums |

Three multi-line patterns:
- **Receipt decomposition**: row 17 이마트 (2 wines), row 71 인바이트 dessert (4 menu items), row 89 홈플러스 (6 items + kitchenware), row 127 풍림아이원 utility bill (17 line items).
- **Cost split-off**: rows 8 (화육면), 147 (동원식당), 159 (곳온니플레이스). Header + a single child whose category is "차감". These are amounts beyond household rules (e.g. "dining recognized up to 15,000 KRW per person") that are excluded from settlement and borne from personal allowance. Cross-checked in the summary sheet at R104·R114·R116·R118·R119 with the formula "approved − deduction = deposit" (Feb: 584,000 − 7,500 = 576,500).

---

## 0. Pre-work: Author CLAUDE.md

At the repo root `/Users/juno/dev/finance_mananger/CLAUDE.md`:

- "When implementing or modifying anything in the User domain, read [`MSA_INTEGRATION.md`](./MSA_INTEGRATION.md) first and follow it."
- "Auth server: `auth.junodevs.com` (auth-svc). JWKS: `https://auth.junodevs.com/auth/.well-known/jwks.json`."
- Four-line summary of the downstream rules: ① store `owner_id` (uuid) only, no FK ② do not replicate email / name ③ verify JWT EdDSA + iss / aud / exp / typ ④ refresh token in httpOnly cookie.

---

## 1. PostgreSQL 17 Schema

Core principle: **every domain table has `owner_id uuid NOT NULL` with no FK**. Imported data is stored in both `transactions_raw` (preserves the original) and `transactions` (normalized references).

```sql
-- Internal ledger users (joint / 엉아 / 아기) — labels independent of the login account.
-- 공동 (joint): joint spending between 엉아 (spouse) and the user.
-- 엉아 (spouse): spending for the spouse.
-- 아기 (baby): spending for the baby.
CREATE TABLE ledger_actors (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id    uuid NOT NULL,
  name        text NOT NULL,
  UNIQUE (owner_id, name)
);

CREATE TABLE categories (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL,
  parent_id    uuid REFERENCES categories(id),  -- intra-table hierarchy may use FK
  name         text NOT NULL,                    -- canonical name after normalization
  kind         text NOT NULL CHECK (kind IN ('income','expense')),
  review_state text NOT NULL DEFAULT 'pending'
               CHECK (review_state IN ('pending','confirmed'))
);
CREATE UNIQUE INDEX categories_owner_name_root_uniq 
  ON categories (owner_id, name) WHERE parent_id IS NULL;
CREATE UNIQUE INDEX categories_owner_parent_name_uniq 
  ON categories (owner_id, parent_id, name) WHERE parent_id IS NOT NULL;

CREATE TABLE merchants (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL,
  name         text NOT NULL,
  review_state text NOT NULL DEFAULT 'pending',
  UNIQUE (owner_id, name)
);

-- Product: a line item with a memo (column F `내용`), normalized into a stable identity. Key for unit-price tracking.
CREATE TABLE products (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL,
  merchant_id  uuid REFERENCES merchants(id),  -- usually NOT NULL, but NULL allowed to express "merchant-agnostic product"
  name         text NOT NULL,                   -- canonical product name after normalization
  review_state text NOT NULL DEFAULT 'pending'
               CHECK (review_state IN ('pending','confirmed'))
);
CREATE UNIQUE INDEX products_owner_merchant_name_uniq 
  ON products (owner_id, merchant_id, name) WHERE merchant_id IS NOT NULL;
CREATE UNIQUE INDEX products_owner_name_no_merchant_uniq 
  ON products (owner_id, name) WHERE merchant_id IS NULL;

-- Payment methods: there are no joint cards. Every payment method is owned by 엉아 or 아기.
-- Owned by 아기: 농협, 신한아기, 롯데, 삼성, 국민, 비씨, 현대, 현금아기.
-- Owned by 엉아: 현금, 신한, 하나, 씨티클, 현금엉아.
-- Summary sheet (rows 103–110): column G = 엉아 payment methods, column J = 아기 payment methods.
CREATE TABLE payment_methods (
  id        uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id  uuid NOT NULL,
  actor_id  uuid REFERENCES ledger_actors(id),  -- payment method owner (엉아 or 아기)
  name      text NOT NULL,
  UNIQUE (owner_id, name)
);

-- Aliases: imported raw text → normalized entity.
CREATE TABLE aliases (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL,
  scope        text NOT NULL CHECK (scope IN ('category','merchant','payment_method','actor','product')),
  raw_text     text NOT NULL,    -- exactly as it appeared
  norm_key     text NOT NULL,    -- after NFC + trim + lower + space/underscore unification
  target_id    uuid NOT NULL,
  UNIQUE (owner_id, scope, norm_key)
);
CREATE INDEX ON aliases (owner_id, scope, norm_key);

-- Import batch tracking (idempotency on re-import).
CREATE TABLE import_batches (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL,
  file_name    text NOT NULL,
  file_hash    bytea NOT NULL,    -- SHA-256
  year         int  NOT NULL,
  month        int  NOT NULL,
  row_count    int  NOT NULL,
  imported_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (owner_id, file_hash)
);

-- Preserves each Excel row verbatim (source of truth for the raw data).
CREATE TABLE transactions_raw (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id        uuid NOT NULL,
  import_batch_id uuid NOT NULL REFERENCES import_batches(id) ON DELETE CASCADE,
  row_index       int  NOT NULL,
  group_id        uuid NOT NULL,          -- key tying header and children of the same receipt
  is_group_header boolean NOT NULL,       -- true for the header row (column E `지출(합계)` filled)
  occurred_on     date,                   -- nullable in case of parse failure
  raw_date_serial double precision,
  merchant_text   text,
  actor_text      text,
  category_text   text,
  total_amount    numeric(15,2),          -- header only; null on children
  memo            text,
  unit_price      numeric(15,2),
  quantity        numeric(15,4),
  line_amount     numeric(15,2),
  payment_text    text,
  evidence_text   text,
  extras          jsonb                   -- catch-all for columns 11/12
);
CREATE INDEX ON transactions_raw (owner_id, occurred_on);
CREATE INDEX ON transactions_raw (owner_id, group_id);

-- Normalized transactions (source for dashboards/aggregation). Stored line-by-line.
CREATE TABLE transactions (
  id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id           uuid NOT NULL,
  raw_id             uuid NOT NULL REFERENCES transactions_raw(id) ON DELETE CASCADE,
  group_id           uuid NOT NULL,        -- mirrors transactions_raw.group_id
  occurred_on        date NOT NULL,
  merchant_id        uuid REFERENCES merchants(id),
  actor_id           uuid REFERENCES ledger_actors(id),
  category_id        uuid REFERENCES categories(id),
  product_id         uuid REFERENCES products(id),  -- only filled for rows with a memo
  payment_method_id  uuid REFERENCES payment_methods(id),
  amount             numeric(15,2) NOT NULL,   -- always positive (= abs(line_amount))
  sign               smallint NOT NULL CHECK (sign IN (-1, 1)), -- -1 = income (refund), 1 = expense
  unit_price         numeric(15,2),
  quantity           numeric(15,4),
  memo               text
);
CREATE INDEX ON transactions (owner_id, occurred_on DESC);
CREATE INDEX ON transactions (owner_id, category_id, occurred_on);
CREATE INDEX ON transactions (owner_id, merchant_id, occurred_on);
CREATE INDEX ON transactions (owner_id, product_id, occurred_on);
CREATE INDEX ON transactions (owner_id, group_id);
```

Notes:
- All amounts and unit prices are `numeric(15,2)` (or `numeric(15,4)` for unit price). Do not use `f64`.
- Excel serial → DATE: epoch is **1899-12-30** (avoids the 1900-02-29 bug).
- Negative expenses are stored with `sign = -1` (no separate table).
- **`transactions` row generation rules**:
  - **Single-line group**: header is itself the line → 1 row in `transactions`.
  - **Multi-line group**: the header gets its own row plus N rows for the children, totaling (1 + N) rows. (The header's `total_amount` overlaps with the child sum, but it is preserved for integrity verification and to keep parity with single-line storage.)
- **`"차감"` (deduction) category**: auto-created by the import pipeline (`kind='expense'`, `review_state='confirmed'`, protected). Stored with `sign=+1` to preserve receipt-sum integrity, but separated out at settlement time. Its contradictory nature — joint payment yet personally borne — is identified by category name.
- **Atomic upserts**: categories/products use partial unique indexes to enable `INSERT ... ON CONFLICT DO NOTHING` to work correctly with nullable columns. The four partial indexes (`categories_owner_name_root_uniq`, `categories_owner_parent_name_uniq`, `products_owner_merchant_name_uniq`, `products_owner_name_no_merchant_uniq`) are defined above and ensure race-safety under any isolation level.

### Settlement Flow and View (M2)

Monthly settlement flow:
1. Deposit the entire salary into the joint account.
2. Through the month, classify every expense by actor (joint / 엉아 / 아기) in Excel.
3. At month end, the summary sheet aggregates per actor — joint expenses split 50/50, personal expenses borne by each.
4. Reconcile the difference in a single transfer. Joint-category "차감" rows represent amounts above household rules (e.g. "dining recognized up to 15,000 KRW per person").
5. Excel summary formula: "approved expense − deduction = settlement deposit" (this settlement result).

The `v_monthly_settlement` view reproduces this settlement result in SQL. The key idea is to **figure out "who spent how much for whom"** and to fairly allocate joint spending.

#### Settlement view (M2)

Joint-settlement output with deduction split out — reproduces the summary sheet's "approved / deduction / deposit" formula in SQL.

```sql
CREATE VIEW v_monthly_settlement AS
SELECT
  t.owner_id,
  date_trunc('month', t.occurred_on)::date AS month,
  SUM(t.amount) FILTER (WHERE c.name != '차감' AND actor.name = '공동' AND t.sign = 1)
    AS recognized_expense,
  SUM(t.amount) FILTER (WHERE c.name = '차감')
    AS deducted_amount,
  (SUM(t.amount) FILTER (WHERE c.name != '차감' AND actor.name = '공동' AND t.sign = 1)
   - SUM(t.amount) FILTER (WHERE c.name = '차감'))
    AS settlement_input
FROM transactions t
JOIN categories c       ON c.id     = t.category_id
JOIN ledger_actors actor ON actor.id = t.actor_id
GROUP BY t.owner_id, date_trunc('month', t.occurred_on);
```

### Sum-integrity check (run once after import)

```sql
SELECT g.group_id,
       g.header_total,
       COALESCE(SUM(t.amount * t.sign), 0) AS lines_sum
FROM (SELECT group_id, total_amount AS header_total
      FROM transactions_raw
      WHERE is_group_header) g
LEFT JOIN transactions t USING (group_id)
GROUP BY g.group_id, g.header_total
HAVING g.header_total <> COALESCE(SUM(t.amount * t.sign), 0);
```

Zero rows = pass. Mismatched rows are surfaced as warnings in the import response and logs.

---

## 2. Rust Backend (`server/`)

### Directory layout

```
server/
  Cargo.toml
  src/
    main.rs
    config.rs
    db.rs              # sqlx::PgPool
    auth/
      mod.rs           # middleware (accepts both Authorization header and cookie)
      jwks.rs          # JWKS fetch + 5-minute TTL in-memory cache + single forced refresh on miss
      claims.rs        # iss / aud / exp / typ / EdDSA verification
    api/
      mod.rs           # axum Router
      import.rs        # POST /api/import (multipart xlsx)
      transactions.rs  # GET /api/transactions
      summary.rs       # GET /api/summary/:year/:month
      settlement.rs    # GET /api/settlement/:year/:month
      price.rs         # GET /api/price-history
      products.rs      # GET /api/products
      merchants.rs     # GET /api/merchant-stats
      aliases.rs       # GET/POST/DELETE /api/aliases, /api/review-queue
      categories.rs    # GET /api/categories, /api/merchants, /api/payment-methods
    import/
      xlsx.rs          # calamine parser, "M월" sheet extraction, serial→date
      grouping.rs      # detect header + children groups (assign group_id, decide is_group_header)
      normalize.rs     # NFC + trim + space/underscore unification → norm_key
      pipeline.rs      # raw insert → group validation → match attempts → unmatched rows go to review pending
    domain/            # model structs
    error.rs
```

### Dependencies (highlights)

`axum`, `tokio`, `tower-http` (CORS, Trace), `sqlx` (postgres, uuid, chrono, decimal, runtime-tokio, tls), `jsonwebtoken` (≥9, EdDSA), `calamine` (read-only xlsx), `rust_decimal`, `time` or `chrono`, `reqwest` (JWKS), `serde`, `serde_json`, `unicode-normalization`, `sha2`, `tracing` / `tracing-subscriber`.

> Rationale: `calamine` is lighter and faster than `umya-spreadsheet` for read-only xlsx. The schema is small enough that an ORM is unnecessary; `sqlx` compile-time query checking is the safer choice.

### Auth middleware essentials

1. On boot, fetch JWKS → in-memory cache (TTL 300 s).
2. Per request: read `Authorization: Bearer <t>` or `Cookie: Authorization=Bearer <t>`.
3. Verify: signature (EdDSA), `iss == "auth-svc"`, the `aud` array contains the service name (`finance-manager`), `exp` not expired, `typ == "access"`.
4. On verification failure (key mismatch), force one JWKS refetch and retry.
5. On pass, inject `Extension<AuthUser { sub, email, groups }>` into handlers. Use `sub` only as DB `owner_id`.
6. `kid` validation is disabled (no `kid` in the header).

### Endpoints (MVP)

| Method | Path | Description |
| --- | --- | --- |
| POST | /api/import | multipart with one .xlsx. Duplicate `file_hash` returns 409. Response includes the batch + group-sum integrity result + list of unmatched entities. |
| GET | /api/transactions | Filters: `?from=&to=&category=&actor=&merchant=&payment=&product=&group=`. Multi-line groups are bundled by `group_id`. |
| GET | /api/summary/:year/:month | Category × actor pivot (matches the Excel "M월(집계)" structure). |
| GET | /api/settlement/:year/:month | Joint-settlement card — `recognized_expense`, `deducted_amount`, `settlement_input` (mirrors "approved − deduction = deposit"). |
| GET | /api/price-history | `?product_id=` unit-price time series (only rows with a memo). |
| GET | /api/merchant-stats | `?merchant_id=` fallback for memo-less transactions. Monthly spend / count. |
| GET | /api/products | `?merchant_id=&q=` product list / search. |
| GET/POST/DELETE | /api/aliases | Alias add / delete. `?scope=category|merchant|payment_method|actor|product`. |
| GET | /api/review-queue | Unmatched raw text list (combined: category / merchant / payment / actor / product). |
| GET | /api/categories, /api/merchants, /api/payment-methods | Normalized entity lists. |

---

## 3. Normalization Strategy

`norm_key` generator (Rust):

1. Unicode NFC (avoids macOS file / clipboard NFD).
2. Trim ends, collapse runs of internal whitespace into a single space.
3. `_` → ` ` (treat underscore and space as equivalent).
4. Leave Hangul as-is; lowercase ASCII letters.

Import pipeline:

1. SHA-256 the .xlsx file → idempotent insert into `import_batches` (duplicate = 409).
2. Read the "M월" sheet row by row and **detect groups**:
   - A row with column E `지출(합계)` filled → assign a new `group_id`, set `is_group_header = true`.
   - A subsequent row sharing the same `occurred_on` and with column E empty is a child of the same group (`is_group_header = false`). The child's `merchant_text` may differ from the header's.
   - A different `occurred_on` or a new header ends the previous group.
3. Insert every row verbatim into `transactions_raw` (including `group_id` and `is_group_header`).
4. For each text column (category / merchant / actor / payment):
   - Compute `norm_key` → use `target_id` if there is a matching alias.
   - Otherwise look for a canonical entity with the same `norm_key` and create an alias automatically (`review_state = pending`).
   - If still nothing, create a new entity automatically (`review_state = pending`) and create the alias.
5. **Product mapping (memo-bearing rows only)**:
   - For a row with a memo (column F), look up product alias by `(merchant_id, norm_key(memo))` → match or create. Set `transactions.product_id`.
   - No memo → `product_id = NULL`. We do not ship a memo-edit UI later (the Excel original is the source of truth).
6. Generate `transactions` rows:
   - **Single-line group**: 1 header row. `amount = abs(total_amount)`, `sign` = original sign.
   - **Multi-line group**: 1 header row + N child rows = (1 + N) rows. `amount = abs(line_amount or total_amount)`, `sign` accordingly.
   - Rows in the "차감" category keep `sign = +1` (preserves receipt-sum integrity). Settlement output separates them in `v_monthly_settlement`.
7. Run the **sum-integrity SQL** → mismatched `group_id`s are surfaced as warnings in the import response and logs.
8. Unmatched (first-seen `norm_key`) entries are exposed via `/api/review-queue`. The user resolves each by ① merging into an existing entity or ② confirming as a new entity. On confirmation, `review_state = confirmed`. Product aliases follow the same flow.

> The catch-all data in columns 11/12 is preserved in `transactions_raw.extras` (jsonb) and otherwise ignored.
> Rows where the "증빙" column appears to contain a category name are logged as warnings and processed normally.
> Free-text household rules in the back of the summary sheet (`M월(집계)`) — "외식 15000까지", "커피 일 만원", etc. — are not auto-imported. A future page will let the user manage them. Out of scope for MVP.

---

## 4. Next.js Frontend (`web/`)

### Routes (App Router)

```
app/
  (auth)/login/page.tsx
  (app)/layout.tsx                  # sidebar + auth-middleware-protected area
  (app)/page.tsx                    # dashboard (current-month summary + settlement card + recent transactions)
  (app)/transactions/page.tsx       # filterable / sortable transactions table + group expand
  (app)/import/page.tsx             # xlsx upload + import result (group-sum integrity surfaced)
  (app)/aliases/page.tsx            # normalization / alias management + review queue (4 tabs: category / merchant / payment / product)
  (app)/price-history/page.tsx      # Products / Merchants toggle chart
middleware.ts                       # call /auth/refresh on access expiry, redirect to /login on failure
lib/api.ts                          # Rust API fetch wrapper (forwards cookies)
```

### Auth

- Call `/auth/login` (form-urlencoded) → response carries access + refresh.
- **Refresh** token: store via Next.js Route Handler with `Set-Cookie: refresh=...; HttpOnly; Secure; SameSite=Lax`.
- **Access** token: server components read it from the request cookies and forward `Cookie: Authorization=Bearer <access>` to the Rust API. Client components keep it in memory only when strictly needed.
- middleware.ts: if access is missing / expired, call `/auth/refresh`; on failure, redirect to `/login`.

### UI behavior highlights

- **`/aliases`**: 4 tabs (category / merchant / payment / **product**). Merging a product alias automatically updates `transactions.product_id` for affected rows.
- **`/price-history`**: header has a "Products / Merchants" toggle. Products shows a unit-price line chart of product-mapped transactions; Merchants shows monthly merchant totals including memo-less rows.
- **`/transactions`**: multi-line groups get a ▸ toggle on the header-styled row, expanding to reveal child lines. Rows in the "차감" category are dimmed and tagged with a "settlement deduction" badge.
- **Dashboard**: settlement card — "approved ₩XXX − deduction ₩X = deposit ₩XXX" (`/api/settlement/:year/:month`).

### UI libraries

- `shadcn/ui` (Radix-based) — forms / dialogs / sidebar.
- `@tanstack/react-table` — transactions table filter / sort / group expand.
- `recharts` — monthly bars, unit-price line.
- `tailwindcss` — styling.

---

## 5. Development Environment

`/Users/juno/dev/finance_mananger/`:

```
CLAUDE.md
MSA_INTEGRATION.md       (existing)
docker-compose.yml       (postgres:17 only)
server/                  (Rust)
web/                     (Next.js)
.env.example
2026년 02월.xlsx          (keep in place, or move to sample/)
```

`.env.example`:

```
DATABASE_URL=postgres://app:app@localhost:5432/finance
JWT_ISSUER=auth-svc
JWT_AUDIENCE=["finance-manager"]
JWKS_URL=https://auth.junodevs.com/auth/.well-known/jwks.json
AUTH_BASE_URL=https://auth.junodevs.com
SERVICE_NAME=finance-manager
BACKEND_CORS_ORIGINS=["http://localhost:3000"]
NEXT_PUBLIC_API_BASE_URL=http://localhost:8000
```

`docker-compose.yml` runs only `postgres:17`. Rust / Next run locally for fast iteration.

---

## 6. Milestones

**M1 — Bootstrap + import** (done 2026-04-25; criteria: uploading `2026년 02월.xlsx` inserts 177 rows into `transactions` (measured), the group-sum integrity SQL returns 0 rows, per-category sums match the Excel "2월(집계)" sheet exactly, all tests pass).
- CLAUDE.md, docker-compose, sqlx migrations (products table, group_id, product_id, v_monthly_settlement included).
- JWT middleware + JWKS cache.
- POST /api/import (calamine + group detection + normalization + raw store).
- Transactions list page (initial filters, group expand).

**M2 — Normalization UI + monthly dashboard + settlement card** (criteria: merging "이 마트" into "이마트" via the review queue updates `merchant_id` on existing transactions and immediately reflects in aggregates. Merging "조닌끼안티" / "조닌 끼안티" product aliases auto-remaps `product_id`. `v_monthly_settlement` returns `deducted_amount = 7,500` for February.)

**Step A (✅ 2026-05-02)**: Atomic upserts + read-only endpoints.
- Atomic upsert refactor in `server/src/import/pipeline.rs`: all entities (merchants, ledger_actors, payment_methods, categories, products) now use `INSERT ... ON CONFLICT DO NOTHING RETURNING` + fallback `SELECT`. Categories and products rely on the four partial unique indexes defined above to make ON CONFLICT atomic for nullable columns.
- Five new endpoints: `GET /api/categories`, `GET /api/merchants`, `GET /api/payment-methods` (with actor join), `GET /api/summary/:year/:month` (category × actor pivot, no deduction subtraction; `LEFT JOIN ledger_actors` to surface NULL actor as "(미지정)"), `GET /api/settlement/:year/:month` (v_monthly_settlement read-only).
- Tests: 49 passed (↑ 2 real concurrency tests with `tokio::sync::Barrier`); settlement test confirmed `deducted_amount = 7500` for Feb 2026.

**Step B (✅ 2026-05-02)**: Alias CRUD + review queue + auto-remap (backend).
- New module `server/src/api/aliases.rs` (530 lines) with 4 handlers: `GET /api/review-queue?scope=...`, `POST /api/aliases` (atomic create/merge), `DELETE /api/aliases/:id`, `POST /api/entities/:scope/:id/confirm`.
- Merge concurrency safeguard: row-level `SELECT ... FOR UPDATE` on source entity + alias re-read under lock; 409 on `alias_changed` if another merge moved it. 차감 category protected. Payment method merge rejects cross-actor targets.
- Tests: 60 passed (+11 new); concurrency test with `tokio::sync::Barrier` confirms single-winner semantics; regression test (golden file import + merge) confirms `v_monthly_settlement` unchanged.

**Original Step B spec (pending frontend implementation)**:
- New endpoints in `server/src/api/aliases.rs`:
  - `GET /api/review-queue?scope=category|merchant|payment_method|actor|product` — list entities with `review_state='pending'` (joined with their primary alias rows so the UI sees raw_text + norm_key + current target). Cross-scope when `?scope` omitted. Includes a `merge_candidates` field per row: other entities with the same `norm_key` ± edit-distance ≤ 1, scoped to the same owner.
  - `POST /api/aliases` — body `{scope, raw_text, target_id}`. Two behaviors in a single endpoint, both atomic in one transaction:
    1. **Create** when no alias exists for `(owner_id, scope, norm_key(raw_text))` → INSERT alias only.
    2. **Merge** when an alias exists with a different `target_id` → UPDATE alias.target_id, then `UPDATE transactions SET <scope>_id = $new WHERE owner_id = $o AND <scope>_id = $old`, then optionally DELETE the now-orphaned entity (only if no other alias points to it AND no transactions still reference it). Returns `{remapped_transaction_count, orphan_deleted: bool}`.
  - `DELETE /api/aliases/:id` — remove an alias row only. Does NOT touch transactions; the user has to merge or confirm to change mappings.
  - `POST /api/entities/:scope/:id/confirm` — flip `review_state` from `pending` to `confirmed`. "차감" is rejected (cannot be modified).
- `payment_method` merge has the extra wrinkle of `actor_id` ownership. PLAN domain rules pin every payment method to either 엉아 or 아기. Merging payment methods across actors is rejected with 409; the UI must surface the conflict.
- Concurrency: the merge path must hold a row-level `SELECT ... FOR UPDATE` on the source entity row before the alias UPDATE so two concurrent merges into different targets cannot both succeed.
- Tests: merging "이 마트" → "이마트" updates `transactions.merchant_id` for all affected rows and the orphaned merchant row is deleted; merging "조닌끼안티" / "조닌 끼안티" products auto-remaps `product_id` (PLAN §6 M2 acceptance criteria); confirm endpoint refuses "차감"; concurrent two-merge race resolves to a single winner with the second 409-ing.
- Acceptance: cargo test green; the M2 acceptance criteria for merge cases pass; `v_monthly_settlement` numbers do not drift after a merge (regression check).

**Step C (✅ 2026-05-02)**: Frontend `/aliases` page — 4 tabs (category / merchant / payment / product).
- Real page replaces the placeholder. Server component (`web/app/(app)/aliases/page.tsx`) fetches the review queue per tab; client component (`web/components/aliases-tab-content.tsx`) handles interactions. Token isolation enforced via two server-side proxy routes: `web/app/api/aliases-proxy/route.ts` (POST merge / DELETE alias) and `web/app/api/entities-proxy/[scope]/[id]/confirm/route.ts` (confirm). The access token never reaches the client bundle.
- Actions implemented: Confirm as new (optimistic), Merge into existing (native `<select>` of candidates from `merge_candidates`, then full list), Delete alias (with confirmation dialog).
- Schemas added in `web/lib/schemas.ts`: `AliasInfoSchema`, `MergeCandidateSchema`, `ReviewQueueItemSchema`, `ReviewQueueResponseSchema`, `PostAliasResponseSchema`, `ConfirmEntityResponseSchema`.
- shadcn primitives added: `web/components/ui/tabs.tsx` (Radix Tabs), `web/components/ui/dialog.tsx` (Radix Dialog).
- **Backend 409 contract refactor (companion change to Step C)**: `AppError::Conflict(String)` → `AppError::Conflict(serde_json::Value)`. All 409 responses from `server/src/api/aliases.rs` now return `{error, message, ...extras}`. Codes: `actor_mismatch` (with `source_actor`/`target_actor`), `alias_changed` (with `target_id` if available), `deduction_protected`, `same_target`, plus `duplicate_record` (sqlx 23505) and `duplicate_import` (`server/src/api/import.rs`). Frontend reads `error` field directly — no regex parsing of prose. Updated tests: `server/tests/test_m2_step_b.rs` (cross-actor merge asserts on structured fields), `server/tests/test_owner_isolation.rs` (sqlx unique violation maps to `duplicate_record`).
- Tests: frontend 69 passed (was 58; +11 in `web/__tests__/aliases.test.tsx` covering merge dialog interaction, 409 actor_mismatch with structured shape, DELETE error path, tab switching). Backend 60 passed in `--test-threads=1` and isolation.
- **Concurrency fix (✅ 2026-05-02)**: `POST /api/aliases` now takes an optional `source_id` field. The handler acquires `SELECT ... FOR UPDATE` on the alias as the first SQL operation in the transaction, then asserts `alias.target_id == source_id` (when provided) — mismatch returns 409 `alias_changed`. This deterministically rejects both fully-concurrent and sequential races: two merges of the same source into different targets always resolve to one 200 + one 409 regardless of scheduler timing. Frontend always sends `source_id = item.id`. The previous Phase 1 / Phase 2 snapshot-diff is removed as redundant. Verified by 3 consecutive parallel-mode runs of `cargo test --test test_m2_step_b` (11/11 each).
- **Known limitation (not blocking Step C acceptance)**: `GET /api/review-queue?scope=payment_method` always returns `[]` because the `payment_methods` table has no `review_state` column (PLAN §1 schema). The Payment tab renders correctly but currently has no functional purpose until either (a) a migration adds `review_state` to `payment_methods` or (b) the Payment tab is removed from the UI. Defer to user decision.

**Step D (pending)**: Frontend dashboard (summary + settlement card) at `web/app/(app)/page.tsx`.
- Month picker at top (default = current calendar month). State stored in URL search params so the dashboard is shareable / refresh-stable.
- **Settlement card** (top): three figures from `GET /api/settlement/:year/:month` rendered as "approved ₩XXX − deduction ₩X = deposit ₩XXX". When `recognized_expense = 0` (no data for the month), the card collapses to "No settlement data for <month>.".
- **Category × Actor pivot table** (middle): rows = categories, columns = actors (공동, 엉아, 아기, plus "(미지정)" if any), cells = `Decimal` formatted as KRW. Total column on the right, total row on the bottom. 차감 is shown as a normal pivot row — not subtracted (the settlement card already separates it).
- **Recent transactions** (bottom): last 10 rows from `GET /api/transactions?from=<month-start>&to=<month-end>`, with the same group-expand behavior as the existing `/transactions` page (reuse the table component if practical).
- Loading states: skeletons while server components fetch.
- Tests: vitest covers the month-picker → URL sync, the empty-month settlement card, and one snapshot of the populated pivot for Feb 2026 mocked data.
- Acceptance: with the golden file imported, the Feb 2026 dashboard shows approved 584,000, deduction 7,500, deposit 576,500 (PLAN §0 cross-check); the pivot shows 차감 as a normal row totaling 7,500.

**M3 — Price tracking + merchant statistics + multi-month aggregation** (criteria: 6 occurrences of 고덕방 iced americano are grouped at 3,400 KRW each, displayed as a unit-price time series. The 167 memo-less rows are surfaced separately as monthly per-merchant totals.)
- /api/price-history (per product).
- /api/merchant-stats (fallback for memo-less rows).
- /price-history page Products / Merchants toggle.
- Multi-month comparison chart.
- xlsx export deferred to a follow-up if needed.

---

## 7. Key Files / Reuse

This is a new project; no existing code to reuse. External references:

- Auth contract — `MSA_INTEGRATION.md` (required reading for User implementation, called out in CLAUDE.md).
- Source data — `2026년 02월.xlsx` (M1 import golden case).

Files to create / modify:

- `CLAUDE.md` (new).
- `docker-compose.yml` (new).
- `.env.example` (new).
- `server/` (entire new cargo project).
- `web/` (entire new next project).

---

## 8. Verification (M1 exit)

1. `docker compose up -d postgres` → `sqlx migrate run` succeeds.
2. `cargo run -p server` startup logs the JWKS fetch.
3. Obtain an access token from auth.junodevs.com (curl, form-urlencoded login) → calling `/api/transactions` with `Authorization: Bearer ...` returns 200 (empty array), not 401.
4. `pnpm dev` → log in via /login → upload `2026년 02월.xlsx` from /import.
5. SQL checks:
   - `SELECT COUNT(*) FROM transactions WHERE owner_id = $sub;` = 282 (single-line 249 + multi-line children 33).
   - `SELECT COUNT(*) FROM transactions WHERE product_id IS NULL;` ≈ 167 (matches the count of memo-less single-line rows).
   - Per-category sums match the Excel "(집계)" sheet.
6. Transactions list page: filter / sort / group expand all work.
7. **Group-integrity SQL** (§1 verification SQL) → 0 rows. Children of the 7 multi-line groups sum to the header total.
8. `SELECT name, COUNT(*), array_agg(unit_price ORDER BY occurred_on) FROM transactions t JOIN products p ON p.id = t.product_id JOIN merchants m ON m.id = t.merchant_id WHERE p.name='아이스아메리카노' AND m.name='고덕방' GROUP BY name;` → 6 rows, all 3,400.
9. In the `/aliases` product tab, merging two products with the same merchant but different memo variants immediately consolidates `transactions.product_id` (verify in UI).
10. `SELECT * FROM v_monthly_settlement WHERE month = '2026-02-01';` → `deducted_amount = 7500`, `settlement_input = recognized_expense - 7500` (cross-check with summary sheet R118·R119).
