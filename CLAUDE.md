# 가계부 통합 뷰어 (finance_mananger)

월별 엑셀(`YYYY년 MM월.xlsx`)을 PostgreSQL에 누적해 카테고리·구매처·상품을 정규화하고 단가 시계열·정산을 보여주는 통합 뷰어. 입력은 계속 엑셀에서 한다.

## 메인 에이전트 운영 규칙 (반드시 준수)

> **메인 에이전트는 코드를 직접 작성·수정하지 않는다.** 코드 변경 또는 에이전트가 수행할 작업이 요청되면 메인은 **(1) 적절한 서브에이전트에게 작업을 전달하고**, **(2) 결과가 올바르게 반영되었는지 확인**하는 역할만 수행한다.

### 위임 규칙
- 백엔드(Rust/server) 작업 → `backend` 서브에이전트
- 프론트엔드(Next.js/web) 작업 → `frontend` 서브에이전트
- 모든 코드 변경 직후 → `reviewer` 서브에이전트 (Opus, 코드 품질·보안·계약 위반 검토)
- 리뷰 통과 후 → `tester` 서브에이전트 (테스트 코드 작성 + 실행 + 통과 확인)
- 구현 상황 변경 시 → `documentation` 서브에이전트 (CLAUDE.md를 현재 구현에 맞게 업데이트)

### 워크플로 (모든 작업에 적용)
1. **위임**: 작업 분류에 맞는 서브에이전트 호출 (backend / frontend)
2. **리뷰**: 작업이 끝나면 즉시 `reviewer`에게 변경 사항 검토 요청
3. **테스트**: 리뷰 후 `tester`가 테스트 코드 작성 및 실행, 통과 확인
4. **문서화**: 구현 상태가 바뀌었으면 `documentation`에게 CLAUDE.md 업데이트 요청
5. **검증**: 메인은 각 단계의 산출물이 실제 파일/실행에 반영됐는지 확인 (직접 코드 작성·수정 금지)

이 순서를 건너뛰지 않는다. 리뷰·테스트가 통과하기 전에 다음 작업으로 넘어가지 않는다.

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

**BFF 인증 라우트 계약** (`web/app/api/auth/`):
- `/api/auth/login` (POST): `username`(email) + `password` JSON → form-urlencoded 변환 후 auth-svc 호출. access 쿠키(`path=/`, expires_in 또는 15분), refresh 쿠키(`path=/api/auth`, 14일) 설정. 클라이언트에는 `{ok:true}`만.
- `/api/auth/refresh` (POST): 우리 refresh 쿠키(`Cookie` 헤더로 `refresh_token=<v>`)로 auth-svc `/auth/refresh` 호출. **TokenPair 응답의 새 refresh_token을 우리 쿠키에 반드시 갱신**(rotation). 401 시 access·refresh 쿠키(`path=/` + path=/api/auth`) 모두 Max-Age=0.
- `/api/auth/logout` (POST): refresh 쿠키를 Cookie 헤더로 auth-svc 호출(2s 타임아웃, 실패 무시). access·refresh 쿠키(두 path) 삭제.
- middleware: self-fetch 없이 `performRefresh(refreshToken)`을 직접 호출. 401/실패 시 `/?from=<path>` 리다이렉트 + 쿠키 정리.
- 공유 헬퍼(`web/lib/auth-cookies.ts`): `parseSetCookieNameValue`, `accessCookieOptions`(path=/), `refreshCookieOptions`(path=/api/auth), `appendLegacyRefreshDelete`(과도기 path=/ 정리).
- `web/lib/perform-refresh.ts`: `performRefresh` 함수 + `RefreshResult` 타입. middleware와 refresh route 핸들러 모두 이 파일에서 import한다(Next.js는 route 파일의 비-HTTP 메서드 named export를 금지하므로 분리).

---

## 아키텍처

```
finance_mananger/
  CLAUDE.md
  MSA_INTEGRATION.md
  PLAN.md                    # 초기 구현 계획 (단일 소스)
  docker-compose.yml         # postgres:17 + server + web
  .env.example
  server/                    # Rust(axum) 백엔드 — backend 에이전트 담당
  web/                       # Next.js 15 App Router 프론트 — frontend 에이전트 담당
  2026년 02월.xlsx            # M1 임포트 골든 케이스
  .claude/agents/            # 서브에이전트 정의
```

### 백엔드 (`server/`, Rust + axum)
- DB: PostgreSQL 17, `sqlx` 컴파일 타임 쿼리 검증
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
- **프론트엔드 라우트**: `/login`, `/(app)/` (대시보드), `/transactions` (필터·정렬·그룹 펼침), `/import` (업로드 + 결과 표 + 무결성 경고), `/api/auth/login|refresh|logout` (BFF 인증, TokenPair rotation, cookie transport), `/aliases` (M2 placeholder), `/price-history` (M3 placeholder). middleware는 performRefresh 직접 호출, 401 시 `/login?from=<path>` 리다이렉트.
- **테스트**: 백엔드 `cargo test` 34 passed, 프론트 `npm test` 101 passed
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

### 테스트 실행 방법
- **백엔드**: `cd server && cargo test -p server` (DATABASE_URL 필요, 임시 테스트 DB 자동 생성)
- **프론트엔드**: `cd web && npm test` (vitest, 101 tests)

---

## 핵심 도메인 규칙 (PLAN에서 발췌)

엑셀 한 행 = 한 거래가 **아니다**. 영수증 1건이 헤더 + 자식 N행으로 분해되는 multi-line 그룹이 존재한다.

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

---

## 서브에이전트 일람

| 이름 | 모델 | 역할 |
| --- | --- | --- |
| `backend` | sonnet | Rust/axum 서버 코드 작성·수정 |
| `frontend` | sonnet | Next.js/web 코드 작성·수정 |
| `reviewer` | opus | 변경된 코드의 품질·보안·MSA 계약 위반 리뷰 |
| `tester` | sonnet | 테스트 코드 작성 + 실행 + 통과 검증 |
| `documentation` | haiku | 현재 구현 상태에 맞춰 CLAUDE.md 업데이트 |

각 정의는 `.claude/agents/<name>.md`에 있다.
