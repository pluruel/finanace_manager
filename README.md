# finance_manager

Unified ledger viewer for monthly Excel files (`YYYY년 MM월.xlsx`). Imports each month into PostgreSQL, normalizes categories / merchants / products, and surfaces unit-price time series and joint settlement.

- Backend: Rust (axum) + SeaORM 1.x + calamine
- Frontend: Next.js 15 App Router + shadcn/ui + tanstack-table + recharts
- DB: PostgreSQL 17 (single in-place SeaORM migration; `Migrator::up` runs at server boot)
- Auth: external `auth.junodevs.com` (EdDSA JWT, MSA)

For working rules and domain notes, see [`CLAUDE.md`](./CLAUDE.md). For the auth contract, see [`MSA_INTEGRATION.md`](./MSA_INTEGRATION.md). Active design/plan docs live under [`docs/superpowers/`](./docs/superpowers/).

## Status — MVP complete

M1 ~ M4 all complete (2026-05-03). Subsequent iterations: dashboard donut redesign, income/expense split + signed-amount convention, actor income donut, SeaORM migration (2026-05-11).

- `server/` (Rust + axum + SeaORM): `/health`, `POST /api/import`, `GET /api/transactions`, `/api/categories`, `/api/aliases`, `/api/review-queue`, `/api/summary/:y/:m`, `/api/summary/income/:y/:m`, `/api/settlement/:y/:m`, `/api/price-history`, `/api/products`, `/api/merchant-stats`, `/api/export/:y/:m`.
- `web/` (Next.js 15): `/login`, `/(app)/` dashboard (settlement strip + actor donuts), `/transactions`, `/import`, `/aliases` (4 tabs), `/price-history` (Products / Merchants toggle).
- Schema: single SeaORM migration at `server/migration/src/m20260510_000001_init.rs` (full schema + pgcrypto + `v_monthly_settlement`). Entities under `server/src/entity/` are auto-generated via `sea-orm-cli`.
- Golden file `2026년 02월.xlsx`: 177 rows imported, group-sum integrity 0 mismatches, `v_monthly_settlement.deducted_amount = 7500` matches the Excel summary.
- Tests: backend `cargo test -p finance-manager` (87 passed), frontend `npm test` (126 passed).

## Quick Start

### 1. Configure environment

```bash
cp .env.example .env
# edit .env if your auth setup differs from the defaults
```

### 2. Run with Docker Compose (recommended)

```bash
docker compose up -d --build
```

Brings up `postgres`, `server` (port 8000), and `web` (port 3000). The server runs `Migrator::up` automatically on boot — no separate migration step needed.

Health check:
```bash
curl http://localhost:8000/health
open http://localhost:3000
```

### 3. Run locally against compose-managed Postgres

If you prefer cargo / npm for fast iteration:

```bash
docker compose up -d postgres

# backend
cd server
DATABASE_URL=postgres://app:app@localhost:5432/finance cargo run -p finance-manager

# frontend (in another shell)
cd web
npm install
npm run dev
```

### Schema changes

Single in-place migration policy. After editing `server/migration/src/m20260510_000001_init.rs`:

```bash
# Reapply against the dev DB
cargo run -p migration -- -u "$DATABASE_URL" fresh

# Regenerate entities
sea-orm-cli generate entity -u "$DATABASE_URL" -o server/src/entity --with-serde both
```

## Tests

```bash
# backend (creates an ephemeral test DB per test; DATABASE_URL must reach a Postgres admin DB)
DATABASE_URL=postgres://app:app@localhost:5432/postgres cargo test -p finance-manager

# frontend
cd web && npm test
```

## Layout

```
finance_mananger/
  CLAUDE.md                                # working rules, domain notes, cumulative context
  MSA_INTEGRATION.md                       # auth contract (required reading for User domain)
  docker-compose.yml                       # postgres + server + web
  .env.example
  docs/superpowers/                        # specs + plans (per-iteration design docs)
  server/                                  # Rust (axum) backend
    Cargo.toml
    src/                                   # api/, auth/, import/, entity/, domain/, db.rs, main.rs
    migration/                             # SeaORM migration sub-crate (single in-place file)
    tests/                                 # integration tests (ephemeral DB per test)
  web/                                     # Next.js 15 App Router frontend
  2026년 02월.xlsx                          # golden import case (M1 reference)
```
