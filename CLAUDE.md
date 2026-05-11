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
  docker-compose.yml         # postgres:17 + server + web
  .env.example
  server/                    # Rust (axum) backend
  web/                       # Next.js 15 App Router frontend
  2026년 02월.xlsx            # M1 import golden case
```

### Backend (`server/`, Rust + axum)
- DB: PostgreSQL 17, SeaORM 1.x (entity-driven; raw SQL via `FromQueryResult` for views and complex aggregates).
- xlsx reading: `calamine`.
- JWT: `jsonwebtoken` (≥9, EdDSA) + 5-minute in-memory JWKS cache + a single forced refresh on verification failure.

### Frontend (`web/`, Next.js 15 App Router)
- UI: shadcn/ui + tailwindcss.
- Tables: `@tanstack/react-table` (multi-line group expand supported).
- Charts: `recharts`.
- Auth: middleware.ts calls `/auth/refresh` on access expiry; redirects to `/login` on failure.

### M2 Implementation Status
M2 Steps A/B/C/D all complete (2026-05-02). Alias CRUD + review queue + auto-remap backend, frontend `/aliases` page (4 tabs), dashboard with month picker / settlement card / category × actor pivot.

### M3 Implementation Status
M3 (price tracking + merchant stats) complete (2026-05-03). Backend: 3 new endpoints (`/api/products`, `/api/price-history`, `/api/merchant-stats`) in `server/src/api/{products,price,merchant_stats}.rs`. Frontend: `/price-history` page with Products / Merchants toggle (`web/app/(app)/price-history/page.tsx` + 3 new components). Backend 71 tests, frontend 86 tests. Multi-month comparison deferred until a second month of data is imported.

### M4 Implementation Status (MVP close-out)
M4 complete (2026-05-03). Three sub-steps: **M4-A** (`payment_methods.review_state` migration; `/api/review-queue?scope=payment_method` and confirm now functional — resolves the prior Payment-tab dead-end); **M4-B** (`GET /api/export/:year/:month` xlsx endpoint via `rust_xlsxwriter` with Transactions / Settlement / Summary sheets, plus dashboard "Excel 다운로드" link routed through `web/app/api/export-proxy/[year]/[month]/route.ts`); **M4-C** (doc cleanup). Backend 76 tests (+5: 2 in `test_m2_step_b.rs` for payment-method confirm/queue, 3 in new `test_m4_export.rs`); frontend 86 tests. Multi-month overlay and household-rules page remain deferred.

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
- The schema lives in a single migration file at `server/migration/src/m20260510_000001_init.rs`. When the schema changes, edit this file in place; do not accumulate migrations.
- Dev workflow: rerun `Migrator::fresh` against the dev DB after edits (`cargo run -p migration -- fresh -u "$DATABASE_URL"`). Production runs `Migrator::up` at server boot.
- Entities live under `server/src/entity/`, generated by `sea-orm-cli generate entity -u "$DATABASE_URL" -o server/src/entity --with-serde both`. Treat that directory as auto-generated and regenerate after every migration edit.

### How to Run Tests
- **Backend**: `cd server && cargo test -p server` (requires `DATABASE_URL`; an ephemeral test DB is created automatically).
- **Frontend**: `cd web && npm test` (vitest).

### Subagent Model Policy
When running subagent-driven development:
- **Implementer subagents**: use `sonnet`.
- **Reviewer subagents** (spec compliance, code quality, final review): use `opus`.

---

## Core Domain Rules

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
- `transactions.amount` is a **signed cash-flow value**: cash-in positive, cash-out negative. Excel is an expense ledger (지출 양수, 환불 음수); the importer flips the sign at write time. There is no separate `sign` column.
- Income vs expense classification lives in **`categories.kind`** (`'income'` | `'expense'`) — driven by the user via the `/aliases` Categories tab toggle. Refunds and `차감` are NOT income; they remain `kind='expense'` with their own row-level sign expressing direction.
- Single-line groups produce 1 row in `transactions`. Multi-line groups produce 1 header + N child rows = (1 + N) rows.
- The `"차감"` (deduction) category is auto-created by the import pipeline (`kind='expense'`, `review_state='confirmed'`, protected). It is stored with the same sign-flip rule as other rows; the `v_monthly_settlement` view isolates it via category name during settlement calculation.

---

## Milestone Summary

- **M1**: Bootstrap + import — ✅ done (2026-04-25). 177 rows inserted from `2026년 02월.xlsx`, group-sum integrity 0 rows, tests passing.
- **M2**: Normalization UI + monthly dashboard + settlement card — ✅ done (2026-05-02). Steps A/B/C/D all green; backend 62 / frontend 79 tests passing.
- **M3**: Price tracking + merchant statistics + multi-month aggregation — ✅ done (2026-05-03). `/api/products`, `/api/price-history`, `/api/merchant-stats`; `/price-history` page Products / Merchants toggle. Backend 71 / frontend 86 tests passing. Acceptance: 6 고덕방 아이스아메리카노 rows show ₩3,400 each.
- **M4**: MVP close-out — ✅ done (2026-05-03). Payment-method review queue (M4-A), xlsx export (M4-B), doc cleanup (M4-C). Backend 76 / frontend 86 tests passing. **MVP is now complete.**

---

## Cumulative Context (Documentation Agent)

- 2026-05-02: M2 Step B complete — alias CRUD, review queue, auto-remap backend done; merge uses SELECT FOR UPDATE + alias re-read under lock for race safety (memoed in project MEMORY.md for future reference)
- 2026-05-02: M2 Step D complete — dashboard at `(app)/page.tsx` with month picker (URL `?ym=YYYY-MM`), settlement card, category × actor pivot, recent transactions. New components: `month-picker.tsx`, `settlement-card.tsx`, `summary-pivot.tsx`. Frontend tests 79/79 (10 new in `dashboard.test.tsx`); backend 62/62.
- 2026-05-03: M3 complete — 3 new backend modules (`server/src/api/{products,price,merchant_stats}.rs`) wired into the router (M1 `stubs.rs` deleted), plus `/price-history` page with Products / Merchants toggle (`web/app/(app)/price-history/page.tsx`, `web/components/{price-history-controls,price-history-chart,merchant-stats-chart}.tsx`). Recharts mocked in `web/__tests__/price-history.test.tsx`. Backend 71/71 (9 new in `tests/test_m3.rs`), frontend 86/86 (7 new). Acceptance: 6 고덕방 아이스아메리카노 rows render at ₩3,400 each. Memo-less row count: actual 64 in normalized `transactions` (memo→product mapping is aggressive).
- 2026-05-03: M4 (MVP close-out) complete — (A) `001_init.sql` rewritten to add `review_state` to `payment_methods` (per "rewrite, don't accumulate" migration policy); `aliases.rs` review_queue + confirm now handle `payment_method` scope, resolving the prior dead-Payment-tab limitation. `categories.rs::PaymentMethodItem` exposes `review_state`. (B) New `server/src/api/export.rs` + `rust_xlsxwriter = "0.78"` produce a 3-sheet xlsx (Transactions / Settlement / Summary) at `GET /api/export/:year/:month`; frontend proxy `web/app/api/export-proxy/[year]/[month]/route.ts` + dashboard "Excel 다운로드" link in `(app)/page.tsx`. (C) Doc cleanup. Backend 76/76 (+5: 2 in `test_m2_step_b.rs`, 3 in new `test_m4_export.rs`); frontend 86/86 (no new tests — proxy route is a thin pass-through). MVP complete.
- 2026-05-07: Dashboard donut redesign — `(app)/page.tsx` now renders a compact `SettlementCard` strip + `DashboardDonuts` grid (one `ActorDonut` per actor, top-6 categories + 기타 + 차감 pinned). New: `web/lib/donut-data.ts` (pure `buildActorSlices` + `collectOrderedActorIds` helpers), `web/components/{actor-donut,dashboard-donuts}.tsx`. Removed: `summary-pivot.tsx`, recent-transactions section. `SettlementCard` gained an opt-in `compact` prop. Frontend tests 102/102 (+16: 13 in `donut-data.test.ts`, 3 new `DashboardDonuts` cases in `dashboard.test.tsx`); recharts mocked out as in `price-history.test.tsx`. No backend changes. Spec/plan: `docs/superpowers/{specs,plans}/2026-05-07-dashboard-donuts*`.
- 2026-05-07: Income/expense split + signed-amount convention — `transactions.sign` column dropped; `transactions.amount` is now a signed cash-flow value (cash-in positive, cash-out negative). Importer flips Excel sign at write time. `categories.kind` is the sole income/expense classifier; refunds and `차감` stay as `kind='expense'`. `v_monthly_settlement` rewritten with `-SUM(amount)` and `kind='expense'` filter. New endpoints: `GET /api/summary/income/:year/:month` (per-actor income totals, zero-filled) and `PATCH /api/categories/:id/kind` (with `차감` 409 protection). Frontend: new `IncomeStrip` between SettlementCard and DashboardDonuts; `/aliases` Categories tab adds inline 수입/지출 Switch (Radix); `signedNumber` helper retired in favor of `parseFloat(amount)`. `001_init.sql` rewritten in place per migration policy; existing data wiped + re-import required. Backend 81/81 (+5: 2 income, 3 kind-toggle), frontend 110/110 (+8: 3 income-strip, 4 kind-toggle, 1 dashboard smoke). Spec/plan: `docs/superpowers/{specs,plans}/2026-05-07-income-expense-sign-*`.
- 2026-05-08: Dashboard 수입/지출 시각 분리 — 도넛 카드는 가구합계/아기/엉아 3장 고정 순서로 expense 만 표시(파란 팔레트), 수입은 카드 헤더에 빨간 텍스트로 흡수, 차감은 별도 도넛 카드로 분리(액터 슬라이스). `IncomeStrip` 컴포넌트는 제거. 백엔드는 importer 휴리스틱(`급여|수입|회수|환급`) 으로 신규 카테고리 `kind='income'` 자동 분류, ON CONFLICT DO NOTHING 으로 사용자 토글 보존. 신규/수정: `web/components/{actor-donut,dashboard-donuts,deduction-donut}.tsx`, `web/lib/donut-data.ts` (4 함수 + EXPENSE_PALETTE/DEDUCTION_PALETTE/INCOME_COLOR), `server/src/import/pipeline.rs::upsert_category`. 테스트: 백엔드 +3 (`tests/test_import_kind_heuristic.rs`), 프런트 donut-data 16개 / dashboard ActorDonut 6개 / DashboardDonuts 5개 / DeductionDonut 2개 (총 116). Spec/plan: `docs/superpowers/{specs,plans}/2026-05-08-dashboard-income-expense-redesign*`.
- 2026-05-08: 보험 부호 분리 — Excel "보험" 카테고리는 양/음수가 섞여 있어(보험료 지출 vs 보험금 수령) 카테고리 단위 `kind` 로 표현 불가. importer (`run_pipeline`) 가 norm_key=="보험" 인 행을 부호별로 분리 — 양수는 그대로 "보험"(expense), 음수는 "보험금"(income, INCOME_KEYWORDS 매칭). INCOME_KEYWORDS 에 "보험" 대신 "보험금" 만 추가(plain 보험은 expense 유지). amount 계산 블록을 카테고리 resolve 앞으로 이동. 골든 데이터 기준 "보험" 4행 → 보험 1 + 보험금 3. 신규 테스트 `import_splits_insurance_rows_by_sign` 추가 (백엔드 85/85, 이전 84). 프런트 116/116 무변경. `sqlx.sh` 도 `-- --all-targets` 플래그 누락 수정. `.sqlx` offline 캐시 재생성(158 파일).
- 2026-05-08: 액터 카드 수입 도넛 — 헤더 아래 "수입 ₩X" 텍스트 줄을 동일 크기(`h-44`) 수입 도넛으로 교체(차트 + 가운데 "수입 ₩X" 라벨, 카테고리 범례 없음). EXPENSE_PALETTE 재사용. 백엔드 `IncomeResponse` 에 `categories: Vec<CategorySummary>` 추가(additive, `summary.rs` 의 `ByActorEntry`/`CategorySummary` 재사용; income 은 부호 그대로 양수, expense 만 `-SUM`). `summary.rs` 타입 doc 도 일반화. 프론트는 `buildActorIncomeSlices` / `buildHouseholdIncomeSlices` 신설(actor name 은 `income.by_actor` → categories cell 순으로 resolve), `IncomeResponseSchema` 에 `categories` 추가. `ActorDonut` props 를 `data + income(number)` → `actorName + expense + income(ActorDonutData)` 로 재설계(actorName 단일 소스), 내부 `DonutChart` 헬퍼로 income/expense 도넛 공통화 — chart/center testid 를 prop 으로 주입(income: `donut-income-chart`/`donut-income-center`, expense: `donut-expense-chart`/`donut-center`). `DashboardDonuts` 는 `incomeFor` 의존 제거, `EMPTY_DONUT` 상수로 actor 미존재 fallback. 테스트: 백엔드 +2 (`income_response_includes_categories_breakdown`, `income_categories_exclude_expense_kind`; 87/87), 프런트 +8 (donut-data 7 + dashboard 1; 124/124). Spec/plan: `docs/superpowers/{specs,plans}/2026-05-08-actor-income-donut*`.
- 2026-05-11: sqlx → SeaORM 원샷 이관 완료. `server/migration/` 신규 크레이트(단일 마이그레이션 in-place 정책 그대로 이식), `src/entity/` sea-orm-cli 자동 생성. CRUD/JOIN 은 ORM-first(`find_*_related`, `ActiveModel`, `OnConflict`), 뷰·집계는 raw SQL + `FromQueryResult`, 부분 unique index 가 있는 upsert(categories/products) 도 raw SQL. `import/pipeline.rs` 는 `DatabaseTransaction` + `ActiveModel` + `OnConflict` 로 재작성, race-safe alias merge 패턴(`LockType::Update` = SELECT FOR UPDATE) 보존. 테스트 인프라는 `#[sqlx::test]` → `tokio::test` + `tests/common/TestDb` 헬퍼(per-test ephemeral DB + `Migrator::up`). `.sqlx/` 캐시·`sqlx.sh`·transitional sqlx 직접 의존 폐기. 백엔드 87/87, 프런트 126/126, 골든 xlsx 재import 결과 byte-equal (177 rows, 0 integrity violations, 6 고덕방 아이스아메리카노 at ₩3,400, `v_monthly_settlement` deducted=7500). Spec/plan: `docs/superpowers/{specs,plans}/2026-05-10-seaorm-migration*`.
