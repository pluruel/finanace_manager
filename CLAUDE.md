# 가계부 통합 뷰어 (finance_mananger)

월별 엑셀(`YYYY년 MM월.xlsx`)을 PostgreSQL에 누적해 카테고리·구매처·상품을 정규화하고 단가 시계열·정산을 보여주는 통합 뷰어. 입력은 계속 엑셀에서 한다.

## 작업 워크플로

코드 변경 작업은 다음 순서로 진행한다.

1. **구현**: 백엔드(`server/`)·프론트엔드(`web/`) 코드를 직접 작성·수정한다.
2. **리뷰**: 변경 후 품질·보안·MSA 계약 위반·도메인 규칙 위반을 스스로 점검한다.
3. **테스트**: 백엔드 `cargo test -p server`, 프론트 `npm test`를 실행해 통과를 확인한다.
4. **문서화**: 구현 상태가 바뀌면 CLAUDE.md를 최신 상태로 업데이트한다.

리뷰·테스트가 통과하기 전에 다음 작업으로 넘어가지 않는다.

---

## 인증 (MSA, 필독)

User 도메인을 구현·수정할 때는 반드시 [`MSA_INTEGRATION.md`](./MSA_INTEGRATION.md)를 먼저 읽고 따른다.

- **인증 서버**: `auth.junodevs.com` (auth-svc)
- **JWKS**: `https://auth.junodevs.com/auth/.well-known/jwks.json`
- **서비스명 (aud)**: `finance-manager`

다운스트림 규칙 (위반 금지):
1. `owner_id uuid`만 저장하고 auth-svc DB로의 **FK 금지**
2. email/이름/그룹 등 사용자 정보 **복제 금지** (필요 시 JWT claim 또는 `/auth/me` 호출)
3. JWT는 **EdDSA**로 검증, `iss=auth-svc` / `aud` 배열에 `finance-manager` 포함 / `exp` 미만료 / `typ=access` 모두 확인
4. refresh 토큰은 **httpOnly + Secure + SameSite 쿠키**에만 저장 (localStorage 금지)

---

## 아키텍처

```
finance_mananger/
  CLAUDE.md
  MSA_INTEGRATION.md
  PLAN.md                    # 초기 구현 계획 (단일 소스)
  docker-compose.yml         # postgres:17 + server + web
  .env.example
  server/                    # Rust(axum) 백엔드
  web/                       # Next.js 15 App Router 프론트
  2026년 02월.xlsx            # M1 임포트 골든 케이스
```

### 백엔드 (`server/`, Rust + axum)
- DB: PostgreSQL 17, `sqlx` 컴파일 타임 쿼리 검증.
- xlsx 읽기: `calamine`
- JWT: `jsonwebtoken` (≥9, EdDSA) + JWKS 5분 메모리 캐시 + 검증 실패 시 1회 강제 갱신
- 자세한 디렉토리/엔드포인트/스키마: [PLAN.md §1·§2](./PLAN.md) 참조

### 프론트엔드 (`web/`, Next.js 15 App Router)
- UI: shadcn/ui + tailwindcss
- 테이블: `@tanstack/react-table` (multi-line 그룹 펼침 지원)
- 차트: `recharts`
- 인증: middleware.ts에서 access 만료 시 `/auth/refresh`, 실패 시 `/login`
- 자세한 라우트: [PLAN.md §4](./PLAN.md) 참조

### M1 구현 현황
- **마이그레이션**: 2개 (`001_init.sql` 스키마 전체 + pgcrypto, `002_fix_settlement_view.sql` 정산 뷰 수정)
- **백엔드 엔드포인트**: `/health` (헬스체크), `POST /api/import` (xlsx 임포트, multipart, 20MB 한도, 멱등성 SHA-256, 단일 트랜잭션), `GET /api/transactions` (필터, 그룹 응답, 재귀 자식)
- **프론트엔드 라우트**: `/login`, `/(app)/` (대시보드), `/transactions` (필터·정렬·그룹 펼침), `/import` (업로드 + 결과 표 + 무결성 경고), `/aliases` (M2 placeholder), `/price-history` (M3 placeholder)
- **테스트**: 백엔드 `cargo test` 34 passed, 프론트 `npm test` 58 passed
- **검증**: 골든 데이터 `2026년 02월.xlsx` 177건 삽입, v_monthly_settlement deducted_amount=7500 일치, 모든 그룹 무결성 검증 0행

---

## 배포 — Docker Compose

서비스는 **docker compose로 배포**한다. 모든 서비스(postgres, server, web)가 compose에서 떠야 하며, 로컬 dev에서도 compose 또는 compose의 postgres 위에서 cargo/pnpm을 돌리는 방식 둘 다 지원한다.

`docker-compose.yml` 구성 (배포용):
- `postgres`: postgres:17, 볼륨 마운트, `DATABASE_URL` 일치
- `server`: server/Dockerfile 빌드, `.env` 주입, postgres 의존
- `web`: web/Dockerfile 빌드, `NEXT_PUBLIC_API_BASE_URL`로 server 가리킴

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

배포·실행 흐름:
1. `docker compose build`
2. `docker compose up -d postgres` 후 `sqlx migrate run` (또는 server 컨테이너의 entrypoint에서 자동 실행)
3. `docker compose up -d server web`
4. 헬스체크: server `/health` 200, web `/` 렌더링

### Python 환경
- `uv` 사용, `.venv`에 가상환경 있음. `uv run` 또는 `.venv/bin/python`으로 실행.

### 마이그레이션 정책
- 마이그레이션 SQL 파일을 누적하지 않는다. 스키마 변경 시 기존 파일을 삭제하고 새로 작성.
- 별도 마이그레이션(ALTER TABLE 등)이 필요한 경우 사용자가 직접 요청한다.

### 테스트 실행 방법
- **백엔드**: `cd server && cargo test -p server` (DATABASE_URL 필요, 임시 테스트 DB 자동 생성)
- **프론트엔드**: `cd web && npm test` (vitest, 58 tests)

---

## 핵심 도메인 규칙 (PLAN에서 발췌)

엑셀 한 행 = 한 거래가 **아니다**. 영수증 1건이 헤더 + 자식 N행으로 분해되는 multi-line 그룹이 존재한다.

### 가계부 내부 사용자 (ledger_actors)
- `ledger_actors` 테이블은 로그인 계정과 무관하며, **지출 대상**을 나타낸다.
  - **공동**: 제3자가 아니라 엉아(배우자)와 본인이 함께한 공동 지출
  - **엉아**: 배우자를 위한 지출
  - **아기**: 아기를 위한 지출
- 월말 정산은 모든 액터의 지출을 파악해 수행한다(공동만 아님).

### 정산 흐름 (매달 반복)
1. 월급을 공동 계좌에 전액 입금
2. 한 달간 모든 지출을 액터별(공동/엉아/아기)로 분류하며 엑셀에 기록
3. 월말에 엑셀 집계 시트에서 액터별 합산 → 공동은 반반, 개인은 각자 부담으로 계산
4. 차액을 한번에 정산 (공동 카테고리 차감 행은 "가계 룰(외식 15,000원까지 인정)" 같은 한도 초과분)
5. 엑셀 집계 시트 산식: "경비인정 - 차감 = 입금액" (이 정산 결과)
- **핵심**: `v_monthly_settlement` 뷰는 "누가 누구를 위해 얼마를 썼는가"를 파악하고, 공동 지출에 대한 공정한 배분을 계산한다.

### 결제수단과 소유자 (payment_methods → actor 매핑)
- 공동 카드는 없다. 모든 결제수단은 **엉아 또는 아기** 소유.
- **아기 소유**: 농협, 신한아기, 롯데, 삼성, 국민, 비씨, 현대, 현금아기
- **엉아 소유**: 현금, 신한, 하나, 씨티클, 현금엉아
- 엑셀 집계 시트(103~110행): G열=엉아 결제수단, J열=아기 결제수단
- `payment_methods` 테이블에 `actor_id` 추가 예정 (M2 마이그레이션)

### 거래 데이터
- 모든 도메인 테이블은 `owner_id uuid NOT NULL`을 갖고 auth-svc로의 FK는 없다.
- 금액은 `numeric(15,2)` 사용. f64 금지.
- Excel serial → DATE: epoch는 **1899-12-30** (1900-02-29 버그 회피).
- 음수 지출은 `sign = -1`로 저장 (별도 테이블로 가르지 않음).
- single-line 그룹 → `transactions` 1행. multi-line 그룹 → 헤더 1행 + 자식 N행 = (1+N)행 저장.
- 카테고리 `"차감"`은 임포트 파이프라인에서 자동 생성(kind='expense', review_state='confirmed' 보호). `sign=+1`로 저장하되 정산 산출에서는 `v_monthly_settlement` 뷰가 분리.

자세한 스키마·엔드포인트·정규화 파이프라인·마일스톤은 [PLAN.md](./PLAN.md)에 있다. **PLAN.md가 단일 소스다** — 충돌 시 PLAN을 따른다.

---

## 마일스톤 요약

- **M1**: 부트스트랩 + 임포트 — ✅ 완료 (2026-04-25). `2026년 02월.xlsx` 177건 삽입, 그룹 합계 무결성 0행, 테스트 통과
- **M2**: 정규화 UI + 월별 대시보드 + 정산 카드 (`v_monthly_settlement`)
- **M3**: 가격 추적 + 구매처 통계 + 다중 월 통합
