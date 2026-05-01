-- v_monthly_settlement 뷰 수정
-- PLAN §1의 원래 정의를 유지하되, settlement_input을 올바르게 계산
-- 집계 시트: 경비인정액 - 차감 = 입금액 (2026-02: 584,000 - 7,500 = 576,500)
--
-- '차감' 카테고리는 '엉아' actor에 속하므로
-- actor='공동' 필터와 분리하여 처리해야 함
--
-- recognized_expense: 공동 actor의 지출 합계 (차감 제외)
-- deducted_amount: 차감 카테고리의 금액 합계 (actor 무관)
-- settlement_input: recognized_expense - deducted_amount

DROP VIEW IF EXISTS v_monthly_settlement;

CREATE VIEW v_monthly_settlement AS
SELECT
  t.owner_id,
  date_trunc('month', t.occurred_on)::date AS month,
  -- 공동 actor의 일반 지출 (차감 제외)
  COALESCE(SUM(t.amount) FILTER (WHERE actor.name = '공동' AND t.sign = 1 AND c.name <> '차감'), 0)
    AS recognized_expense,
  -- 차감 카테고리 총합 (actor 무관)
  COALESCE(SUM(t.amount) FILTER (WHERE c.name = '차감'), 0)
    AS deducted_amount,
  -- 입금액 = 경비인정 - 차감
  COALESCE(SUM(t.amount) FILTER (WHERE actor.name = '공동' AND t.sign = 1 AND c.name <> '차감'), 0)
  - COALESCE(SUM(t.amount) FILTER (WHERE c.name = '차감'), 0)
    AS settlement_input
FROM transactions t
JOIN categories c         ON c.id     = t.category_id
JOIN ledger_actors actor  ON actor.id = t.actor_id
GROUP BY t.owner_id, date_trunc('month', t.occurred_on);
