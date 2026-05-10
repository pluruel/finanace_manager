-- Enable pgcrypto for gen_random_uuid()
CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- 가계부 내부 사용자(공동/엉아/아기) — 로그인 계정과 무관한 라벨.
-- 공동: 엉아(배우자)와 본인이 함께한 공동 지출
-- 엉아: 배우자를 위한 지출
-- 아기: 아기를 위한 지출
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
               CHECK (review_state IN ('pending','confirmed'))
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
               CHECK (review_state IN ('pending','confirmed'))
);

-- 결제수단: 공동 카드는 없다. 모든 결제수단은 엉아 또는 아기 소유.
-- 아기 소유: 농협, 신한아기, 롯데, 삼성, 국민, 비씨, 현대, 현금아기
-- 엉아 소유: 현금, 신한, 하나, 씨티클, 현금엉아
CREATE TABLE payment_methods (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL,
  name         text NOT NULL,
  actor_id     uuid REFERENCES ledger_actors(id),  -- 결제수단 소유자 (NULL=미매핑)
  review_state text NOT NULL DEFAULT 'pending'
               CHECK (review_state IN ('pending','confirmed')),
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

-- Partial unique indexes for categories and products.
-- PostgreSQL treats two NULLs as distinct in ordinary UNIQUE constraints, so
-- (owner_id, NULL, name) duplicates would not be caught. Partial indexes close
-- that gap and enable ON CONFLICT targeting without advisory locks.
CREATE UNIQUE INDEX categories_owner_name_root_uniq
  ON categories (owner_id, name) WHERE parent_id IS NULL;
CREATE UNIQUE INDEX categories_owner_parent_name_uniq
  ON categories (owner_id, parent_id, name) WHERE parent_id IS NOT NULL;

CREATE UNIQUE INDEX products_owner_merchant_name_uniq
  ON products (owner_id, merchant_id, name) WHERE merchant_id IS NOT NULL;
CREATE UNIQUE INDEX products_owner_name_no_merchant_uniq
  ON products (owner_id, name) WHERE merchant_id IS NULL;

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
-- amount 는 캐시플로우 부호: 현금 유입 양수, 유출 음수.
-- 임포트 시 엑셀 라인 금액의 부호를 반전해서 저장한다 (엑셀은 지출 장부 관점이므로).
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
  amount            numeric(15,2) NOT NULL,  -- 캐시플로우 부호 (유입+/유출-)
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
-- 저장 규약상 지출은 음수, 차감도 음수, 환불은 양수.
-- 정산 카드는 양수로 표시하므로 모두 -SUM(...) 로 양수화한다.
-- recognized_expense: 공동 actor 의 일반 지출(차감 제외) 양수화
-- deducted_amount: 차감 카테고리 합계(actor 무관) 양수화
-- settlement_input: recognized_expense - deducted_amount
-- 수입(kind='income')은 정산에 포함하지 않는다 (도메인 규칙).
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
