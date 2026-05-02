# Finance Manager — Unified Ledger Viewer (finance_mananger)

A unified viewer that accumulates monthly Excel files (`YYYY년 MM월.xlsx`) into PostgreSQL, normalizes categories / merchants / products, and surfaces unit-price time series and settlement. Input continues to come from Excel.

## Workflow

For any code-changing task, follow this order:

1. **Implement** — Write or modify backend (`server/`) and frontend (`web/`) code directly.
2. **Review** — After changes, self-check for quality, security, MSA contract violations, and domain-rule violations.
3. **Test** — Run `cargo test -p server` for the backend and `npm test` for the frontend, and confirm they pass.
4. **Document** — When the implementation state changes, update CLAUDE.md to match.

Do not move on to the next task until review and tests have passed.

---

## Authentication (MSA, Required Reading)

When implementing or modifying anything in the User domain, read [`MSA_INTEGRATION.md`](./MSA_INTEGRATION.md) first and follow it.

- **Auth server**: `auth.junodevs.com` (auth-svc)
- **JWKS**: `https://auth.junodevs.com/auth/.well-known/jwks.json`
- **Service name (aud)**: `finance-manager`

Downstream rules (do not violate):
1. Store only `owner_id uuid`; **no FK** to the auth-svc DB.
2. **Do not replicate** user info (email, name, groups, etc.). Use JWT claims or `/auth/me` if needed.
3. Verify JWT with **EdDSA**, ensuring `iss=auth-svc`, `aud` array contains `finance-manager`, `exp` is not expired, and `typ=access`.
4. Refresh tokens go in **httpOnly + Secure + SameSite cookies only** (no localStorage).

---

## Architecture

```
finance_mananger/
  CLAUDE.md
  MSA_INTEGRATION.md
  PLAN.md                    # initial implementation plan (single source of truth)
  docker-compose.yml         # postgres:17 + server + web
  .env.example
  server/                    # Rust (axum) backend
  web/                       # Next.js 15 App Router frontend
  2026년 02월.xlsx            # M1 import golden case
```

### Backend (`server/`, Rust + axum)
- DB: PostgreSQL 17, `sqlx` with compile-time query checking.
- xlsx reading: `calamine`.
- JWT: `jsonwebtoken` (≥9, EdDSA) + 5-minute in-memory JWKS cache + a single forced refresh on verification failure.
- Detailed directory layout / endpoints / schema: see [PLAN.md §1·§2](./PLAN.md).

### Frontend (`web/`, Next.js 15 App Router)
- UI: shadcn/ui + tailwindcss.
- Tables: `@tanstack/react-table` (multi-line group expand supported).
- Charts: `recharts`.
- Auth: middleware.ts calls `/auth/refresh` on access expiry; redirects to `/login` on failure.
- Detailed routes: see [PLAN.md §4](./PLAN.md).

### M2 Implementation Status
M2 Steps A/B/C/D are all complete (2026-05-02). For full per-step details — endpoints, files touched, test counts, known limitations — see [PLAN.md §6](./PLAN.md). Do not duplicate that content here.

---

## Deployment — Docker Compose

The application **deploys via docker compose**. All services (postgres, server, web) must come up under compose, and local dev supports both running everything in compose and running cargo / pnpm against the compose-managed postgres.

`docker-compose.yml` (deployment config):
- `postgres`: postgres:17, volume-mounted, `DATABASE_URL` aligned.
- `server`: builds from `server/Dockerfile`, reads `.env`, depends on postgres.
- `web`: builds from `web/Dockerfile`, points at the server via `NEXT_PUBLIC_API_BASE_URL`.

`.env.example`:
```
DATABASE_URL=postgres://app:app@postgres:5432/finance
JWT_ISSUER=auth-svc
JWT_AUDIENCE=["finance-manager"]
JWKS_URL=https://auth.junodevs.com/auth/.well-known/jwks.json
AUTH_BASE_URL=https://auth.junodevs.com
SERVICE_NAME=finance-manager
BACKEND_CORS_ORIGINS=["http://localhost:3000"]
NEXT_PUBLIC_API_BASE_URL=http://localhost:8000
```

Deployment / run flow:
1. `docker compose build`.
2. `docker compose up -d postgres`, then `sqlx migrate run` (or run automatically from the server container's entrypoint).
3. `docker compose up -d server web`.
4. Health checks: server `/health` returns 200, web `/` renders.

### Python Environment
- Uses `uv`; the virtualenv lives in `.venv`. Run with `uv run` or `.venv/bin/python`.

### Migration Policy
- Do not let migration SQL files accumulate. When the schema changes, delete the existing file and rewrite it.
- If a separate migration (e.g. `ALTER TABLE`) is required, the user will request it explicitly.

### How to Run Tests
- **Backend**: `cd server && cargo test -p server` (requires `DATABASE_URL`; an ephemeral test DB is created automatically).
- **Frontend**: `cd web && npm test` (vitest, 58 tests).

---

## Core Domain Rules (extracted from PLAN)

One Excel row is **not** equivalent to one transaction. A single receipt may decompose into a header + N child rows, forming a multi-line group.

### Internal ledger users (`ledger_actors`)
- The `ledger_actors` table is unrelated to the login account; it represents the **target of spending**.
  - **공동 (joint)**: not a third party — joint spending shared between 엉아 (spouse) and the user.
  - **엉아 (spouse)**: spending for the spouse.
  - **아기 (baby)**: spending for the baby.
- Month-end settlement covers all actors (not just joint).

### Settlement Flow (every month)
1. Deposit the entire salary into the joint account.
2. Throughout the month, classify every expense by actor (joint / spouse / baby) in Excel.
3. At month end, the Excel summary sheet sums per actor — joint expenses split 50/50, personal expenses borne by each.
4. Reconcile the difference in a single transfer. Joint-category "차감" rows represent amounts beyond household rules (e.g. "dining recognized up to 15,000 KRW per person").
5. Excel summary formula: "approved expense − deduction = settlement deposit".
- **Key**: the `v_monthly_settlement` view captures "who spent how much for whom" and computes the fair allocation of joint spending.

### Payment methods and ownership (`payment_methods` → actor mapping)
- There are no joint cards. Every payment method is owned by **either 엉아 or 아기**.
- **Owned by 아기**: 농협, 신한아기, 롯데, 삼성, 국민, 비씨, 현대, 현금아기.
- **Owned by 엉아**: 현금, 신한, 하나, 씨티클, 현금엉아.
- Excel summary sheet (rows 103–110): column G = 엉아 payment methods, column J = 아기 payment methods.
- `actor_id` will be added to `payment_methods` (M2 migration).

### Transaction data
- Every domain table carries `owner_id uuid NOT NULL`; no FK to auth-svc.
- Money is stored as `numeric(15,2)`. Do not use `f64`.
- Excel serial → DATE: epoch is **1899-12-30** (avoiding the 1900-02-29 bug).
- Negative expenses are stored with `sign = -1` (no separate table).
- Single-line groups produce 1 row in `transactions`. Multi-line groups produce 1 header + N child rows = (1 + N) rows.
- The `"차감"` (deduction) category is auto-created by the import pipeline (`kind='expense'`, `review_state='confirmed'`, protected). It is stored with `sign=+1`, but the `v_monthly_settlement` view separates it during settlement calculation.

For the full schema, endpoints, normalization pipeline, and milestones, see [PLAN.md](./PLAN.md). **PLAN.md is the single source of truth** — when in conflict, follow PLAN.

---

## Milestone Summary

- **M1**: Bootstrap + import — ✅ done (2026-04-25). 177 rows inserted from `2026년 02월.xlsx`, group-sum integrity 0 rows, tests passing.
- **M2**: Normalization UI + monthly dashboard + settlement card — ✅ done (2026-05-02). Steps A/B/C/D all green; backend 62 / frontend 79 tests passing.
- **M3**: Price tracking + merchant statistics + multi-month aggregation.

---

## Cumulative Context (Documentation Agent)

- 2026-05-02: M2 Step B complete — alias CRUD, review queue, auto-remap backend done; merge uses SELECT FOR UPDATE + alias re-read under lock for race safety (memoed in project MEMORY.md for future reference)
- 2026-05-02: M2 Step D complete — dashboard at `(app)/page.tsx` with month picker (URL `?ym=YYYY-MM`), settlement card, category × actor pivot, recent transactions. New components: `month-picker.tsx`, `settlement-card.tsx`, `summary-pivot.tsx`. Frontend tests 79/79 (10 new in `dashboard.test.tsx`); backend 62/62.
