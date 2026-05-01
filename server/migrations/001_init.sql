-- Enable pgcrypto for gen_random_uuid()
CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- 가계부 내부 사용자(공동/엉아/아기) — 로그인 계정과 무관한 라벨
CREATE TABLE ledger_actors (
  id        uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id  uuid NOT NULL,
  name      text NOT NULL,
  UNIQUE (owner_id, name)
);

CREATE TABLE categories (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL,
  parent_id    uuid REFERENCES categories(id),  -- 같은 테이블 내부 계층은 FK 허용
  name         text NOT NULL,                   -- 정규화 후의 표준명
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

-- 상품: 메모(F열 내용)가 채워진 라인 아이템을 정규화한 단위. 단가 추적의 키.
CREATE TABLE products (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL,
  merchant_id  uuid REFERENCES merchants(id),  -- NULL 허용: "구매처 무관 상품"도 표현 가능
  name         text NOT NULL,                  -- 정규화 후의 표준 상품명
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
  id        uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id  uuid NOT NULL,
  scope     text NOT NULL CHECK (scope IN ('category','merchant','payment_method','actor','product')),
  raw_text  text NOT NULL,    -- 원본 그대로
  norm_key  text NOT NULL,    -- NFC + trim + lower + 공백/언더스코어 통일 후
  target_id uuid NOT NULL,
  UNIQUE (owner_id, scope, norm_key)
);
CREATE INDEX aliases_lookup_idx ON aliases (owner_id, scope, norm_key);

-- 임포트 배치 추적 (재임포트 시 멱등성 확보)
CREATE TABLE import_batches (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id    uuid NOT NULL,
  file_name   text NOT NULL,
  file_hash   bytea NOT NULL,   -- SHA-256
  year        int  NOT NULL,
  month       int  NOT NULL,
  row_count   int  NOT NULL,
  imported_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (owner_id, file_hash)
);

-- 엑셀 한 행을 그대로 보존 (원본 진실의 원천)
CREATE TABLE transactions_raw (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id        uuid NOT NULL,
  import_batch_id uuid NOT NULL REFERENCES import_batches(id) ON DELETE CASCADE,
  row_index       int  NOT NULL,
  group_id        uuid NOT NULL,         -- 같은 영수증을 묶는 키 (헤더/자식 공유)
  is_group_header boolean NOT NULL,      -- true면 헤더 행(컬럼 E 지출(합계) 보유)
  occurred_on     date,                  -- 파싱 실패 가능 → null 허용
  raw_date_serial double precision,
  merchant_text   text,
  actor_text      text,
  category_text   text,
  total_amount    numeric(15,2),         -- 헤더만 채움. 자식은 NULL
  memo            text,
  unit_price      numeric(15,4),         -- 단가는 numeric(15,4) (소수점 단가 대응)
  quantity        numeric(15,4),
  line_amount     numeric(15,2),
  payment_text    text,
  evidence_text   text,
  extras          jsonb                  -- 컬럼 11/12 잡 데이터
);
CREATE INDEX transactions_raw_date_idx ON transactions_raw (owner_id, occurred_on);
CREATE INDEX transactions_raw_group_idx ON transactions_raw (owner_id, group_id);

-- 정규화된 거래 (대시보드/집계의 소스). 라인 단위 저장.
CREATE TABLE transactions (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id          uuid NOT NULL,
  raw_id            uuid NOT NULL REFERENCES transactions_raw(id) ON DELETE CASCADE,
  group_id          uuid NOT NULL,       -- transactions_raw.group_id와 동일
  occurred_on       date NOT NULL,
  merchant_id       uuid REFERENCES merchants(id),
  actor_id          uuid REFERENCES ledger_actors(id),
  category_id       uuid REFERENCES categories(id),
  product_id        uuid REFERENCES products(id),   -- 메모 있는 행만 채움
  payment_method_id uuid REFERENCES payment_methods(id),
  amount            numeric(15,2) NOT NULL,  -- 항상 양수 (line_amount의 절댓값)
  sign              smallint NOT NULL CHECK (sign IN (-1, 1)),  -- -1=수입(회수), 1=지출
  unit_price        numeric(15,4),
  quantity          numeric(15,4),
  memo              text
);
CREATE INDEX transactions_date_idx ON transactions (owner_id, occurred_on DESC);
CREATE INDEX transactions_category_idx ON transactions (owner_id, category_id, occurred_on);
CREATE INDEX transactions_merchant_idx ON transactions (owner_id, merchant_id, occurred_on);
CREATE INDEX transactions_product_idx ON transactions (owner_id, product_id, occurred_on);
CREATE INDEX transactions_group_idx ON transactions (owner_id, group_id);

-- 정산 뷰: 차감을 분리한 공동 정산 산출
-- 집계 시트의 "경비인정 / 차감 / 입금액" 산식을 SQL로 재현
CREATE VIEW v_monthly_settlement AS
SELECT
  t.owner_id,
  date_trunc('month', t.occurred_on)::date AS month,
  SUM(CASE WHEN c.name = '차감' THEN 0 ELSE t.amount END)
    FILTER (WHERE actor.name = '공동' AND t.sign = 1)         AS recognized_expense,
  SUM(t.amount) FILTER (WHERE c.name = '차감')                AS deducted_amount,
  SUM(CASE WHEN c.name = '차감' THEN -t.amount ELSE t.amount END)
    FILTER (WHERE actor.name = '공동' AND t.sign = 1)         AS settlement_input
FROM transactions t
JOIN categories c         ON c.id     = t.category_id
JOIN ledger_actors actor  ON actor.id = t.actor_id
GROUP BY t.owner_id, date_trunc('month', t.occurred_on);
