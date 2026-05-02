# finance_manager

Unified ledger viewer for monthly Excel files (`YYYY년 MM월.xlsx`). Imports each month into PostgreSQL, normalizes categories / merchants / products, and surfaces unit-price time series and joint settlement.

- Backend: Rust (axum) + sqlx + calamine
- Frontend: Next.js 15 App Router + shadcn/ui + tanstack-table + recharts
- DB: PostgreSQL 17
- Auth: external `auth.junodevs.com` (EdDSA JWT, MSA)

For the full design, schema, endpoints, and milestones, see [`PLAN.md`](./PLAN.md). For working rules and domain notes, see [`CLAUDE.md`](./CLAUDE.md). For the auth contract, see [`MSA_INTEGRATION.md`](./MSA_INTEGRATION.md).

## Status — runnable

M1 is complete and runnable end-to-end:

- `server/` builds and serves `/health`, `POST /api/import` (xlsx, multipart, 20 MB cap, SHA-256 idempotency, single transaction), and `GET /api/transactions` (filters, grouped response, recursive children).
- `web/` builds and serves `/login`, `/(app)/` dashboard, `/transactions`, `/import`, plus M2/M3 placeholder routes (`/aliases`, `/price-history`).
- Migrations: `server/migrations/001_init.sql` (full schema + pgcrypto, includes `v_monthly_settlement`).
- Verified against the golden file `2026년 02월.xlsx`: 177 rows imported, group-sum integrity 0 mismatches, `v_monthly_settlement.deducted_amount = 7500` matches the Excel summary.
- Tests: backend `cargo test -p server` (34 passed), frontend `npm test` (58 passed).

M2 (alias / review-queue UI, monthly dashboard, settlement card) and M3 (price history, merchant stats, multi-month) are not yet implemented — corresponding routes are placeholders.

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

Brings up `postgres`, `server` (port 8000), and `web` (port 3000). The server runs migrations on startup.

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
DATABASE_URL=postgres://app:app@localhost:5432/finance cargo run -p server

# frontend (in another shell)
cd web
npm install
npm run dev
```

## Tests

```bash
# backend (creates an ephemeral test DB; DATABASE_URL must point to a reachable Postgres)
cd server && cargo test -p server

# frontend
cd web && npm test
```

## Layout

```
finanace_manager/
  CLAUDE.md            # working rules and domain notes
  MSA_INTEGRATION.md   # auth contract (required reading for User domain)
  PLAN.md              # full design — single source of truth
  docker-compose.yml   # postgres + server + web
  .env.example
  server/              # Rust (axum) backend
  web/                 # Next.js 15 App Router frontend
  2026년 02월.xlsx      # M1 import golden case
```
