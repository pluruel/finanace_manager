# 수입/지출 분리 및 부호 규약 변경

**Status:** Approved 2026-05-07
**Owner:** Junnoh Lee
**Scope:** 백엔드(`server/`) + 프론트엔드(`web/`) 양쪽 + DB 스키마 재작성

## 배경

현재 `transactions` 테이블은 다음과 같이 동작한다.

- `amount numeric(15,2)` 는 항상 양수.
- `sign smallint`(`-1` | `+1`) 가 별도로 존재. 엑셀 라인 금액이 음수이면 `sign=-1`, 그 외(차감 포함)는 `+1`.
- `categories.kind` 컬럼은 스키마에는 있으나 임포트 파이프라인이 항상 `kind='expense'` 로만 카테고리를 만들기 때문에 사실상 분류용으로 쓰이지 않는다.

이 모델은 두 가지 문제가 있다.

1. **수입(급여, 이자 등)이 1급 시민이 아니다.** 엑셀에서 음수로 적힌 금액은 모두 "지출의 음수"로 저장되므로 수입이 별도 분류로 드러나지 않는다.
2. **부호 규약이 금융/회계 관점과 반대다.** 회계상 현금 유출은 음수, 유입은 양수가 자연스러운데 현재는 지출이 `sign=+1`(양수)이다. `SUM(amount * sign)` 같은 계산이 곳곳에 흩어져 있어 가독성이 떨어진다.

또한 도메인 규칙상 **환불(음수 행)과 차감은 수입이 아니다.** 환불·차감은 정산(세틀먼트) 계산을 위한 조정값일 뿐, 수입 카테고리(급여·이자 등)와 본질적으로 다르다.

## 목표

- `categories.kind` 를 **유일한 수입/지출 분류 기준**으로 만든다(예: `급여`, `이자` → `income`; `대출이자`, `식비`, `차감` → `expense`).
- `transactions.sign` 컬럼을 **삭제**하고, `transactions.amount` 를 **부호 있는 캐시플로우 값**으로 저장한다(현금 유입 양수, 유출 음수).
- 정산(`v_monthly_settlement`)·요약(`/api/summary`)·도넛·엑셀 export 가 모두 새 규약 아래에서 동작하도록 재작성한다.
- 대시보드에 수입을 별도로 노출(액터별 수입 스트립)한다.
- `/aliases` 카테고리 탭에 수입↔지출 인라인 토글 스위치를 추가한다.

## 비목표

- 데이터 마이그레이션 없음. 서비스 전이므로 `001_init.sql` 을 직접 재작성하고 기존 데이터는 모두 삭제 후 `2026년 02월.xlsx` 를 재임포트한다(프로젝트 마이그레이션 정책 그대로).
- 다중 월 비교, 가구 규칙 페이지 등 미정 기능은 손대지 않는다.
- 수입에 대한 도넛 그리드는 만들지 않는다(수입 카테고리가 적어 오버킬). 대신 액터별 합계만 스트립으로 표시.

## 도메인 규칙(확정)

- **수입(`kind='income'`)**: 급여, 이자(받은 이자) 등 진짜 현금 유입. 엑셀에서는 음수로 적힌다(엑셀 자체가 지출 장부 관점이므로).
- **지출(`kind='expense'`)**: 식비·공과금·대출이자 등 일반 소비. `차감` 도 지출 종류이지만 정산용 특수 카테고리.
- **환불 / 음수 지출 라인**: 분류는 그대로 `expense`. 엑셀에서 음수, 저장 후에는 양수(현금 유입)로 보존되어 해당 카테고리 합계를 자연스럽게 깎는다. **수입이 아니다.**
- **`차감`**: 카테고리 이름이 `'차감'` 으로 고정된 특수 지출 카테고리. 기존처럼 임포트 시 `review_state='confirmed'` 로 자동 생성. 정산 뷰에서만 별도 항목으로 분리.

### 부호 규약 (핵심)

저장된 `amount` 는 **현금흐름 부호**를 따른다: 유입(+), 유출(−). 엑셀은 지출 장부 관점이므로 그대로 두면 부호가 반대이고, 따라서 임포트 시 **엑셀 라인 금액을 일괄 반전**해서 저장한다 — `stored_amount = -excel_amount`.

| 케이스 | 엑셀 값 | 저장값 | `kind` |
|---|---|---|---|
| 일반 지출 (식비 +10,000) | `+10000` | `-10000` | `expense` |
| 환불 (식비 -2,000) | `-2000` | `+2000` | `expense` |
| 급여 -3,500,000 | `-3500000` | `+3500000` | `income` |
| 차감 +3,000 | `+3000` | `-3000` | `expense` (특수) |

수입과 환불 모두 저장 후 양수가 되지만 `kind` 가 다르므로 대시보드/정산에서 분리된다.

## 스키마 변경 (`server/migrations/001_init.sql` 재작성)

### `transactions`

```sql
CREATE TABLE transactions (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id          uuid NOT NULL,
  raw_id            uuid NOT NULL REFERENCES transactions_raw(id) ON DELETE CASCADE,
  group_id          uuid NOT NULL,
  occurred_on       date NOT NULL,
  merchant_id       uuid REFERENCES merchants(id),
  actor_id          uuid REFERENCES ledger_actors(id),
  category_id       uuid REFERENCES categories(id),
  product_id        uuid REFERENCES products(id),
  payment_method_id uuid REFERENCES payment_methods(id),
  amount            numeric(15,2) NOT NULL,  -- 부호 있음. 유입(+) / 유출(-)
  unit_price        numeric(15,4),
  quantity          numeric(15,4),
  memo              text
);
```

- `sign smallint ... CHECK (sign IN (-1,1))` 줄을 **삭제**한다.
- `amount` 의 항상-양수 규약을 **삭제**한다(주석 갱신 필수).
- 기존 인덱스는 유지(`transactions_date_idx` 등 그대로).

### `categories`

기존 정의 유지(`kind text NOT NULL CHECK (kind IN ('income','expense'))`). 임포트 시 기본값 `'expense'` 도 유지. 차이는 사용자가 `/aliases` 에서 토글로 `kind` 를 바꿀 수 있다는 점.

### `v_monthly_settlement` 재작성

저장 규약상 지출은 음수, 차감도 음수, 환불은 양수다. 정산 카드는 양수 표시이므로 모두 `-SUM` 로 양수화해서 노출한다.

```sql
CREATE VIEW v_monthly_settlement AS
SELECT
  t.owner_id,
  date_trunc('month', t.occurred_on)::date AS month,
  -- 공동 actor 의 일반 지출(차감 제외). 환불(양수 amount)은 자연 차감됨.
  COALESCE(-SUM(t.amount) FILTER (
    WHERE actor.name = '공동' AND c.kind = 'expense' AND c.name <> '차감'
  ), 0) AS recognized_expense,
  -- 차감 합계(actor 무관). 차감 행은 음수 amount 로 저장됨 → 양수화.
  COALESCE(-SUM(t.amount) FILTER (WHERE c.name = '차감'), 0) AS deducted_amount,
  -- 입금액 = 경비인정 - 차감
  COALESCE(-SUM(t.amount) FILTER (
    WHERE actor.name = '공동' AND c.kind = 'expense' AND c.name <> '차감'
  ), 0)
  - COALESCE(-SUM(t.amount) FILTER (WHERE c.name = '차감'), 0) AS settlement_input
FROM transactions t
JOIN categories c        ON c.id     = t.category_id
JOIN ledger_actors actor ON actor.id = t.actor_id
GROUP BY t.owner_id, date_trunc('month', t.occurred_on);
```

- 수입(`kind='income'`)은 정산에 포함하지 않는다(도메인 규칙).
- 환불(양수 amount, `kind='expense'`, 차감 아님)은 `-SUM(...)` 안에서 음수 지출들과 자연 상쇄되어 `recognized_expense` 를 줄인다(예: 식비 1만원 지출 → -10000, 식비 3천원 환불 → +3000, `-SUM` = 7000).
- 차감은 저장 후 음수이므로 `-SUM` 로 양수 deduction 을 만든다.

### 기타 도메인 테이블

`ledger_actors`, `merchants`, `products`, `payment_methods`, `aliases`, `import_batches`, `transactions_raw` 는 변경 없음.

## 임포트 파이프라인 변경 (`server/src/import/pipeline.rs`)

- 현재 라인 540~566 의 `is_deduction → sign` 분기 **전체 삭제**.
- `let raw_amount = row.line_amount.or(row.total_amount);` 의 결과를 **부호 반전**해서 저장: `amount = -raw_amount`. `abs()` 호출 제거.
- `차감` 카테고리 자동 생성 로직(`is_deduction` 으로 review_state 결정)은 카테고리 upsert 쪽에 그대로 둔다(부호와 무관, 단지 카테고리 메타 처리).
- 그룹 합계 무결성 체크: `transactions_raw.total_amount` 는 엑셀 원본을 그대로 보존하고(반전 안 함), `transactions.amount` 만 반전 저장한다. 따라서 비교는 `-SUM(t.amount) = g.header_total` (또는 동등하게 `SUM(t.amount) = -g.header_total`). 라인 436, 446 의 `SUM(t.amount * t.sign)` 자리를 `-SUM(t.amount)` 로 교체.

## API 변경

### `GET /api/summary/:year/:month` (`server/src/api/summary.rs`)

- 현재 `SUM(t.amount * t.sign)` 자리를 **`-SUM(t.amount)`** 로 변경. 저장상 지출은 음수이므로 반전해서 양수 지출 합계로 응답한다(프론트에서 그대로 양수로 그릴 수 있도록).
- 기본 필터에 `c.kind = 'expense'` 추가(도넛/피벗에서 노출되는 카테고리 = 지출만).
- 응답 DTO 의 `sign: i16` 필드는 삭제. `amount` 자체가 양수 지출 크기 + 환불 우세 카테고리는 자연 음수가 된다. 프론트에서는 `Math.abs(amount)` 를 슬라이스 크기로, `amount < 0` 을 환불 우세 표식으로 사용.

### 신규 `GET /api/summary/income/:year/:month`

```json
{
  "month": "2026-02",
  "by_actor": [
    {"actor_id": "...", "actor_name": "공동", "total": 0},
    {"actor_id": "...", "actor_name": "엉아", "total": 3500000},
    {"actor_id": "...", "actor_name": "아기", "total": 0}
  ],
  "total": 3500000
}
```

쿼리: `SELECT actor_id, SUM(amount) FROM transactions JOIN categories ... WHERE c.kind='income' AND month=...GROUP BY actor_id`. 빈 actor 도 0 으로 채워서 응답(프론트 정렬 안정성을 위해).

### 신규 `PATCH /api/categories/:id/kind`

- Body: `{"kind": "income" | "expense"}`.
- 200 OK 시 갱신된 카테고리 반환.
- `owner_id` 일치 검사 + 파라미터 화이트리스트 검증.

### `GET /api/export/:year/:month` (`server/src/api/export.rs`)

- Transactions 시트: `sign` 컬럼 **제거**, 대신 `kind` 컬럼 추가(`income` / `expense`). `amount` 는 저장된 캐시플로우 부호 그대로(지출 음수, 수입/환불 양수).
- Settlement 시트: 모양 동일(뷰 재계산 결과만 반영).
- Summary 시트: 라인 366 의 `SUM(t.amount * t.sign)` → `-SUM(t.amount)` 로 변경(지출 양수 합계 기준). `kind='expense'` 필터 추가.

## 프론트엔드 변경 (`web/`)

### 대시보드 `(app)/page.tsx`

상→하 순서:

1. 헤더(기존)
2. `SettlementCard`(기존)
3. **`<IncomeStrip />`**(신규) — 액터별 수입 합계를 한 줄로
4. `DashboardDonuts`(기존, 다만 지출 전용 데이터)

### 신규 `web/components/income-strip.tsx`

- 입력: `IncomeSummary`(위 신규 엔드포인트 응답).
- 표시: 가로 3 칸(또는 더미 actor 가 있으면 +1) — `공동 ₩X / 엉아 ₩Y / 아기 ₩Z`. 응답의 `total` 은 이미 양수(서버에서 income kind 행 합계가 양수로 저장됨).
- 빈 액터는 `₩0` 으로 채워 그리드 정렬 보존(도넛 그리드의 빈 카드 처리와 동일 컨벤션).
- 페치는 서버 컴포넌트에서 `summary` 와 병렬로 수행하여 단일 라운드트립 추가.

### 도넛 데이터 (`web/lib/donut-data.ts`, `DashboardDonuts`)

- 백엔드가 `kind='expense'` 필터 + `-SUM(amount)` 로 응답하므로 일반적으로 `amount` 는 양수(지출 크기). 환불이 우세한 카테고리는 음수가 들어올 수 있다.
- `signedNumber(amount, sign)` 헬퍼는 제거. 슬라이스 크기 계산은 `Math.abs(amount)`. 중앙 라벨/소계는 합산값을 그대로 표시(음수면 "−₩X" 로 표기, 환불 우세 표식).
- 차감 핀 처리는 그대로(이름 기반). 차감은 백엔드에서 이미 양수로 반전되어 옴.

### `/aliases` 카테고리 탭

- 각 카테고리 행에 인라인 `<Switch>` 추가. 라벨: `수입` / `지출`(또는 `income` / `expense` 영문 그대로 보일지 결정 — 본 스펙에서는 한글 라벨로 통일).
- 토글 시 `PATCH /api/categories/:id/kind` 호출, 낙관적 업데이트 + 실패 시 롤백 토스트.
- `차감` 카테고리는 토글 비활성화(시스템 보호 카테고리).

## 테스트 계획

### 백엔드(`cargo test -p server`)

수정:
- `tests/test_m1_pipeline.rs`: 그룹 합계 무결성 검사 부분을 `SUM(amount)` 기반으로 갱신. 음수 라인이 있는 영수증의 합산 케이스 추가.
- `tests/test_m2_*.rs`: 정산 뷰 결과 기댓값을 새 공식으로 재계산.
- `tests/test_m4_export.rs`: export 헤더에서 `sign` 제거 / `kind` 추가 반영.

신규:
- `PATCH /api/categories/:id/kind` 동작 + 권한 체크 + 차감 보호.
- `GET /api/summary/income/:year/:month` 의 액터별 합계 + 빈 액터 zero-fill.

### 프론트엔드(`npm test`)

수정:
- `donut-data.test.ts`: signedNumber 헬퍼 제거 후 amount 그대로 사용하는 케이스로 단순화.
- `dashboard.test.tsx`: `IncomeStrip` 렌더링 + zero-fill 케이스.

신규:
- `income-strip.test.tsx`(컴포넌트 단위).
- `aliases.test.tsx` 에 `kind` 토글 인터랙션 케이스.

## 누적 컨텍스트 / 문서 갱신

- `CLAUDE.md` Core Domain Rules 의 "Negative expenses are stored with `sign = -1` (no separate table)." 줄을 새 규약("amount is a signed cash-flow value; income classification lives in `categories.kind`.")으로 교체.
- Cumulative Context 에 2026-05-07 항목으로 본 변경 요약을 한 줄 추가.

## 빌드 순서(권고)

1. `001_init.sql` 재작성 + `cargo sqlx prepare` 재생성.
2. 임포트 파이프라인 sign 제거 + 그룹 무결성 쿼리 갱신.
3. `summary.rs` / `settlement.rs` / `export.rs` 의 sign 제거 및 kind 필터 적용.
4. `PATCH /api/categories/:id/kind`, `GET /api/summary/income/:year/:month` 추가.
5. 프론트 `donut-data.ts`, `IncomeStrip`, `/aliases` 토글.
6. 데이터 wipe + `2026년 02월.xlsx` 재임포트 후 수동 검수(공동 정산값, 액터별 수입, 도넛 합계).
7. 테스트 갱신/추가 → backend·frontend 모두 green.
8. `CLAUDE.md` 갱신.

## 미해결 / 후속

- 수입 카테고리가 늘어나면 액터별 단일 합계로 부족할 수 있음. 그 시점에 수입 도넛 그리드 추가를 재검토.
- 환불이 정산에 미치는 영향(공동 카테고리 환불이 recognized_expense 를 줄이는 동작)이 직관적인지는 첫 월 데이터로 검수 후 판단. 필요시 별도 "환불 별도 표기" 옵션을 후속 스펙에서 다룬다.
