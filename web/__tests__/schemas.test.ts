/**
 * 테스트 1: zod 스키마 라운드트립
 *
 * 백엔드 응답 샘플 JSON을 파싱해 모든 스키마가 통과하는지 검증한다.
 * - Decimal string/number 양쪽 입력 처리
 * - recursive children 1단계
 * - 각 스키마의 optional/nullable 필드 처리
 */

import { describe, it, expect } from "vitest";
import {
  TransactionItemSchema,
  TransactionsResponseSchema,
  ImportResponseSchema,
  IntegrityWarningSchema,
  UnresolvedAliasSchema,
  SettlementSchema,
  TransactionFilterSchema,
} from "../lib/schemas";

// ─── 테스트 픽스처 ────────────────────────────────────────────────────────────

const sampleTransactionItem = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  group_id: "550e8400-e29b-41d4-a716-446655440001",
  occurred_on: "2026-02-01",
  merchant_id: "550e8400-e29b-41d4-a716-446655440002",
  merchant_name: "우렁쌈밥",
  actor_id: "550e8400-e29b-41d4-a716-446655440003",
  actor_name: "공동",
  category_id: "550e8400-e29b-41d4-a716-446655440004",
  category_name: "배달",
  product_id: "550e8400-e29b-41d4-a716-446655440005",
  product_name: "제육볶음+우렁쌈밥",
  payment_method_id: "550e8400-e29b-41d4-a716-446655440006",
  payment_method_name: "농협",
  amount: "24900.00",
  sign: 1,
  unit_price: "24900.0000",
  quantity: "1.0000",
  memo: "제육볶음+우렁쌈밥",
  children: [],
};

const sampleMultiLineItem = {
  id: "660e8400-e29b-41d4-a716-446655440000",
  group_id: "660e8400-e29b-41d4-a716-446655440001",
  occurred_on: "2026-02-24",
  merchant_id: "660e8400-e29b-41d4-a716-446655440002",
  merchant_name: "풍림아이원",
  actor_id: null,
  actor_name: null,
  category_id: "660e8400-e29b-41d4-a716-446655440004",
  category_name: "전기",
  product_id: null,
  product_name: null,
  payment_method_id: null,
  payment_method_name: null,
  amount: "44760.00",
  sign: 1,
  unit_price: "44760.0000",
  quantity: "1.0000",
  memo: "전기 278Kw",
  children: [
    {
      id: "660e8400-e29b-41d4-a716-446655440010",
      group_id: "660e8400-e29b-41d4-a716-446655440001",
      occurred_on: "2026-02-24",
      merchant_id: "660e8400-e29b-41d4-a716-446655440020",
      merchant_name: "(주)대호안전관리공",
      actor_id: null,
      actor_name: null,
      category_id: null,
      category_name: null,
      product_id: "660e8400-e29b-41d4-a716-446655440030",
      product_name: "난방 0.88 mw",
      payment_method_id: null,
      payment_method_name: null,
      amount: "93500.00",
      sign: 1,
      unit_price: "93500.0000",
      quantity: "1.0000",
      memo: "난방 0.88 mw",
      children: [],
    },
  ],
};

const sampleDeductionItem = {
  id: "770e8400-e29b-41d4-a716-446655440000",
  group_id: "770e8400-e29b-41d4-a716-446655440001",
  occurred_on: "2026-02-02",
  merchant_id: "770e8400-e29b-41d4-a716-446655440002",
  merchant_name: "화육면",
  actor_id: "770e8400-e29b-41d4-a716-446655440003",
  actor_name: "엉아",
  category_id: "770e8400-e29b-41d4-a716-446655440004",
  category_name: "차감",
  product_id: null,
  product_name: null,
  payment_method_id: null,
  payment_method_name: null,
  amount: "4000.00",
  sign: 1,
  unit_price: null,
  quantity: null,
  memo: null,
  children: [],
};

// ─── TransactionItemSchema ────────────────────────────────────────────────────

describe("TransactionItemSchema", () => {
  it("parses a valid single-line item", () => {
    const result = TransactionItemSchema.safeParse(sampleTransactionItem);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.id).toBe(sampleTransactionItem.id);
      expect(result.data.amount).toBe("24900.00"); // string 정규화
      expect(result.data.children).toEqual([]);
    }
  });

  it("parses Decimal as number input and converts to string", () => {
    const withNumberAmount = { ...sampleTransactionItem, amount: 24900.0 };
    const result = TransactionItemSchema.safeParse(withNumberAmount);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(typeof result.data.amount).toBe("string");
    }
  });

  it("parses Decimal as string input", () => {
    const withStringAmount = { ...sampleTransactionItem, amount: "24900.00" };
    const result = TransactionItemSchema.safeParse(withStringAmount);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.amount).toBe("24900.00");
    }
  });

  it("handles nullable fields correctly", () => {
    const result = TransactionItemSchema.safeParse(sampleDeductionItem);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.product_id).toBeNull();
      expect(result.data.memo).toBeNull();
      expect(result.data.unit_price).toBeNull();
      expect(result.data.quantity).toBeNull();
    }
  });

  it("parses multi-line item with 1-level children", () => {
    const result = TransactionItemSchema.safeParse(sampleMultiLineItem);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.children).toHaveLength(1);
      expect(result.data.children[0].product_name).toBe("난방 0.88 mw");
    }
  });

  it("provides default empty array when children is missing", () => {
    const withoutChildren = { ...sampleTransactionItem };
    // @ts-ignore - 테스트 목적으로 children 제거
    delete withoutChildren.children;

    const result = TransactionItemSchema.safeParse(withoutChildren);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.children).toEqual([]);
    }
  });

  it("rejects invalid UUID", () => {
    const invalid = { ...sampleTransactionItem, id: "not-a-uuid" };
    const result = TransactionItemSchema.safeParse(invalid);
    expect(result.success).toBe(false);
  });
});

// ─── TransactionsResponseSchema ───────────────────────────────────────────────

describe("TransactionsResponseSchema", () => {
  it("parses a valid transactions response", () => {
    const response = {
      items: [sampleTransactionItem, sampleDeductionItem],
      total: 2,
    };
    const result = TransactionsResponseSchema.safeParse(response);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.total).toBe(2);
      expect(result.data.items).toHaveLength(2);
    }
  });

  it("parses response with multi-line items", () => {
    const response = {
      items: [sampleMultiLineItem],
      total: 1,
    };
    const result = TransactionsResponseSchema.safeParse(response);
    expect(result.success).toBe(true);
  });

  it("parses empty items array", () => {
    const response = { items: [], total: 0 };
    const result = TransactionsResponseSchema.safeParse(response);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.total).toBe(0);
    }
  });
});

// ─── ImportResponseSchema ─────────────────────────────────────────────────────

describe("ImportResponseSchema", () => {
  it("parses a valid import response", () => {
    const importResponse = {
      batch_id: "550e8400-e29b-41d4-a716-446655440099",
      year: 2026,
      month: 2,
      row_count: 177,
      transactions_inserted: 177,
      integrity_warnings: [],
      unresolved_aliases: [
        { scope: "merchant", raw_text: "우렁쌈밥", norm_key: "우렁쌈밥" },
      ],
    };
    const result = ImportResponseSchema.safeParse(importResponse);
    expect(result.success).toBe(true);
  });

  it("parses import response with integrity warnings", () => {
    const withWarnings = {
      batch_id: "550e8400-e29b-41d4-a716-446655440099",
      year: 2026,
      month: 2,
      row_count: 100,
      transactions_inserted: 100,
      integrity_warnings: [
        {
          group_id: "550e8400-e29b-41d4-a716-446655440088",
          header_total: "10000.00",
          lines_sum: "9500.00",
        },
      ],
      unresolved_aliases: [],
    };
    const result = ImportResponseSchema.safeParse(withWarnings);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.integrity_warnings).toHaveLength(1);
    }
  });
});

// ─── IntegrityWarningSchema ───────────────────────────────────────────────────

describe("IntegrityWarningSchema", () => {
  it("parses with Decimal as string", () => {
    const result = IntegrityWarningSchema.safeParse({
      group_id: "550e8400-e29b-41d4-a716-446655440088",
      header_total: "10000.00",
      lines_sum: "9500.00",
    });
    expect(result.success).toBe(true);
  });

  it("parses with Decimal as number", () => {
    const result = IntegrityWarningSchema.safeParse({
      group_id: "550e8400-e29b-41d4-a716-446655440088",
      header_total: 10000,
      lines_sum: 9500,
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(typeof result.data.header_total).toBe("string");
    }
  });
});

// ─── SettlementSchema ─────────────────────────────────────────────────────────

describe("SettlementSchema", () => {
  it("parses settlement data", () => {
    const result = SettlementSchema.safeParse({
      year: 2026,
      month: 2,
      recognized_expense: "584000.00",
      deducted_amount: "7500.00",
      settlement_input: "576500.00",
    });
    expect(result.success).toBe(true);
  });

  it("parses settlement with number inputs", () => {
    const result = SettlementSchema.safeParse({
      year: 2026,
      month: 2,
      recognized_expense: 584000,
      deducted_amount: 7500,
      settlement_input: 576500,
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(typeof result.data.deducted_amount).toBe("string");
    }
  });
});

// ─── TransactionFilterSchema ──────────────────────────────────────────────────

describe("TransactionFilterSchema", () => {
  it("parses empty filter", () => {
    const result = TransactionFilterSchema.safeParse({});
    expect(result.success).toBe(true);
  });

  it("parses partial filter", () => {
    const result = TransactionFilterSchema.safeParse({
      from: "2026-02-01",
      to: "2026-02-28",
      category: "배달",
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.from).toBe("2026-02-01");
    }
  });

  it("parses full filter", () => {
    const result = TransactionFilterSchema.safeParse({
      from: "2026-02-01",
      to: "2026-02-28",
      category: "배달",
      actor: "공동",
      merchant: "이마트",
      payment: "농협",
      product: "제육볶음",
      group: "550e8400-e29b-41d4-a716-446655440001",
    });
    expect(result.success).toBe(true);
  });
});
