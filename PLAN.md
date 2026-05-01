# 가계부 통합 뷰어 — 초기 구현 계획

## Context

사용자는 매월 별도 엑셀(`YYYY년 MM월.xlsx`)에 가계부를 입력해 왔고, 입력은 앞으로도 엑셀에서 한다. 이 앱은 **여러 달치 데이터를 통합 조회·분석하는 뷰어**다. 핵심 가치 셋:

1. 월별 .xlsx를 임포트해 PostgreSQL에 누적
2. **카테고리·구매처·상품 정규화** — "이 마트"/"이마트", "외식_점심"/"외식 점심", "조닌끼안티"/"조닌 끼안티" 같은 표기 차이를 같은 항목으로 묶기
3. **가격 추적** — 메모(F열 `내용`)가 채워진 행을 product 라인 아이템으로 매핑해 단가 시계열 형성. 메모 없는 행은 구매처 단위 통계로 폴백.

서버는 Rust(axum), DB는 PostgreSQL 17, 프론트는 Next.js 15 App Router. 인증은 외부 `auth.junodevs.com`(EdDSA JWT, MSA). 다운스트림 DB는 `sub`(uuid)만 보유하고 사용자 정보 복제 금지(`MSA_INTEGRATION.md` 계약).

### 엑셀 구조 — 핵심 발견 (`2026년 02월.xlsx` 실측)

엑셀 한 행 = 한 거래가 아니다. **영수증 1건이 여러 행으로 분해되는 multi-line 그룹**이 존재한다.

- **헤더 행**: 컬럼 E `지출(합계)`에 영수증 총액이 적힘. `단가 × 개수 = 지출(매수)`도 동시에 채워짐 → 헤더도 라인이다.
- **자식 행**: 헤더 직후 행이 같은 `occurred_on`(날짜)을 가지고 컬럼 E가 비어 있으면 동일 그룹의 자식. 컬럼 G(`단가`), H(`개수`), I(`지출(매수)`), F(`내용`=상품명)만 채움. 자식의 `merchant_text`는 헤더와 다를 수 있음. 헤더의 합계 = 자식+헤더의 line_amount 합.

| 분류 | 2월 건수 | 처리 |
|---|---|---|
| 그룹 총수 | 256 | |
| multi-line 그룹 | 7 (2.7%) | 영수증 1건당 2~17개 라인. 단가 추적 골든 케이스 |
| single-line 그룹 (메모 있음) | 82 | 단가 추적 가능 (product 매핑) |
| single-line 그룹 (메모 없음) | 167 | product 매핑 안 함, 구매처/카테고리 합계만 |

multi-line 패턴 3종:
- **영수증 분해**: 17행 이마트(2종 와인), 71행 인바이트 디저트(4종 메뉴), 89행 홈플러스(6종 + 주방용품), 127행 풍림아이원 관리비(17개 명세).
- **분담 차감**: 8행 화육면, 147행 동원식당, 159행 곳온니플레이스. 헤더 + 카테고리="차감"인 자식 1행. 가계 룰(외식 인당 15,000원까지 인정)에 따른 한도 초과분으로 정산에서 제외 → 개인 용돈에서 부담. 집계 시트 R104·R114·R116·R118·R119에서 "경비인정액 - 차감 = 입금액"으로 검증됨 (2월: 584,000 - 7,500 = 576,500).

---

## 0. 사전 작업: CLAUDE.md 작성

루트 `/Users/juno/dev/finance_mananger/CLAUDE.md`:

- "User 도메인을 구현/수정할 때는 반드시 [`MSA_INTEGRATION.md`](./MSA_INTEGRATION.md)를 먼저 읽고 따른다."
- "인증 서버: `auth.junodevs.com` (auth-svc). JWKS: `https://auth.junodevs.com/auth/.well-known/jwks.json`."
- 다운스트림 규칙 요점 4줄: ① owner_id(uuid)만 저장·FK 금지 ② email/이름 복제 금지 ③ JWT는 EdDSA·iss/aud/exp/typ 검증 ④ refresh는 httpOnly 쿠키.

---

## 1. PostgreSQL 17 스키마

핵심 원칙: **모든 도메인 테이블에 `owner_id uuid NOT NULL`, FK 없음**. 임포트 데이터는 `transactions_raw`(원본 보존)와 `transactions`(정규화 참조) 양쪽으로 저장.

```sql
-- 가계부 내부 사용자(공동/엉아/아기) — 로그인 계정과 무관한 라벨
CREATE TABLE ledger_actors (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id    uuid NOT NULL,
  name        text NOT NULL,
  UNIQUE (owner_id, name)
);

CREATE TABLE categories (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL,
  parent_id    uuid REFERENCES categories(id),  -- 같은 테이블 내부 계층은 FK 허용
  name         text NOT NULL,                    -- 정규화 후의 표준명
  kind         text NOT NULL CHECK (kind IN ('income','expense')),
  review_state text NOT NULL DEFAULT 'pending'
               CHECK (review_state IN ('pending','confirmed')),
  UNIQUE (owner_id, parent_id, name)
);

CREATE TABLE merchants (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL,
  name         text NOT NULL,
  review_state text NOT NULL DEFAULT 'pending',
  UNIQUE (owner_id, name)
);

-- 상품: 메모(F열 `내용`)가 채워진 라인 아이템을 정규화한 단위. 단가 추적의 키.
CREATE TABLE products (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL,
  merchant_id  uuid REFERENCES merchants(id),  -- 보통 NOT NULL이지만 NULL 허용해 "구매처 무관 상품"도 표현 가능
  name         text NOT NULL,                   -- 정규화 후의 표준 상품명
  review_state text NOT NULL DEFAULT 'pending'
               CHECK (review_state IN ('pending','confirmed')),
  UNIQUE (owner_id, merchant_id, name)
);

CREATE TABLE payment_methods (
  id        uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id  uuid NOT NULL,
  name      text NOT NULL,
  UNIQUE (owner_id, name)
);

-- 별칭: 임포트된 raw 텍스트 → 정규화 엔티티
CREATE TABLE aliases (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL,
  scope        text NOT NULL CHECK (scope IN ('category','merchant','payment_method','actor','product')),
  raw_text     text NOT NULL,    -- 원본 그대로
  norm_key     text NOT NULL,    -- NFC + trim + lower + 공백/언더스코어 통일 후
  target_id    uuid NOT NULL,
  UNIQUE (owner_id, scope, norm_key)
);
CREATE INDEX ON aliases (owner_id, scope, norm_key);

-- 임포트 배치 추적 (재임포트 시 멱등성 확보)
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

-- 엑셀 한 행을 그대로 보존 (원본 진실의 원천)
CREATE TABLE transactions_raw (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id        uuid NOT NULL,
  import_batch_id uuid NOT NULL REFERENCES import_batches(id) ON DELETE CASCADE,
  row_index       int  NOT NULL,
  group_id        uuid NOT NULL,          -- 같은 영수증을 묶는 키 (헤더/자식 공유)
  is_group_header boolean NOT NULL,       -- true면 헤더 행(컬럼 E `지출(합계)` 보유)
  occurred_on     date,                   -- 파싱 실패 가능 → null 허용
  raw_date_serial double precision,
  merchant_text   text,
  actor_text      text,
  category_text   text,
  total_amount    numeric(15,2),          -- 헤더만 채움. 자식은 NULL
  memo            text,
  unit_price      numeric(15,2),
  quantity        numeric(15,4),
  line_amount     numeric(15,2),
  payment_text    text,
  evidence_text   text,
  extras          jsonb                   -- 컬럼 11/12 잡 데이터
);
CREATE INDEX ON transactions_raw (owner_id, occurred_on);
CREATE INDEX ON transactions_raw (owner_id, group_id);

-- 정규화된 거래 (대시보드/집계의 소스). 라인 단위 저장.
CREATE TABLE transactions (
  id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id           uuid NOT NULL,
  raw_id             uuid NOT NULL REFERENCES transactions_raw(id) ON DELETE CASCADE,
  group_id           uuid NOT NULL,        -- transactions_raw.group_id와 동일
  occurred_on        date NOT NULL,
  merchant_id        uuid REFERENCES merchants(id),
  actor_id           uuid REFERENCES ledger_actors(id),
  category_id        uuid REFERENCES categories(id),
  product_id         uuid REFERENCES products(id),  -- 메모 있는 행만 채움
  payment_method_id  uuid REFERENCES payment_methods(id),
  amount             numeric(15,2) NOT NULL,   -- 항상 양수 (= line_amount의 절댓값)
  sign               smallint NOT NULL CHECK (sign IN (-1, 1)), -- -1=수입(회수), 1=지출
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

주의:
- 금액·단가는 모두 `numeric(15,2)` (또는 단가는 `numeric(15,4)`). f64 금지.
- Excel serial → DATE: epoch는 **1899-12-30**(1900-02-29 버그 회피).
- 음수 지출은 `sign = -1`로 저장(별도 테이블로 가르지 않음).
- **transactions 생성 규칙**:
  - **single-line 그룹**: 헤더가 곧 라인 → transactions에 1 row.
  - **multi-line 그룹**: 헤더 행도 transactions에 1 row 생성. 자식 N개도 별도 라인으로 저장. 총 (1+N)개 라인. (헤더의 `total_amount`는 자식 합과 중복이지만 데이터 무결성 검증 차원에서 보존. single-line과의 통일성 위해 헤더도 라인으로 저장.)
- **"차감" 카테고리**: 임포트 파이프라인에서 자동 생성(kind='expense', review_state='confirmed' 보호). 영수증 합계 무결성을 위해 `sign=+1`로 저장하지만 정산 산출에선 분리. 공동 결제·개인 귀속이라는 모순적 성격을 카테고리 이름으로 식별.

### 정산 뷰 (M2)

차감을 분리한 공동 정산 산출 — 집계 시트의 "경비인정 / 차감 / 입금액" 산식을 SQL로 재현.

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

### 합계 무결성 검증 (임포트 직후 1회)

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

결과 0행이 합격. 불일치 행은 import 응답 + 로그에 경고.

---

## 2. Rust 백엔드 (`server/`)

### 디렉토리

```
server/
  Cargo.toml
  src/
    main.rs
    config.rs
    db.rs              # sqlx::PgPool
    auth/
      mod.rs           # 미들웨어 (Authorization 헤더/쿠키 모두 수용)
      jwks.rs          # JWKS fetch + 5분 TTL 메모리 캐시 + miss 시 1회 강제 갱신
      claims.rs        # iss/aud/exp/typ/EdDSA 검증
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
      xlsx.rs          # calamine 파서, "M월" 시트 추출, serial→date
      grouping.rs      # 헤더+자식 그룹 검출 (group_id 부여, is_group_header 판정)
      normalize.rs     # NFC + trim + 공백·언더스코어 통일 → norm_key
      pipeline.rs      # raw 저장 → 그룹 검증 → 매칭 시도 → 미매칭은 review pending
    domain/            # 모델 구조체
    error.rs
```

### 의존성 (요지)

`axum`, `tokio`, `tower-http`(CORS·Trace), `sqlx`(postgres, uuid, chrono, decimal, runtime-tokio, tls), `jsonwebtoken`(≥9, EdDSA), `calamine`(xlsx 읽기 전용), `rust_decimal`, `time` 또는 `chrono`, `reqwest`(JWKS), `serde`, `serde_json`, `unicode-normalization`, `sha2`, `tracing`/`tracing-subscriber`.

> 선택 근거: 쓰기 없는 xlsx 읽기엔 `calamine`이 `umya-spreadsheet`보다 가볍고 빠르다. 스키마가 단순해 ORM 불요 → `sqlx` 컴파일 타임 쿼리 검증이 안전하다.

### 인증 미들웨어 핵심

1. 부팅 시 JWKS fetch → 메모리 캐시(TTL 300s).
2. 매 요청: `Authorization: Bearer <t>` 또는 `Cookie: Authorization=Bearer <t>` 추출.
3. 검증: 서명(EdDSA), `iss == "auth-svc"`, `aud` 배열에 서비스명(`finance-manager`) 포함, `exp` 미만료, `typ == "access"`.
4. 검증 실패(키 미스매치)면 JWKS 1회 강제 재fetch 후 재시도.
5. 통과하면 `Extension<AuthUser { sub, email, groups }>`로 핸들러에 주입. `sub`만 DB `owner_id`로 사용.
6. `kid` 검증 비활성(헤더에 `kid` 없음).

### 엔드포인트 (MVP)

| 메서드 | 경로 | 설명 |
| --- | --- | --- |
| POST | /api/import | multipart로 .xlsx 1개. file_hash 중복이면 409. 결과로 batch + 그룹 합계 무결성 결과 + 미매칭 목록 반환 |
| GET | /api/transactions | `?from=&to=&category=&actor=&merchant=&payment=&product=&group=` 필터. multi-line 그룹은 group_id로 묶어 함께 반환 |
| GET | /api/summary/:year/:month | 카테고리×액터 피벗 (엑셀 "M월(집계)"와 동일 구조) |
| GET | /api/settlement/:year/:month | 공동 정산 카드 — `recognized_expense`, `deducted_amount`, `settlement_input` (집계 시트 "경비인정-차감=입금액") |
| GET | /api/price-history | `?product_id=` 단가 시계열 (메모 있는 라인만) |
| GET | /api/merchant-stats | `?merchant_id=` 메모 없는 거래용 폴백. 월별 지출액·횟수 |
| GET | /api/products | `?merchant_id=&q=` 상품 목록·검색 |
| GET/POST/DELETE | /api/aliases | 별칭 등록/삭제. `?scope=category|merchant|payment_method|actor|product` |
| GET | /api/review-queue | 미매칭 raw 텍스트 목록 (category/merchant/payment/actor/product 통합) |
| GET | /api/categories, /api/merchants, /api/payment-methods | 정규화 엔티티 목록 |

---

## 3. 정규화 전략

`norm_key` 생성 함수 (Rust):

1. Unicode NFC 정규화 (macOS 파일·클립보드 NFD 회피).
2. 양끝 trim, 내부 다중 공백 → 단일 공백.
3. `_` → ` ` (언더스코어와 공백 동일시).
4. 한글은 그대로, 영문은 `to_lowercase`.

임포트 파이프라인:

1. .xlsx 파일 SHA-256 → `import_batches`에 멱등 삽입(중복 409).
2. "M월" 시트 한 행씩 읽으며 **그룹 검출**:
   - 컬럼 E `지출(합계)`가 채워진 행 → 새 `group_id` 부여, `is_group_header = true`.
   - 헤더 직후 행이 같은 `occurred_on`을 가지고 컬럼 E가 비어 있으면 동일 group의 자식 (`is_group_header = false`). 자식의 `merchant_text`는 헤더와 다를 수 있음.
   - 다른 `occurred_on` 또는 새 헤더가 나오면 그룹 종료.
3. `transactions_raw`에 모든 행을 그대로 저장 (group_id, is_group_header 포함).
4. 각 텍스트 컬럼(category/merchant/actor/payment)에 대해:
   - `norm_key` 계산 → `aliases`에 매핑이 있으면 `target_id` 사용.
   - 없으면 같은 `norm_key`의 정규 엔티티를 찾아 자동 매핑 alias 생성(`review_state = pending`).
   - 그것도 없으면 새 엔티티 자동 생성(`review_state = pending`) + alias.
5. **product 매핑 (메모 있는 행만)**:
   - 메모(F열) 있는 행 → `(merchant_id, norm_key(memo))`로 product alias 조회 → 매칭/생성. `transactions.product_id` 채움.
   - 메모 없음 → `product_id = NULL`. 후속 메모 편집 UI는 만들지 않는다(엑셀 원본이 진실의 원천).
6. `transactions` 행 생성:
   - **single-line 그룹**: 헤더 1 row 생성. `amount = abs(total_amount)`, `sign`은 부호.
   - **multi-line 그룹**: 헤더 1 row + 자식 N개 = (1+N)개 생성. `amount = abs(line_amount 또는 total_amount)`, `sign`은 부호.
   - 카테고리="차감"인 행은 `sign = +1` 그대로(영수증 합계 무결성 유지). 정산 산출은 `v_monthly_settlement` 뷰에서 분리.
7. **합계 무결성 검증 SQL** 실행 → 불일치 group_id는 import 응답·로그에 경고로 노출.
8. 미매칭(처음 보는 norm_key)은 `/api/review-queue`로 노출 → 사용자가 UI에서 ① 기존 엔티티에 합치기 ② 새 엔티티 확정. 확정하면 `review_state = confirmed`. product alias도 동일 흐름.

> 컬럼 11/12의 잡 데이터는 `transactions_raw.extras` jsonb로 보존만 하고 무시.
> "증빙" 컬럼이 카테고리명으로 보이는 행은 임포트 로그에 경고만 남기고 진행.
> 집계 시트(`M월(집계)`) 후반부의 자유 텍스트 가계 룰("외식 15000까지", "커피 일 만원" 등)은 자동 임포트 대상 아님. 향후 별도 페이지에서 사용자가 직접 관리. MVP 미포함.

---

## 4. Next.js 프론트엔드 (`web/`)

### 라우트 (App Router)

```
app/
  (auth)/login/page.tsx
  (app)/layout.tsx                  # 사이드바 + auth 미들웨어 통과 영역
  (app)/page.tsx                    # 대시보드 (이번 달 요약 + 정산 카드 + 최근 거래)
  (app)/transactions/page.tsx       # 필터·정렬 가능한 거래 테이블 + 그룹 펼침
  (app)/import/page.tsx             # xlsx 업로드 + 임포트 결과 (그룹 합계 무결성 표시)
  (app)/aliases/page.tsx            # 정규화/별칭 관리 + review queue (4탭: category/merchant/payment/product)
  (app)/price-history/page.tsx      # Products / Merchants 토글 차트
middleware.ts                       # access 만료시 /auth/refresh, 실패시 /login
lib/api.ts                          # Rust API fetch wrapper (cookie 전달)
```

### 인증

- `/auth/login` 호출(form-urlencoded) → 응답으로 access + refresh 받음.
- **refresh** 토큰: Next.js Route Handler에서 `Set-Cookie: refresh=...; HttpOnly; Secure; SameSite=Lax`로 저장.
- **access** 토큰: 서버 컴포넌트는 요청 헤더의 쿠키에서 꺼내 Rust API 호출 시 `Cookie: Authorization=Bearer <access>` 로 전달. 클라이언트 컴포넌트에선 메모리 보관(필요 최소).
- middleware.ts: access 만료/부재 시 `/auth/refresh` 호출해 갱신, 실패면 `/login` 리다이렉트.

### UI 동작 요점

- **`/aliases`**: 4탭(category/merchant/payment/**product**). product alias 합치기 시 기존 `transactions.product_id` 자동 갱신.
- **`/price-history`**: 헤더에 "Products / Merchants" 토글. Products는 product 매핑된 거래의 단가 라인 차트, Merchants는 메모 없는 거래까지 포함한 구매처별 월합계 추이.
- **`/transactions`**: multi-line 그룹은 헤더 스타일 행에 ▸ 토글, 펼치면 자식 라인 표시. 카테고리="차감"인 행은 회색 + "정산 차감" 뱃지.
- **대시보드**: 정산 카드 — "경비인정 ₩XXX − 차감 ₩X = 입금액 ₩XXX" (`/api/settlement/:year/:month`).

### UI 라이브러리

- `shadcn/ui` (Radix 기반) — 폼/다이얼로그/사이드바
- `@tanstack/react-table` — 거래 테이블 필터/정렬/그룹 expand
- `recharts` — 월별 집계 막대/단가 추이 라인
- `tailwindcss` — 스타일링

---

## 5. 개발 환경

`/Users/juno/dev/finance_mananger/`:

```
CLAUDE.md
MSA_INTEGRATION.md       (기존)
docker-compose.yml       (postgres:17만 띄움)
server/                  (Rust)
web/                     (Next.js)
.env.example
2026년 02월.xlsx          (현재 위치 유지, 또는 sample/로 이동)
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

`docker-compose.yml`은 postgres:17 단일 서비스. Rust/Next는 로컬 dev로 실행해 빠른 반복.

---

## 6. 마일스톤

**M1 — 부트스트랩 + 임포트** (완료 2026-04-25, 기준 달성: `2026년 02월.xlsx` 업로드 시 transactions에 177건 삽입(실측), 그룹 합계 무결성 SQL이 0행, 카테고리별 합계가 엑셀 "2월(집계)"와 ±0원 일치, 모든 테스트 통과)
- CLAUDE.md, docker-compose, sqlx 마이그레이션 (products 테이블, group_id, product_id, v_monthly_settlement 포함)
- JWT 미들웨어 + JWKS 캐시
- POST /api/import (calamine + 그룹 검출 + 정규화 + raw 저장)
- 거래 목록 페이지 (필터 1차, 그룹 펼침)

**M2 — 정규화 UI + 월별 대시보드 + 정산 카드** (완료 기준: review queue에서 "이 마트"를 "이마트"에 합치면 기존 거래의 `merchant_id`도 갱신되고 집계가 즉시 반영. "조닌끼안티"/"조닌 끼안티" product alias 합치기 시 product_id 자동 재매핑. `v_monthly_settlement`가 2월에 대해 deducted_amount = 7,500을 반환)
- /aliases 페이지(미매칭 목록, 합치기/확정) — 4탭 product 포함
- alias 변경 시 영향받는 transactions 자동 재매핑 (merchant_id, product_id 등)
- /api/summary/:year/:month + 대시보드(엑셀 "(집계)"와 동일 피벗 + 정산 카드)
- /api/settlement/:year/:month

**M3 — 가격 추적 + 구매처 통계 + 다중 월 통합** (완료 기준: 고덕방 아이스아메리카노 6회가 모두 3400원으로 묶여 단가 시계열 표시. 메모 없는 167건은 구매처 단위 월합계 추이로 별도 노출)
- /api/price-history (product 단위)
- /api/merchant-stats (메모 없는 거래용 폴백)
- /price-history 페이지 Products/Merchants 토글
- 다중 월 비교 차트
- xlsx 내보내기는 후속(필요 시)

---

## 7. 핵심 파일 / 재사용

신규 프로젝트라 재사용할 기존 코드는 없음. 외부 참조:

- 인증 계약 — `MSA_INTEGRATION.md` (User 구현 시 필독, CLAUDE.md에서 명시)
- 데이터 원본 — `2026년 02월.xlsx` (M1 임포트 골든 케이스)

수정/생성 대상 파일:

- `CLAUDE.md` (신규)
- `docker-compose.yml` (신규)
- `.env.example` (신규)
- `server/` 전체 (신규 cargo 프로젝트)
- `web/` 전체 (신규 next 프로젝트)

---

## 8. 검증 (M1 종료 시점)

1. `docker compose up -d postgres` → `sqlx migrate run` 성공.
2. `cargo run -p server` 부팅 시 JWKS fetch 로그 확인.
3. auth.junodevs.com에서 access 토큰 획득(curl로 form-urlencoded 로그인) → `/api/transactions`에 `Authorization: Bearer ...` 호출 시 401 아닌 200(빈 배열).
4. `pnpm dev` → /login 통과 → /import 페이지에서 `2026년 02월.xlsx` 업로드.
5. SQL 검증:
   - `SELECT COUNT(*) FROM transactions WHERE owner_id = $sub;` = 282 (single-line 249 + multi-line 자식 33).
   - `SELECT COUNT(*) FROM transactions WHERE product_id IS NULL;` ≈ 167 (메모 없는 single-line 행 수와 일치).
   - 카테고리별 합계가 엑셀 "(집계)" 시트 값과 일치.
6. 거래 목록 페이지에서 필터링·정렬·그룹 펼침 동작 확인.
7. **그룹 무결성 SQL**(§1 검증 SQL) → 0행. 7개 multi-line 그룹의 자식 합 == 헤더 합계.
8. `SELECT name, COUNT(*), array_agg(unit_price ORDER BY occurred_on) FROM transactions t JOIN products p ON p.id = t.product_id JOIN merchants m ON m.id = t.merchant_id WHERE p.name='아이스아메리카노' AND m.name='고덕방' GROUP BY name;` → 6회, 모두 3400.
9. `/aliases` product 탭에서 같은 (merchant, 메모변형) 두 product를 합쳤을 때 transactions.product_id가 즉시 통합되는지 UI 확인.
10. `SELECT * FROM v_monthly_settlement WHERE month = '2026-02-01';` → `deducted_amount = 7500`, `settlement_input = recognized_expense - 7500` (집계 시트 R118·R119와 교차 검증).
