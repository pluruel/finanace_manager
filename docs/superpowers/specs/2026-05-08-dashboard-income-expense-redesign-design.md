# Dashboard Income/Expense Redesign

**Status:** Approved 2026-05-08
**Owner:** Junnoh Lee
**Scope:** `server/src/import/pipeline.rs` (휴리스틱 1곳) + `web/` (도넛 그리드 재구성)

## 동기

`(app)/page.tsx` 의 도넛 그리드가 수입/지출을 한 도넛에 섞어서 다음 문제가 동시에 발생.

- 급여·회수·수입 기타 카테고리가 `kind='expense'` 상태로 남아 지출 도넛에 거대한 음수 슬라이스로 등장. 도넛 중앙 합계가 `-₩5,168,173` 처럼 음수.
- 같은 이유로 IncomeStrip(`/api/summary/income`) 가 ₩0.
- 퍼센티지가 부호 섞인 net total 을 분모로 잡아 `2856.2%` 같이 폭주.
- 시각적으로 수입과 지출이 구분되지 않음.
- "공동" 카드가 공동-actor 만의 지출이라, 가구 전체 흐름이 한눈에 안 들어옴.
- 차감(deduction)은 정산 성격인데 일반 지출 슬라이스 사이에 끼어 있어 결이 다름.

## 목표

1. 수입은 빨간색 텍스트로 카드 헤더에 표시. 지출 도넛은 파란 팔레트.
2. 차감은 별도 도넛 카드로 분리.
3. 도넛 카드 순서를 `가구 합계 / 아기 / 엉아` 로 고정. 공동 actor 전용 뷰는 도넛에서 제거 (정산 카드 / 엑셀 export 에는 그대로 남음).
4. 신규 카테고리 생성 시 이름 휴리스틱(`급여|수입|회수|환급`) 으로 `kind='income'` 자동 설정. 기존 ON CONFLICT DO NOTHING 으로 한 번 생성된 카테고리의 kind 는 보존.

## 비목표

- 새 엔드포인트나 스키마 변경 없음. 기존 `/api/summary/:y/:m`(expense), `/api/summary/income/:y/:m` 그대로 사용.
- 다중월 비교 / 가구 룰(예: 외식 1인당 15,000원 한도) UI — 별도 마일스톤.
- 일회성 데이터 백필 스크립트 — 사용자가 wipe + re-import 할 예정. 휴리스틱이 다음 import 때 자연 적용됨.

## 페이지 레이아웃

위 → 아래:

1. **헤더 행** (현행 유지): "대시보드" 제목 · `MonthPicker` · "Excel 다운로드"
2. **정산 카드** (현행 compact)
3. **지출 도넛 그리드** (3열, 모바일 1열): `가구 합계` / `아기` / `엉아`
4. **차감 도넛 카드** (1행, full width): 액터별 슬라이스 1개

`IncomeStrip` 행 제거.

## 카드 사양

### 지출 도넛 카드 (3장)

```
┌─ {actorName} ──────────────────┐
│ 수입  ₩X,XXX,XXX  (빨강)         │  ← 수입>0 일 때만, 0이면 행 자체 숨김
│                                │
│        ╭───────╮                │
│       ╱  지출   ╲               │
│      │ ₩XXX,XXX│                │
│       ╲       ╱                 │
│        ╰───────╯                │
│ ● {cat}  ₩XXX,XXX · NN.N%       │
│ ● {cat}  ₩XXX,XXX · NN.N%       │
│ ● 기타   ₩XXX,XXX · NN.N%       │
└────────────────────────────────┘
```

- **슬라이스 소스**: `kind='expense' AND category_name != '차감'` 인 카테고리.
- **그룹핑**: 절대값 기준 top-6 + 나머지를 단일 `기타`. (현행 `buildActorSlices` 와 동일하나, 차감 처리 라인을 제거.)
- **퍼센티지**: `Math.abs(slice.value) / Σ Math.abs(slice.value)`. 부호 섞임에 안전, 100% 수렴.
- **중앙 라벨**: "지출" 라벨 + 합계(부호 그대로). 환불 우세 액터는 음수도 가능.
- **수입 헤더**: `income.by_actor[actorId].total` (가구 합계 카드는 `income.total`). 0 이거나 음수면 비표시.
- **렌더 룰**:
  - 수입 > 0 → 빨간 수입 헤더 행 표시.
  - 슬라이스 ≥ 1 → 도넛 + 중앙 라벨 + 범례 표시.
  - 슬라이스 = 0 && 수입 > 0 → 도넛 자리에 작은 placeholder ("이 달 지출 없음"), 헤더는 정상.
  - 슬라이스 = 0 && 수입 = 0 → 카드 전체에 "이 달의 거래 내역이 없습니다" 단일 텍스트.

### 가구 합계 카드 (도넛 그리드 1번 슬롯)

- 슬라이스 = 모든 actor 의 expense (차감 제외) 를 카테고리별로 합산. 동일한 top-6 + 기타 규칙.
- 수입 헤더 = `income.total`.
- 카드 라벨 = "가구 합계".

### 차감 도넛 카드 (도넛 그리드 아래)

```
┌─ 차감 ───────────────────────────────┐
│        ╭─────╮                      │
│       │₩X,XXX│                      │
│        ╰─────╯                      │
│ ● 아기  ₩X,XXX · NN.N%               │
│ ● 공동  ₩X,XXX · NN.N%               │
│ ● 엉아  ₩X,XXX · NN.N%               │
└──────────────────────────────────────┘
```

- **슬라이스 소스**: `kind='expense' AND category_name == '차감'` 인 row 들을 액터별로 묶음.
- **슬라이스 1개 = actor 1명**. 차감 0 원인 actor 는 슬라이스에서 제외(빈 슬라이스 회피).
- **팔레트**: 회색조 (가독성 위해 명도 차이만 두는 grayscale 4단). `차감` 자체가 회색 문맥이므로 액터 색 통일 안 함.
- **중앙 라벨**: 총 차감액.
- **빈 카드**: 모든 actor 차감 0 → 카드 자체 미렌더(섹션 통째 생략).

## 슬라이스 빌더 (정확한 룰)

`web/lib/donut-data.ts` 에 다음 순수 함수 4개:

```ts
buildActorSlices(summary, actorId)        // 단일 actor, 차감 제외
buildHouseholdSlices(summary)             // 모든 actor 합산, 차감 제외
buildDeductionByActor(summary)            // 차감 row 만, actor 슬라이스
incomeFor(income, actorRef)               // actorRef = actorId | "household"
```

공통 처리:
1. `cat.by_actor[*].amount` 를 `parseFloat`. NaN 또는 0 인 셀 스킵.
2. 차감 분리: `category_name === '차감'` 은 expense 빌더에서 제외, 차감 빌더에만 포함.
3. expense 빌더는 `Math.abs(value)` 내림차순 정렬, top-6 + 나머지 `기타`.
4. 퍼센티지는 컴포넌트에서 `Math.abs(slice.value) / Σ Math.abs(slice.value)`.

## 색상 토큰

```ts
EXPENSE_PALETTE = [
  "#1e40af", "#2563eb", "#3b82f6",   // blue 800/600/500
  "#0891b2", "#0e7490", "#6366f1",   // cyan/indigo
];
OTHER_COLOR     = "#94a3b8";   // slate-400 (현행 유지)
INCOME_COLOR    = "#dc2626";   // red-600 (헤더 텍스트)
DEDUCTION_PALETTE = ["#4b5563", "#6b7280", "#9ca3af", "#d1d5db"];  // grayscale
```

기존 `PALETTE`/`DEDUCTION_COLOR` 상수는 새 토큰으로 대체.

## 백엔드 변경

### `server/src/import/pipeline.rs`

`upsert_category` 의 INSERT 문(현재 라인 49-60):

```rust
const INCOME_KEYWORDS: &[&str] = &["급여", "수입", "회수", "환급"];

let kind = if INCOME_KEYWORDS.iter().any(|kw| norm.contains(kw)) {
    "income"
} else {
    "expense"
};

let cat_id_opt: Option<Uuid> = sqlx::query_scalar!(
    r#"INSERT INTO categories (owner_id, name, kind, review_state)
       VALUES ($1, $2, $3, $4)
       ON CONFLICT (owner_id, name) WHERE parent_id IS NULL DO NOTHING
       RETURNING id"#,
    owner_id, norm, kind, review_state,
).fetch_optional(&mut *conn).await?;
```

ON CONFLICT 분기에서 기존 row 의 kind 를 건드리지 않음 → 사용자가 토글했거나, 휴리스틱이 잘못 분류했다가 토글 받은 분류 모두 보존됨.

차감 카테고리(`norm == "차감"`)는 INCOME_KEYWORDS 에 매치되지 않으므로 expense 로 자연 생성. 변경 없음.

### 다른 파일

수정 없음. `summary.rs` 의 `c.kind = 'expense'` 필터는 그대로(차감도 expense). `income.rs` 도 그대로.

## 프런트 파일 변경

### 수정

- **`web/lib/donut-data.ts`** — 위 4개 함수 구현, 새 색상 토큰 export.
- **`web/components/actor-donut.tsx`** — 헤더 수입 행 추가, 중앙 "지출 ₩X" 라벨, 차감 special-case 제거, 퍼센티지 공식 변경.
- **`web/components/dashboard-donuts.tsx`** — props `{ summary, income }`. 카드 3장 고정 순서: 가구합계 → 아기 → 엉아. 액터 이름 매칭(`actor_name === '아기'` 등)으로 `summary.actors` 에서 actor_id lookup.
- **`web/app/(app)/page.tsx`** — `IncomeSection` 삭제. `fetchIncome` 결과를 `DashboardDonutsSection` 으로 전달. 새 `DeductionDonutSection` 추가.

### 신규

- **`web/components/deduction-donut.tsx`** — 차감 전용 카드. props `{ summary }`. 내부에서 `buildDeductionByActor` 호출. 0 슬라이스면 `null` 반환 → 페이지에서 자연 생략.

### 삭제

- **`web/components/income-strip.tsx`** — 콘텐츠가 액터 카드 헤더로 흡수됨. `dashboard.test.tsx` 에서 IncomeStrip 관련 단언도 함께 제거.

## 데이터 플로

```
page.tsx
  ├─ SettlementSection  (현행)
  ├─ DashboardDonutsSection (server)
  │    ├─ fetch /api/summary/:y/:m   → SummaryResponse
  │    ├─ fetch /api/summary/income/:y/:m → IncomeResponse
  │    └─ DashboardDonuts({ summary, income })
  │         └─ ActorDonut × 3  (가구합계 / 아기 / 엉아)
  └─ DeductionDonutSection (server)
       ├─ 위 SummaryResponse 재사용 (Suspense 키 분리하여 병렬 fetch도 OK)
       └─ DeductionDonut({ summary })
```

가구 합계 카드와 차감 카드 모두 `SummaryResponse` 한 번만 있으면 그릴 수 있으므로, 페이지에서 fetch 1회 후 두 섹션에 prop drilling 하거나, 두 섹션이 각자 fetch (Suspense 단위로 명확) 둘 다 가능. 구현 시 후자 선호 — 현행 페이지 패턴(섹션별 독립 fetch)에 일관.

## 테스트

### 백엔드

**신규: `server/tests/test_import_kind_heuristic.rs`** (또는 기존 `test_m1.rs` 에 추가)

| 케이스 | 기대 |
|--------|------|
| 신규 카테고리 "급여" upsert | DB row 의 kind = 'income' |
| 신규 카테고리 "수입 기타" upsert | kind = 'income' |
| 신규 카테고리 "회수" upsert | kind = 'income' |
| 신규 카테고리 "환급" upsert | kind = 'income' |
| 신규 카테고리 "외식" upsert | kind = 'expense' |
| 신규 카테고리 "차감" upsert | kind = 'expense' |
| "외식" 을 expense 로 만든 뒤 income 으로 수동 UPDATE 후 동일 이름 재 upsert | kind 가 'income' 그대로 (ON CONFLICT 무동작) |

### 프런트

**`web/__tests__/donut-data.test.ts`** (수정)

기존 13개 케이스 중 차감 슬라이스 관련은 새 함수로 이동. 신규/수정 케이스:

- `buildActorSlices` 가 `차감` 카테고리를 슬라이스에서 제외
- `buildHouseholdSlices` 가 모든 actor 의 expense 를 카테고리별로 합산, 차감 제외
- `buildDeductionByActor` 가 액터별 1 슬라이스 반환, 차감 0 인 actor 는 결과에서 제외
- `incomeFor(income, actorId)` / `incomeFor(income, "household")` 동작
- 퍼센티지 헬퍼 (또는 컴포넌트 inline) 가 `Σ|value|` 분모로 100% 합 (±0.5% 허용)

**`web/__tests__/dashboard.test.tsx`** (수정)

- 카드 순서가 가구합계 / 아기 / 엉아 (`getAllByTestId('actor-donut-...')` 로 ordered match)
- 수입 > 0 인 액터 카드에 빨간 수입 헤더 텍스트
- 수입 = 0 액터는 수입 헤더 행 미렌더
- `IncomeStrip` DOM 미존재
- 차감 데이터가 있는 픽스처에서 차감 카드 렌더, 모든 액터 차감 0 인 픽스처에서는 미렌더

기존 IncomeStrip 단위 테스트 (`income-strip.test.tsx`) 가 있다면 함께 삭제.

## 엣지 케이스

| 케이스 | 동작 |
|--------|------|
| 수입 없는 actor (예: 아기 income=0) | 카드 헤더 수입 행 숨김. 도넛은 그대로. |
| 지출 0 + 수입만 있는 actor | 카드 렌더. 빨간 수입 헤더 + 도넛 자리 placeholder "이 달 지출 없음". |
| 지출도 수입도 0 인 actor | 빈 카드 placeholder ("이 달의 거래 내역이 없습니다") |
| 모든 액터 차감 0 | 차감 카드 섹션 통째 미렌더 |
| `actor_id == null` row | `summary.actors` 에 없으므로 가구합계에는 포함되나 개별 카드에는 표시 안 됨. 현행과 동일. |
| 차감 카테고리에 actor_id NULL row | "(미지정)" 슬라이스로 렌더 |
| 신규 import 후 사용자가 자동 분류된 income 카테고리를 expense 로 토글 | DB UPDATE 로 변경. 다음 import 시 ON CONFLICT DO NOTHING 이므로 사용자 분류 보존 |

## 위험 / 결정 사항

- **휴리스틱 false positive**: 카테고리 이름에 "회수" 가 들어가지만 실제로는 expense 인 케이스 (예: "회수권 외식")? — 현재 사용자의 카테고리 명명 관습상 보고된 적 없음. 발생 시 사용자가 /aliases Categories 탭에서 토글하면 영구 보존됨.
- **휴리스틱 false negative**: "보너스" 처럼 키워드 미포함 income — 동일하게 사용자 토글로 처리. 휴리스틱은 보조이지 정답이 아님을 문서화 (CLAUDE.md cumulative context).
- **공동 actor 도넛 제거**: 정산 카드와 엑셀 export 에 공동 actor 데이터가 그대로 보존되므로 정보 손실 없음. 다만 "공동만 빠르게 보고싶다" 워크플로가 한 단계 늘어남 — 추후 actor 필터 토글 추가 가능 (별도 마일스톤).
- **차감 도넛 슬라이스 단위가 actor 인지 sub-category 인지**: 향후 차감 sub-category 가 늘면 actor × sub-category 두 축이 됨. 일단 actor 1축으로 두고, sub-category 가 실데이터에 등장하면 그때 재설계.

## 명시적 out-of-scope

- 다중월 overlay, 가구 룰 페이지 — 기존 deferred 유지.
- 차감 sub-category 그룹핑.
- `/transactions`, `/aliases`, `/price-history` UI 변경.
- 백엔드 집계 변경 (top-N 서버 이관 등).
