# 액터 카드 수입 도넛 추가 — Design

작성일: 2026-05-08
관련: `2026-05-08-dashboard-income-expense-redesign-design.md`

## 배경

현재 대시보드의 액터 도넛 카드(`web/components/actor-donut.tsx`)는 카드당 헤더 → "수입 ₩X" 텍스트 줄 → 지출 도넛(중앙 "지출 ₩X") + 카테고리 범례 구조다. 사용자 요청: 수입을 텍스트 줄이 아닌 **도넛**으로 시각화하되, 카테고리별 수치 범례는 노출하지 않는다.

## 결정 사항(브레인스토밍 합의)

- 수입 도넛은 차트 + 가운데 라벨("수입 ₩X")만 표시, 카테고리 범례 없음.
- 색 팔레트는 지출과 동일한 `EXPENSE_PALETTE` 재사용.
- 같은 카드 안에 세로 스택: 수입 도넛(위) → 지출 도넛(아래) → 지출 범례.
- 두 도넛은 동일 크기(`h-44`)로 균형 유지.

## 구현 범위

### 1. 백엔드 — `/api/summary/income/:year/:month` 확장

`server/src/api/income.rs::IncomeResponse` 에 `categories` 필드를 **추가**한다(기존 `by_actor`/`total` 유지, additive change).

```rust
pub struct IncomeResponse {
    pub month: String,                            // 기존
    pub by_actor: Vec<IncomeByActor>,             // 기존
    pub total: Decimal,                           // 기존
    pub categories: Vec<CategorySummary>,         // 신규
}
```

`CategorySummary` / `ByActorEntry` / `ActorRef` 의 모양은 `summary.rs` 와 **동일 형식**으로 노출한다. 코드 중복을 줄이기 위해 `summary.rs` 의 타입을 재export 하거나 income.rs 에서 동일 구조의 타입을 정의한다(둘 다 가능; 구현 단계에서 결정).

#### SQL

`summary.rs::handle_get_summary` 의 SQL 패턴을 따르되 두 가지가 다르다:

- 필터: `c.kind = 'income'`
- 부호: 수입은 저장상 양수이므로 `SUM(t.amount)` 그대로(부호 뒤집지 않음).

```sql
SELECT
    c.id, c.name, c.kind,
    a.id, a.name,
    SUM(t.amount)::numeric(15,2) AS amount
FROM transactions t
JOIN categories c         ON c.id = t.category_id AND c.owner_id = t.owner_id
LEFT JOIN ledger_actors a ON a.id = t.actor_id    AND a.owner_id = t.owner_id
WHERE t.owner_id = $1
  AND c.kind = 'income'
  AND t.occurred_on >= make_date($2, $3, 1)
  AND t.occurred_on  < make_date($2, $3, 1) + INTERVAL '1 month'
GROUP BY c.id, c.name, c.kind, a.id, a.name
ORDER BY c.name, a.name
```

기존 `by_actor` / `total` 합계는 별도 쿼리 그대로 유지(또는 categories 에서 파생). 코드 단순성 위해 별도 쿼리 유지를 권장.

#### 테스트

- `tests/test_income_endpoint.rs`(또는 기존 income 테스트 파일)에 **2개** 추가:
  1. `income_endpoint_returns_categories_breakdown`: 골든 데이터 임포트 후 `categories` 가 비어있지 않고 액터별 셀에 양수 값.
  2. `income_categories_only_include_income_kind`: `kind='expense'` 카테고리는 응답에 없음.

### 2. 프론트엔드 — 데이터 레이어

`web/lib/schemas.ts`:
- `IncomeResponseSchema` 에 `categories: z.array(CategorySummarySchema)` 추가(기존 `CategorySummarySchema` 재사용).

`web/lib/donut-data.ts`:
- 신규 `buildActorIncomeSlices(income: IncomeResponse | null, actorId: string | null): ActorDonutData`
  - `income.categories` 를 순회, 해당 actorId 셀의 양수 값을 슬라이스로. `차감` 제외 로직 **불필요**.
  - 정렬·top-N·기타 묶음은 expense 와 동일하게 `topNWithOther` 재사용.
  - 색은 `EXPENSE_PALETTE` 재사용.
- 신규 `buildHouseholdIncomeSlices(income: IncomeResponse | null): ActorDonutData`
  - 카테고리별 모든 액터 합산, expense `buildHouseholdSlices` 와 대칭 구조.
- `incomeFor` 헬퍼는 그대로 유지(현재는 ActorDonut 에서 단순 total 표시에 쓰이는데, 본 작업으로 호출이 사라짐). 외부 사용처가 없다면 함께 정리 가능 — 단 관련 테스트 영향 확인 필요.

### 3. 프론트엔드 — 컴포넌트

`web/components/actor-donut.tsx`:
- Props 변경:
  ```ts
  type Props = {
    expense: ActorDonutData;   // 기존 data 를 명확화
    income: ActorDonutData;    // 신규
  };
  ```
- 헤더 아래 `수입 ₩X` 텍스트 줄(현 36–46행) 제거.
- 그 자리에 **수입 도넛 블록**: 지출 도넛과 동일 구조(`h-44` ResponsiveContainer + 중앙 "수입 ₩X" 라벨), 단 카테고리 범례 `<ul>` 없음.
- 수입 슬라이스가 비어있으면 수입 도넛 영역을 렌더하지 않음(현재 `hasIncome` 조건과 동일 의미).
- 지출 도넛 + 범례는 그대로.
- `hasNothing` 조건은 `expense.slices` 와 `income.slices` 모두 비었을 때.

`web/components/dashboard-donuts.tsx`:
- 가구·각 액터별 expense/income 두 `ActorDonutData` 생성해 `ActorDonut` 에 전달.
- `incomeFor` 호출 제거.

### 4. 테스트

**프론트엔드:**
- `donut-data.test.ts`:
  - `buildActorIncomeSlices` — actorId null/특정 액터, 빈 입력, top-N 컷, household 합산. 약 4–5 케이스.
  - 차감 카테고리가 income 빌더에서 제외/처리되는지(데이터상 차감은 expense kind 이므로 자연 제외).
- `dashboard.test.tsx`:
  - 기존 `donut-income` 텍스트 단언 제거.
  - 신규: `donut-income-chart` testid 가 income > 0 카드에서 존재, income == 0 카드에서 부재.
  - 가운데 "수입 ₩X" 라벨 단언.
- recharts mock 은 기존 패턴(`price-history.test.tsx` 참조) 그대로.

**백엔드:**
- 위 §1 의 income endpoint 테스트 2개.

### 5. 마이그레이션 / 데이터

- 스키마 변경 없음.
- 재임포트 불필요.

### 6. 영향 범위 / 비호환

- `IncomeResponse` 추가 필드는 호환 유지(consumer 가 무시 가능).
- `ActorDonut` props 변경은 내부 컴포넌트(외부 export 없음). `dashboard-donuts.tsx` 만 수정.
- `IncomeStrip` 은 2026-05-08 redesign 에서 이미 제거됨 — 영향 없음.

## 비스코프 (YAGNI)

- 수입 카테고리 범례 표시 — 사용자 명시적 요청에 따라 제외.
- 수입 도넛에 `차감` 분리 — 차감은 expense kind 이므로 income 도넛에 등장하지 않음(별도 처리 불요).
- 수입 카테고리 색을 별도 팔레트로 — 명시적으로 EXPENSE_PALETTE 재사용 결정.
- 수입 도넛 클릭/툴팁 동작 — 기존 expense 도넛과 동일한 Recharts Tooltip 만.

## 수용 기준

1. 골든 데이터(`2026년 02월.xlsx`) 기준 가구 합계 카드에 수입 도넛 + "수입 ₩1,120,588" 라벨 렌더(현 텍스트와 동일 값).
2. 아기 카드는 income == 0 이면 수입 도넛 영역 미렌더.
3. 엉아 카드 수입 도넛 중앙 라벨 "수입 ₩5,950,643".
4. 수입 도넛에 카테고리 금액·% 범례 없음.
5. 두 도넛 동일 크기(`h-44`).
6. `cargo test -p server` 및 `npm test` 통과.
