import { z } from "zod";

// ─── Decimal 직렬화 helper ──────────────────────────────────────────────────
// rust_decimal::Decimal는 JSON으로 string 또는 number 양쪽 모두 올 수 있다.
// 수신 후 string으로 정규화한다.
const DecimalSchema = z
  .union([z.string(), z.number()])
  .transform(String);

// ─── 거래 아이템 (재귀) ────────────────────────────────────────────────────
// GET /api/transactions → { items: TransactionItem[], total: number }
// TransactionItem 필드는 server/src/api/transactions.rs:29-50 기준
export type TransactionItem = {
  id: string;
  group_id: string;
  occurred_on: string; // "YYYY-MM-DD"
  merchant_id: string | null;
  merchant_name: string | null;
  actor_id: string | null;
  actor_name: string | null;
  category_id: string | null;
  category_name: string | null;
  product_id: string | null;
  product_name: string | null;
  payment_method_id: string | null;
  payment_method_name: string | null;
  amount: string; // Decimal → string
  sign: number; // i16 (-1 | 1)
  unit_price: string | null;
  quantity: string | null;
  memo: string | null;
  children: TransactionItem[];
};

// ZodType 제네릭: output=TransactionItem, def=ZodTypeDef, input=unknown
// DecimalSchema의 input이 string|number이므로 input을 unknown으로 열어둔다.
export const TransactionItemSchema: z.ZodType<TransactionItem, z.ZodTypeDef, unknown> = z.lazy(() =>
  z.object({
    id: z.string().uuid(),
    group_id: z.string().uuid(),
    occurred_on: z.string(),
    merchant_id: z.string().uuid().nullable(),
    merchant_name: z.string().nullable(),
    actor_id: z.string().uuid().nullable(),
    actor_name: z.string().nullable(),
    category_id: z.string().uuid().nullable(),
    category_name: z.string().nullable(),
    product_id: z.string().uuid().nullable(),
    product_name: z.string().nullable(),
    payment_method_id: z.string().uuid().nullable(),
    payment_method_name: z.string().nullable(),
    amount: DecimalSchema,
    sign: z.number().int(),
    unit_price: DecimalSchema.nullable(),
    quantity: DecimalSchema.nullable(),
    memo: z.string().nullable(),
    // children: 항상 배열 (single-line은 [])
    children: z.array(z.lazy(() => TransactionItemSchema)).default([]),
  }),
);

// GET /api/transactions 응답 루트
export const TransactionsResponseSchema = z.object({
  items: z.array(TransactionItemSchema),
  total: z.number().int(),
});

export type TransactionsResponse = z.infer<typeof TransactionsResponseSchema>;

// ─── 임포트 관련 스키마 ─────────────────────────────────────────────────────
// POST /api/import 응답 — server/src/domain/mod.rs:54-68 기준
export const IntegrityWarningSchema = z.object({
  group_id: z.string().uuid(),
  header_total: DecimalSchema,
  lines_sum: DecimalSchema,
});

export type IntegrityWarning = z.infer<typeof IntegrityWarningSchema>;

// UnresolvedAlias — server/src/domain/mod.rs:71-76 기준
export const UnresolvedAliasSchema = z.object({
  scope: z.string(),
  raw_text: z.string(),
  norm_key: z.string(),
});

export type UnresolvedAlias = z.infer<typeof UnresolvedAliasSchema>;

export const ImportResponseSchema = z.object({
  batch_id: z.string().uuid(),
  year: z.number().int(),
  month: z.number().int(),
  row_count: z.number().int(),
  transactions_inserted: z.number().int(),
  integrity_warnings: z.array(IntegrityWarningSchema),
  // unresolved_aliases는 배열 (number 아님)
  unresolved_aliases: z.array(UnresolvedAliasSchema),
});

export type ImportResponse = z.infer<typeof ImportResponseSchema>;

// ─── 정산 스키마 (M2 대비) ──────────────────────────────────────────────────
export const SettlementSchema = z.object({
  recognized_expense: DecimalSchema,
  deducted_amount: DecimalSchema,
  settlement_input: DecimalSchema,
});

export type Settlement = z.infer<typeof SettlementSchema>;

// ─── Review Queue / Alias schemas (M2 Step C) ──────────────────────────────
// server/src/api/aliases.rs: AliasInfo, ReviewQueueItem, MergeCandidate,
// PostAliasResponse

export const AliasInfoSchema = z.object({
  alias_id: z.string().uuid(),
  raw_text: z.string(),
  norm_key: z.string(),
});

export type AliasInfo = z.infer<typeof AliasInfoSchema>;

export const MergeCandidateSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
});

export type MergeCandidate = z.infer<typeof MergeCandidateSchema>;

export const ReviewQueueItemSchema = z.object({
  scope: z.string(),
  id: z.string().uuid(),
  name: z.string(),
  review_state: z.string(),
  raw_texts: z.array(AliasInfoSchema),
  merge_candidates: z.array(MergeCandidateSchema),
});

export type ReviewQueueItem = z.infer<typeof ReviewQueueItemSchema>;

export const ReviewQueueResponseSchema = z.array(ReviewQueueItemSchema);

export const PostAliasResponseSchema = z.object({
  created: z.boolean(),
  remapped_transaction_count: z.number().int(),
  orphan_deleted: z.boolean(),
});

export type PostAliasResponse = z.infer<typeof PostAliasResponseSchema>;

export const ConfirmEntityResponseSchema = z.object({
  id: z.string().uuid(),
  review_state: z.string(),
});

export type ConfirmEntityResponse = z.infer<typeof ConfirmEntityResponseSchema>;

// ─── 필터 파라미터 ──────────────────────────────────────────────────────────
export const TransactionFilterSchema = z.object({
  from: z.string().optional(),
  to: z.string().optional(),
  category: z.string().optional(),
  actor: z.string().optional(),
  merchant: z.string().optional(),
  payment: z.string().optional(),
  product: z.string().optional(),
  group: z.string().optional(),
});

export type TransactionFilter = z.infer<typeof TransactionFilterSchema>;
