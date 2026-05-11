# 일괄 클러스터 병합 (Bulk Cluster Merge)

- 작성일: 2026-05-11
- 스코프: products / merchants 의 유사 엔티티를 한 화면에서 묶어 한 번에 병합
- 관련 문서: `MSA_INTEGRATION.md`, 기존 alias 시스템 (`server/src/api/aliases.rs`)

## 1. 문제 정의

같은 제품/가맹점이 표기 차이("고덕방 아메리카노" / "고덕방 아아" / "고덕방 아이스아메리카노")로 인해 `products` / `merchants` 테이블에 별개 row 로 흩어져 있음. 현재 `/aliases` 페이지의 review queue 는 신규 raw 1건 단위 검토만 지원하고, **이미 만들어진 canonical entity 들 자체를 묶는 도구는 없음**.

목표: 사용자가 "사실은 같은 것"인 entity 후보 묶음을 한 화면에서 보고, 대표 하나만 남기고 나머지를 흡수하는 일괄 병합 도구를 제공한다. 진입은 `/aliases` 페이지의 새 탭 단일 통로.

비목표:
- 자동 병합 없음 (사용자가 직접 확정).
- import-time 추천 / 토스트 / 배지 없음. 사용자가 여러 달 import 누적 후 필요할 때 탭에 들어가서 일괄 정리하는 흐름.
- categories / payment_methods 는 이번 스코프 외.
- 의미 임베딩(아아↔아메리카노) 미지원. trigram 한도 내에서 동작.

## 2. 사용자 시나리오

1. 사용자가 월간 엑셀을 여러 달 import 누적한다.
2. 정리할 시점이 되면 `/aliases` → "클러스터" 탭 진입.
3. 서브탭에서 Products 또는 Merchants 선택, 임계치 슬라이더 조정 후 "다시 계산" 클릭.
4. 후보 카드들이 렌더된다. 각 카드에는 비슷한 멤버 N개, 트랜잭션 수 가장 많은 row 가 기본 대표(라디오)로 선택됨, 나머지는 흡수 체크박스 기본 ON.
5. 사용자가 대표 라디오 / 흡수 체크박스를 조정하고 "병합" 클릭.
6. 카드가 사라지고 클러스터 목록이 갱신된다. 토스트로 "N건 병합 완료" 안내.
7. 모든 카드를 처리했거나 만족스러우면 탭을 떠남.

## 3. 아키텍처

### 3.1 구성요소

- **Backend**: `server/src/api/clusters.rs` (신규)
  - `GET  /api/clusters?scope=product|merchant&threshold=0.5`
  - `POST /api/clusters/merge`
- **DB**: `pg_trgm` extension + products/merchants `name` 컬럼 GIN trgm 인덱스
- **Frontend**:
  - `web/app/(app)/aliases/page.tsx` — 5번째 "클러스터" 탭 추가
  - `web/components/cluster-tab.tsx` — Products/Merchants 서브토글, 임계치 슬라이더, 다시계산 버튼
  - `web/components/cluster-card.tsx` — 멤버 리스트, 라디오/체크박스, 병합 버튼
  - `web/lib/cluster-data.ts` — 정렬/표시 헬퍼

import 응답이나 외부 알림과는 연결되지 않는다.

### 3.2 alias 모델 짧은 복습

- `aliases(owner_id, scope, raw_text, norm_key, target_id, UNIQUE(owner_id, scope, norm_key))`
- 컬럼명은 `target_id` (entity_id 아님). review_state 는 alias 가 아니라 target entity (categories/merchants/products/payment_methods) 쪽에 존재.
- 본 기능은 **alias 를 새로 만들지 않는다** (사용자 요청: 이전 값 보존 X, 학습 효과 없이 일괄화).

## 4. DB 변경

### 4.1 정책

`server/migration/src/m20260510_000001_init.rs` 에 in-place 추가. 신규 마이그레이션 파일 X. dev 는 `Migrator::fresh` 로 갈아엎고 골든 엑셀 재import.

### 4.2 추가 항목

```sql
CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX IF NOT EXISTS idx_products_name_trgm
  ON products USING gin (name gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_merchants_name_trgm
  ON merchants USING gin (name gin_trgm_ops);
```

신규 테이블 / 신규 컬럼 없음.

## 5. Backend API

### 5.1 `GET /api/clusters`

쿼리 파라미터:
- `scope` : `product` | `merchant` (필수)
- `threshold` : f32, 기본 0.5, 범위 0.3 ~ 0.9

응답:
```json
{
  "scope": "product",
  "threshold": 0.5,
  "clusters": [
    {
      "members": [
        { "id": "uuid", "name": "고덕방 아이스아메리카노", "txn_count": 6, "latest_seen": "2026-02-28" },
        { "id": "uuid", "name": "고덕방 아아",            "txn_count": 2, "latest_seen": "2026-02-15" }
      ],
      "suggested_canonical_id": "uuid",
      "avg_similarity": 0.62
    }
  ],
  "truncated": false
}
```

계산:
1. 같은 owner 내 페어 추출 — `JOIN ... ON a.id < b.id AND a.name % b.name AND similarity(a.name, b.name) >= $threshold` (GIN trgm 인덱스 사용).
2. Rust 측 union-find 로 컴포넌트 묶음.
3. 멤버별 `txn_count` (transactions 의 product_id/merchant_id 기준 COUNT), `latest_seen` (transactions.occurred_on MAX) 집계.
4. 단일 멤버 컴포넌트 제외.
5. `suggested_canonical_id` = 멤버 중 `txn_count` 최댓값. 동률이면 가나다 첫 번째.
6. 클러스터 정렬: 멤버 수 내림차순.
7. 결과가 200 클러스터 초과 시 truncate, `truncated: true`.

### 5.2 `POST /api/clusters/merge`

요청:
```json
{
  "scope": "product",
  "canonical_id": "uuid",
  "absorb_ids": ["uuid", "uuid"]
}
```

검증:
- `canonical_id` 가 `absorb_ids` 에 포함되면 400.
- 모든 id 가 호출자 owner 소유여야 함 (조건 미충족 시 404).
- `absorb_ids` 비어있으면 400.

처리 (단일 트랜잭션):
1. `Products|Merchants::find().filter(id IN absorb_ids).lock_exclusive()` (SELECT FOR UPDATE).
2. `Transactions::update_many().col_expr({product_id|merchant_id}, canonical_id).filter(... IN absorb_ids)`.
3. `Aliases::delete_many().filter(target_id IN absorb_ids)` — 기존 alias 들은 흡수 row 가 사라지면서 dangling 되므로 삭제. (학습 보존 안 함.)
4. `Products|Merchants::delete_many().filter(id IN absorb_ids)`.
5. commit.

응답:
```json
{ "merged_count": 2, "txn_relinked": 8, "aliases_deleted": 1 }
```

## 6. Frontend

### 6.1 진입점

`/aliases` 페이지의 "클러스터" 탭 단일 통로. 외부 알림 / 배지 / 토스트 추천 없음. 사용자가 정리하고 싶을 때 탭에 들어와서 직접 "다시 계산" 버튼을 누르는 흐름.

### 6.2 클러스터 탭 UI

- 서브탭: Products / Merchants 토글
- 임계치 슬라이더 (0.3 ~ 0.9, step 0.05, 기본 0.5)
- "다시 계산" 버튼 — 슬라이더 변경 시 자동 refetch 가 아니라 명시적 클릭으로 호출
- 결과 영역
  - 0건: "묶을 후보가 없습니다" 빈 상태
  - N건: 카드 그리드 (멤버 수 내림차순)
  - truncated: 상단에 "200개 이상이라 잘렸어요. 임계치를 올려보세요" 안내

### 6.3 클러스터 카드

- 멤버 리스트 (트랜잭션 수 내림차순)
- 각 멤버 1줄: `[O 라디오] [✓ 체크박스] 이름 — 거래 N건, 최근 YYYY-MM-DD`
- 기본 상태: 트랜잭션 수 최대 멤버가 라디오 ON, 나머지는 모두 흡수 체크박스 ON
- 라디오로 선택된 row 의 흡수 체크박스는 disabled (자기 자신 흡수 불가)
- 하단 "병합" 버튼: 흡수 0개면 disabled
- 클릭 시 `POST /clusters/merge` → 카드 fade out → `/clusters` refetch

### 6.4 헬퍼 (`web/lib/cluster-data.ts`)

순수 함수:
- `pickDefaultCanonical(members)` → 트랜잭션 수 최댓값 / 동률시 가나다순 첫 번째
- `formatLatestSeen(date)` → 표시 포맷
- `sortMembersForDisplay(members)` → 트랜잭션 수 내림차순

## 7. 엣지케이스 & 에러처리

| 상황 | 처리 |
|---|---|
| canonical_id 가 absorb_ids 에 포함 | 400 |
| absorb_ids 비어있음 | 400 |
| 다른 owner 의 row 시도 | owner_id 필터로 자연스럽게 404 |
| 병합 도중 다른 import 가 흡수 대상 row 의 transactions 추가 | SELECT FOR UPDATE 잠금. 잠금 후에도 transactions update 는 set-based 라 새로 들어온 행도 포함됨 (병합 기준 시점 이후 import 는 다음 라운드) |
| 같은 raw 가 다음 import 때 또 들어옴 | 학습 효과 없음 → 신규 product/merchant 재생성 → 다음 클러스터 라운드에서 다시 묶음. 사용자 합의된 트레이드오프 |
| pg_trgm extension 미설치 | init 마이그레이션이 `CREATE EXTENSION IF NOT EXISTS` 보장 |
| 결과 200 클러스터 초과 | 응답 truncate + `truncated: true` 플래그 |

## 8. 테스트 계획

### 8.1 Backend (`server/tests/test_clusters.rs` 신규)

1. `clusters_groups_similar_products_above_threshold`
2. `clusters_excludes_singletons`
3. `clusters_respects_owner_isolation`
4. `clusters_threshold_filter_works` — threshold=0.9 에서 비슷한 row 가 안 묶임
5. `merge_relinks_transactions_and_deletes_absorbed`
6. `merge_deletes_aliases_pointing_to_absorbed`
7. `merge_rejects_canonical_in_absorb_ids` (400)
8. `merge_rejects_empty_absorb_ids` (400)
9. `merge_works_for_merchant_scope`

### 8.2 Frontend (`web/__tests__/clusters.test.tsx` 신규)

1. 클러스터 탭 렌더 (서브토글 + 슬라이더 + 다시계산 버튼)
2. 카드 렌더 — 트랜잭션 수 내림차순, 기본 대표 라디오 자동 선택
3. 임계치 슬라이더 변경 후 "다시 계산" 클릭 → refetch
4. 흡수 체크박스 0개 → 병합 버튼 disabled
5. 병합 버튼 클릭 → POST + 카드 사라짐
6. `lib/cluster-data.ts` 헬퍼 단위 테스트

### 8.3 인수 기준

- 골든 엑셀 (`2026년 02월.xlsx`) 재import 후, scope=product / threshold=0.5 에서 의미상 동일한 product 묶음이 최소 1개 이상 후보로 등장.
- 한 클러스터를 병합하면 transactions 전부 canonical 가리킴, 흡수 row 와 그 alias 는 삭제됨.

## 9. 구현 순서 제안

1. DB: init.rs 에 pg_trgm + GIN 인덱스 in-place 추가, fresh + reimport 로 검증
2. Backend: cluster 계산 함수, `GET /clusters`, `POST /clusters/merge`
3. Frontend: cluster-tab + cluster-card + lib helper, /aliases 탭 통합
4. 백엔드 테스트 → 프론트 테스트 → 골든 데이터 인수 확인

## 10. 미정/추후 과제

- categories scope 확장 (사용자 요청 없으면 보류)
- threshold 사용자별 기본값 저장 (현재는 세션 휘발)
- 의미 임베딩 기반 추천 (오버킬, 보류)
