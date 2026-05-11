# /aliases 수동 병합 개선 설계

날짜: 2026-05-11

## 배경

현재 `/aliases` 페이지의 merge 기능은 백엔드가 미리 계산한 추천 후보(Levenshtein ≤1 또는 norm_key 공유)가 있을 때만 활성화된다. 후보가 없으면 버튼 자체가 비활성되어, 사용자가 직접 알아보고 합치고 싶은 항목이 있어도 조작 불가능하다.

또한 결제수단의 actor(엉아/아기) 분류가 잘못됐을 때 수정하는 UI가 없다.

## 목표

1. 추천 없이도 사용자가 직접 검색해서 merge 대상을 선택할 수 있게 한다.
2. confirmed 항목끼리도 merge 가능하게 한다 (카테고리 기준).
3. 결제수단 actor 분류를 언제든 수정할 수 있게 한다.

## 범위

- 주요 탭: 카테고리, 상품 (가맹점은 클러스터 탭 수동 모드가 이미 존재)
- 결제수단은 merge 아닌 actor 재배정

---

## Feature 1 — Merge Dialog 직접 검색

### 개요

기존 merge dialog에 "직접 검색" 섹션을 추가한다. 추천 후보와 항상 함께 표시되며, 추천이 없어도 검색창으로 대상을 찾아 선택할 수 있다.

### UX 흐름

1. alias 탭(카테고리/상품/가맹점)에서 merge 버튼 클릭
2. dialog 상단: 기존 추천 후보 목록 (없으면 "추천 후보 없음" 표시)
3. dialog 하단: 직접 검색 섹션 — text input + 검색 결과 목록
4. 추천 후보와 검색 결과 중 하나만 선택 가능 (라디오 동작)
5. 선택 확정 후 기존 merge API 호출

### 백엔드

새 검색 엔드포인트 3개 추가:

```
GET /api/categories?q={query}   → 최대 20건, confirmed 포함 전체 검색
GET /api/merchants?q={query}    → 동일
GET /api/products?q={query}     → 동일
```

- `q` 파라미터: 이름에 대한 ILIKE `%query%` 검색
- 응답: `[{ id, name, review_state }]`
- merge 실행: 기존 `POST /api/aliases` (action=merge) 그대로

### 프론트엔드

- `MergeDialog` 컴포넌트에 검색 입력창 + 결과 목록 추가
- 검색은 300ms debounce, 2자 이상 입력 시 실행
- 추천 후보 선택 시 검색 선택 해제, 그 반대도 동일
- 기존 테스트: `MergeDialog` 관련 테스트에 검색 케이스 추가

### 제약

- `차감` 카테고리는 target 선택 불가 (기존 backend 409 그대로 커버)
- 검색 결과에서 자기 자신은 제외

---

## Feature 2 — 클러스터 탭 카테고리 scope 추가

### 개요

클러스터 탭의 scope 드롭다운에 카테고리를 추가한다. 결제수단은 merge 대신 actor 재배정(Feature 3)으로 처리하므로 포함하지 않는다.

### UX 흐름

- 추천 모드: `GET /api/clusters?scope=category` — 기존 trigram 유사도 그대로 사용 (카테고리 이름이 짧아 결과가 적을 수 있음)
- 수동 모드: `GET /api/categories` 전체 목록 → 로컬 검색 필터, 2개 이상 선택 + 대표 1개 지정 후 병합

### 백엔드

- `GET /api/clusters?scope=category` — 기존 clusters 엔드포인트에 `category` scope 처리 추가
- `POST /api/clusters/merge` (action: scope=category) — 기존 그대로
- `GET /api/categories` (전체 목록, 수동 모드용) — Feature 1의 검색 엔드포인트와 통합 가능 (`q` 없이 호출 시 전체 반환)

### 프론트엔드

- `ClusterTab` scope 드롭다운에 `category` 추가
- 수동 모드: 기존 `ManualMergePanel` 컴포넌트 재사용 (scope만 카테고리로)
- `차감` 카테고리 선택 시 병합 버튼 비활성 + 안내 메시지

---

## Feature 3 — 결제수단 Actor 인라인 토글

### 개요

결제수단 탭의 전체 목록(pending + confirmed)에 엉아/아기 토글을 상시 표시한다. 분류가 잘못됐을 때 언제든 수정 가능하다.

### UX 흐름

- 결제수단 탭: 각 행에 `[아기] [엉아]` 토글 버튼 표시
- 토글 변경 시 즉시 저장 (별도 확정 버튼 없음)
- pending 항목: 토글 변경 후 "확정" 버튼으로 review_state도 confirmed로 변경
- confirmed 항목: 토글 변경만으로 즉시 actor 수정 (review_state 변경 없음)

### 백엔드

신규 엔드포인트:

```
PATCH /api/payment-methods/:id/actor
Body: { actor_id: UUID }
Response: { id, name, actor_id, review_state }
```

- `actor_id`는 `ledger_actors` 테이블의 유효한 ID여야 함
- 존재하지 않는 payment_method id → 404
- 유효하지 않은 actor_id → 400

### 프론트엔드

- 결제수단 탭 컴포넌트에 actor 토글 UI 추가
- 기존 `/api/aliases-proxy` 통해 프록시하거나 별도 payment-methods-proxy 신설
- 낙관적 업데이트(optimistic update) — 토글 즉시 반영, 실패 시 rollback

### 데이터 영향

- `payment_methods.actor_id` 변경 → 해당 결제수단의 모든 거래가 새 actor로 즉시 반영
- 트랜잭션 rows 자체는 건드리지 않음 (payment_method → actor 참조 구조 그대로)

---

## 구현 우선순위

| 순위 | Feature | 이유 |
|------|---------|------|
| 1 | Merge Dialog 검색 | 즉각적인 불편 해소, 구현 범위 작음 |
| 2 | 결제수단 actor 토글 | 분류 수정이 중요한 운영 작업 |
| 3 | 클러스터 카테고리 확장 | 클러스터 탭 이미 있어 상대적으로 덜 급함 |

세 feature는 서로 독립적이므로 순서대로 구현 가능하다.

---

## 테스트 계획

### 백엔드

- Feature 1: `GET /api/categories?q=외식` 검색 결과 반환, 자기 자신 제외, 차감 포함 여부
- Feature 2: `GET /api/clusters?scope=category` 결과, `POST /api/clusters/merge` scope=category
- Feature 3: `PATCH /api/payment-methods/:id/actor` 정상/404/400 케이스

### 프론트엔드

- Feature 1: 검색창 debounce, 추천↔검색 라디오 상호 해제, merge 성공/실패
- Feature 2: 카테고리 scope 드롭다운, 차감 선택 시 비활성
- Feature 3: 토글 클릭 → 낙관적 업데이트, 실패 시 rollback
