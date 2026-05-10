# SeaORM 전환 설계 (sqlx → SeaORM 원샷 이관)

- 작성일: 2026-05-10
- 대상 범위: `server/` (Rust + axum) 백엔드 전체
- 결정: sqlx 단독 사용을 폐기하고 **SeaORM** 으로 완전 이관한다. 한 PR 안에서 끝낸다.

## 0. 배경 / 문제

현재 백엔드는 sqlx 0.7 의 컴파일타임 체크 쿼리를 직접 작성하는 구조다. 87개 테스트가 그린이고 동작은 안정적이지만, 다음 통증점이 누적되었다:

1. **JOIN/관계 매핑 보일러플레이트** — `aliases.rs` (961줄) 같은 큰 모듈에서 여러 테이블 조인 시 SELECT를 손으로 쓰고 row→struct 매핑 반복.
2. **스키마-구조체 이중 관리** — `001_init.sql` 과 Rust struct를 양쪽에서 따로 유지. 컬럼 추가 시 두 곳 수정.
3. **동적 쿼리 조립** — 조건부 WHERE/정렬/페이지네이션을 `query!` 로 표현 못 해 문자열 조립이나 분기 코드.
4. **트랜잭션/관계 저장** — import 파이프라인의 부모-자식 묶음 INSERT, `FOR UPDATE` 락, upsert가 손코딩이라 위험.
5. **`.sqlx/` 오프라인 캐시 관리 비용** — 쿼리 변경 때마다 `cargo sqlx prepare` 챙겨야 하고 CI에서 `SQLX_OFFLINE=true` 환경에 의존.

## 1. 채택 기술 / 비채택 사유

| 후보 | 결정 | 이유 |
|---|---|---|
| **SeaORM 1.x** | ✅ 채택 | async-native, entity 기반, `sea-query` 동적 쿼리, `sea-orm-migration` 내장, sqlx 위에서 동작해 마이그레이션 위험 최소, axum 친화 |
| Diesel + diesel-async | ❌ | `schema.rs` codegen 방식이 단일 마이그레이션 in-place 정책과 맞지 않음. JSON/Decimal 처리 수동 비용 |
| sqlx + sea-query | ❌ | 동적 쿼리만 해결. `.sqlx/` 캐시·이중 관리 통증점이 그대로 |

## 2. 아키텍처

### 2.1 크레이트 구성

```
server/
  Cargo.toml             # sqlx 제거, sea-orm + sea-orm-migration 추가
  migration/             # 신규 sub-crate (sea-orm-migration 표준 레이아웃)
    Cargo.toml
    src/lib.rs
    src/m20260510_000001_init.rs   # 단일 마이그레이션 (rewrite-in-place)
  src/
    entity/              # sea-orm-cli 자동 생성, 체크인
      mod.rs
      ledger_actor.rs
      payment_method.rs
      category.rs
      alias.rs
      transaction.rs
      product.rs
      ...
    db.rs                # DatabaseConnection 보유 + 부팅시 Migrator::up
    api/...              # ORM-first 로 재작성
    import/pipeline.rs   # 트랜잭션 패턴은 SeaORM API 로 매핑
```

`migration/` 은 워크스페이스 멤버. `server` 가 `migration` 을 path 의존성으로 참조해 부팅·테스트에서 `Migrator::up` / `Migrator::fresh` 호출.

### 2.2 의존성 (`server/Cargo.toml` 변화 요약)

추가:
```toml
sea-orm = { version = "1", features = [
  "runtime-tokio-rustls",
  "sqlx-postgres",
  "with-uuid",
  "with-chrono",
  "with-rust_decimal",
  "with-json",
  "macros",
  "debug-print",
] }
sea-orm-migration = { version = "1", features = [
  "runtime-tokio-rustls",
  "sqlx-postgres",
] }
```

제거:
- `sqlx = { version = "0.7", features = [...] }` 직접 의존 (transitive 로는 SeaORM 이 들어감)
- `.sqlx/` 디렉토리, `sqlx.sh` 스크립트
- CI/문서의 `SQLX_OFFLINE=true` 환경변수 의존

### 2.3 런타임 상태

- 기존 `AppState { pool: PgPool, ... }` → `AppState { db: DatabaseConnection, ... }`
- `main.rs`, `bin/test_import.rs` 부트 시퀀스 동일 (커넥션 문자열 형식만 SeaORM 식)
- 부팅 시 `migration::Migrator::up(&db, None).await?` 자동 적용

## 3. 마이그레이션 정책

### 3.1 단일 파일 in-place 편집

기존 CLAUDE.md 의 "Do not let migration SQL files accumulate. When the schema changes, delete the existing file and rewrite it" 정책을 SeaORM 으로 그대로 옮긴다.

- 마이그레이션 파일은 `m20260510_000001_init.rs` 하나만 유지.
- 스키마 변경이 필요하면 이 파일을 직접 수정한 뒤 dev DB 에서 `Migrator::fresh` 로 재적용. 별도 incremental 마이그레이션은 사용자가 명시 요청한 경우에만 추가.

### 3.2 마이그레이션 본문 구조

```rust
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // 1) 테이블/PK/FK/UNIQUE/INDEX/CHECK : SchemaManager DSL
        m.create_table(Table::create()
            .table(Transaction::Table)
            .col(ColumnDef::new(Transaction::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(Transaction::OwnerId).uuid().not_null())
            .col(ColumnDef::new(Transaction::Amount).decimal_len(15, 2).not_null())
            .col(ColumnDef::new(Transaction::OccurredOn).date().not_null())
            .foreign_key(...)
            .to_owned()).await?;
        // ... 나머지 테이블

        // 2) 뷰 / 트리거 / 부분인덱스 / 함수 : raw SQL
        m.get_connection().execute_unprepared(r#"
            CREATE VIEW v_monthly_settlement AS
            SELECT ...
        "#).await?;

        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.get_connection().execute_unprepared(
            "DROP VIEW IF EXISTS v_monthly_settlement"
        ).await?;
        m.drop_table(Table::drop().table(Transaction::Table).to_owned()).await?;
        // ...
        Ok(())
    }
}
```

### 3.3 DSL vs raw SQL 경계

- **DSL**: 테이블, PK, FK, UNIQUE, 일반 INDEX, CHECK
- **raw SQL (`execute_unprepared`)**: VIEW (`v_monthly_settlement`), 부분 인덱스, 트리거, 함수, 복잡한 GIN/GiST. "복잡하면 raw SQL" 이 아니라 "DSL 이 깔끔히 표현 못 하면 raw SQL". 가독성 우선.

### 3.4 엔티티 생성

1. 마이그레이션을 빈 DB 에 적용
2. `sea-orm-cli generate entity -o server/src/entity --with-serde both` 실행
3. 결과를 그대로 체크인. `entity/` 는 자동생성물이므로 손으로 수정하지 않는다.
4. 자동생성기가 놓치는 관계 (복합 FK, 다대다 중간 테이블 등) 만 `Relation` 정의를 손으로 보강한다. 컬럼명·타입을 바꾸고 싶으면 마이그레이션 컬럼명을 손본 뒤 재생성.

## 4. 런타임 쿼리 패턴

### 4.1 단순 CRUD / 조건부 조회

```rust
let mut q = Category::find().filter(category::Column::OwnerId.eq(owner));
if let Some(k) = kind {
    q = q.filter(category::Column::Kind.eq(k));
}
let rows = q.order_by_asc(category::Column::Name).all(&db).await?;
```

`Cond::any()/all()` 로 OR/AND 트리도 표현. → 통증점 #3 해소.

### 4.2 JOIN / 관계 매핑

```rust
// N:1 동시 로드
let rows: Vec<(transaction::Model, Option<category::Model>)> =
    Transaction::find()
        .find_also_related(Category)
        .filter(transaction::Column::OwnerId.eq(owner))
        .all(&db).await?;

// 1:N (header → children)
let groups: Vec<(transaction::Model, Vec<transaction_line::Model>)> =
    Transaction::find()
        .find_with_related(TransactionLine)
        .all(&db).await?;
```

관계는 `entity/transaction.rs::Relation` 에 한 번 선언하면 끝. → 통증점 #1 해소.

### 4.3 동적 / 다중 조인 / 페이징

`/api/transactions?from=&to=&actor=&q=` 같은 복합 쿼리는 `QuerySelect::join(JoinType::LeftJoin, ...)` + `Condition::all()` 누적 + `.paginate(&db, page_size)`.

### 4.4 복잡 집계 / 뷰 — raw SQL + `FromQueryResult`

ORM 으로 무리해서 표현하지 않는다. raw SQL 영역 후보:

- `v_monthly_settlement` 조회 — `api/settlement.rs`
- 카테고리×액터 피벗 — `api/summary.rs`
- 머천트/상품 집계 — `api/merchant_stats.rs`, `api/price.rs`, `api/products.rs`
- xlsx export 의 대용량 SELECT — `api/export.rs`
- LIKE/유사도 검색

```rust
#[derive(FromQueryResult, Serialize)]
struct SettlementRow { actor_id: Uuid, joint: Decimal, personal: Decimal /* ... */ }

let rows = SettlementRow::find_by_statement(Statement::from_sql_and_values(
    DbBackend::Postgres,
    "SELECT * FROM v_monthly_settlement WHERE owner_id = $1 AND ym = $2",
    [owner.into(), ym.into()],
)).all(&db).await?;
```

→ `.sqlx/` 캐시 제거 + 결과 매핑 보일러플레이트 제거. 통증점 #5 해소.

### 4.5 트랜잭션 / 락 / upsert

```rust
let txn = db.begin().await?;

// SELECT FOR UPDATE (alias merge race-safe 패턴)
let alias = Alias::find_by_id(id)
    .lock_exclusive()
    .one(&txn).await?
    .ok_or(NotFound)?;

// upsert (ON CONFLICT DO NOTHING)
Category::insert(model).on_conflict(
    OnConflict::columns([category::Column::OwnerId, category::Column::NormKey])
        .do_nothing().to_owned()
).exec(&txn).await?;

txn.commit().await?;
```

`&txn` 을 다음 호출로 전파하면 동일 트랜잭션. → 통증점 #4 해소.

### 4.6 import 파이프라인 (가장 위험한 영역, 630줄)

- 그룹별 header + N children INSERT → `Transaction::insert_many()`, `RETURNING id` 는 `exec_with_returning`.
- ON CONFLICT 분기 (사용자 토글 보존용 DO NOTHING) → `OnConflict::columns(...).do_nothing()` 1:1 매핑.
- 차감/보험금 sign-split 같은 도메인 로직은 그대로 유지. DB 인터페이스만 교체.

### 4.7 통증점 매핑 요약

| 통증점 | 해소 수단 |
|---|---|
| JOIN/관계 매핑 보일러플레이트 | `find_*_related`, `Relation` 한 번 선언 |
| 스키마-구조체 이중 관리 | 마이그레이션이 단일 출처, `entity/` 자동생성 |
| 동적 쿼리 조립 | `Condition::all/any`, `QuerySelect` 누적 |
| 트랜잭션/관계 저장 | `db.begin()`, `&txn` 전파, `lock_exclusive`, `OnConflict` |
| `.sqlx/` 캐시 관리 | 컴파일타임 DB 연결 불필요 → 캐시 자체 폐기 |

## 5. 테스트 전략

### 5.1 인프라

- 기존 ephemeral DB 패턴 (테스트마다 임시 DB 생성 → 마이그레이션 → drop) 그대로.
- 변경점은 적용 함수 한 줄: `sqlx::migrate!("./migrations").run(&pool)` → `migration::Migrator::up(&db, None)`.
- `tests/common/mod.rs` 헬퍼가 `&PgPool` 대신 `&DatabaseConnection` 을 반환.

### 5.2 검증 게이트 (모두 통과해야 머지 가능)

1. `cargo build -p server` 클린.
2. `cargo test -p server` **87/87 그린** — 행동 동등성. 테스트 수가 줄거나 늘면 안 됨.
3. `cd web && npm test` **124/124 그린** — 백엔드 응답 스키마 동일성.
4. **데이터 동등성 스모크**: 깨끗한 DB 에 `2026년 02월.xlsx` import 후
   - `transactions` 행 수 = 177
   - 그룹합 무결성 = 0 위반
   - "고덕방 아이스아메리카노" 6 행 ₩3,400
   - `v_monthly_settlement` 결과가 sqlx 시절의 사전 dump 와 byte-equal
5. 부팅 후 `/health` 200, 대시보드 `?ym=2026-02` 정상 렌더 (수동 1회).

## 6. 작업 순서 (원샷 PR 내부 커밋 분할)

bisect 가능한 단위로 커밋을 쪼갠다.

1. **C1 — 의존성/스캐폴딩**: `Cargo.toml` 정리 (`sqlx` 제거, `sea-orm`/`sea-orm-migration` 추가), `migration/` sub-crate 생성, 워크스페이스 등록. 컴파일 깨짐 허용.
2. **C2 — 마이그레이션 이식**: `001_init.sql` 모든 DDL 을 `m20260510_000001_init::up()` 으로 변환. 뷰는 `execute_unprepared`. `down()` 작성. `001_init.sql` 삭제.
3. **C3 — 엔티티 생성**: 빈 DB 에 적용 → `sea-orm-cli generate entity -o server/src/entity --with-serde both` → 결과 체크인. 누락된 `Relation` 보강.
4. **C4 — 인프라 교체**: `db.rs`, `AppState`, `main.rs`, `bin/test_import.rs`, 테스트 헬퍼를 `DatabaseConnection` 기반으로.
5. **C5 — API 모듈 이관 (CRUD)**: `categories.rs`, `aliases.rs`, `products.rs`, `transactions.rs`, `import.rs` 라우터 — ORM-first.
6. **C6 — API 모듈 이관 (집계/뷰)**: `settlement.rs`, `summary.rs`, `income.rs`, `merchant_stats.rs`, `price.rs`, `export.rs` — raw SQL + `FromQueryResult`.
7. **C7 — import 파이프라인 이관**: `import/pipeline.rs`. 트랜잭션·`lock_exclusive`·`OnConflict` 패턴 매핑. 골든 데이터 재 import 검증.
8. **C8 — 잔여물 정리**: `.sqlx/` 삭제, `sqlx.sh` 삭제, `sqlx-cli` 관련 문서/명령 제거, CLAUDE.md 갱신 (마이그레이션 정책 SeaORM 기준 표현, 새 테스트 수, 누적 컨텍스트 한 줄).

## 7. 위험 / 완화

- **엔티티 자동생성 누락**: `sea-orm-cli` 가 일부 FK·복합 unique 를 빠뜨릴 수 있음 → C3 직후 수동 diff 검토, 누락 시 `Relation` 보강.
- **Decimal 직렬화 차이**: `rust_decimal` serde 형식 (`serde-with-str` vs SeaORM 기본) 이 응답 JSON 에 영향 줄 수 있음 → 5.2 의 frontend 124 테스트가 가드. 차이 발생 시 entity 의 `#[serde(with = "rust_decimal::serde::str")]` 어노테이션을 수동 보강.
- **import 트랜잭션 의미 변화**: `&DatabaseTransaction` 전파 시 race-safe alias 재조회가 정확히 보존됐는지 5.2 의 import 스모크 + 기존 alias merge 통합 테스트로 확인.
- **CI 환경**: 기존 CI 가 `SQLX_OFFLINE=true` 를 가정하면 환경변수 제거 + workflow 수정 (C8).

## 8. 비범위 (이번 PR에서 안 함)

- 신규 기능 추가, API 시그니처 변경, 응답 스키마 변경 — 본 PR 은 **순수 인프라 이관**. 동일한 입력에 동일한 출력을 보장한다.
- 신규 테스트 추가. 기존 테스트만 그대로 통과시킨다 (예외: 5.2(4) 의 데이터 동등성 스모크는 1회성 검증이며 영구 테스트로 추가하지 않는다).
- 추가 Postgres 확장(GIN/trgm 등) 도입.

## 9. 수용 기준

- `cargo test -p server` 87/87 그린, `npm test` 124/124 그린.
- `001_init.sql` 와 `.sqlx/` 가 저장소에서 삭제되어 있다.
- `Cargo.toml` 의 `[dependencies]` 에 `sqlx` 직접 의존이 없다.
- `2026년 02월.xlsx` 재 import 후 행 수·합계·뷰 결과가 사전 스냅샷과 일치.
- CLAUDE.md 의 "Architecture / Backend" 와 "Migration Policy" 섹션이 SeaORM 기준으로 갱신되어 있다.
