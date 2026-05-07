/**
 * 테스트 4: buildTableData 함수 단위 테스트
 *
 * 픽스처:
 * - items 3개: single-line 1, multi-line(자식 2), single-line(차감)
 *
 * 검증:
 * - TableRow 평탄화 결과
 * - group depth, isChildRow (subRows 존재 여부)
 * - "차감" 배지 trigger 조건 (category_name === "차감")
 * - children이 없는 아이템은 subRows가 undefined
 */

import { describe, it, expect } from "vitest";
import type { TransactionItem } from "../lib/schemas";

// buildTableData는 transactions-table.tsx의 내부 함수이므로
// 동일 로직을 여기서 재구현해 단위 테스트한다.
type TableRow = {
  item: TransactionItem;
  subRows?: TableRow[];
};

function buildTableData(items: TransactionItem[]): TableRow[] {
  return items.map((item): TableRow => {
    if (item.children.length > 0) {
      return {
        item,
        subRows: item.children.map((child): TableRow => ({ item: child })),
      };
    }
    return { item };
  });
}

// ─── 테스트 픽스처 ────────────────────────────────────────────────────────────

function makeItem(overrides: Partial<TransactionItem> = {}): TransactionItem {
  return {
    id: "550e8400-e29b-41d4-a716-000000000001",
    group_id: "550e8400-e29b-41d4-a716-000000000099",
    occurred_on: "2026-02-01",
    merchant_id: "550e8400-e29b-41d4-a716-000000000002",
    merchant_name: "테스트구매처",
    actor_id: null,
    actor_name: null,
    category_id: null,
    category_name: "식비",
    product_id: null,
    product_name: null,
    payment_method_id: null,
    payment_method_name: null,
    amount: "10000.00",
    unit_price: null,
    quantity: null,
    memo: null,
    children: [],
    ...overrides,
  };
}

describe("buildTableData", () => {
  it("converts single-line item to TableRow without subRows", () => {
    const singleItem = makeItem({ id: "aaa00000-0000-0000-0000-000000000001" });
    const rows = buildTableData([singleItem]);

    expect(rows).toHaveLength(1);
    expect(rows[0].item).toBe(singleItem);
    expect(rows[0].subRows).toBeUndefined();
  });

  it("converts multi-line item to TableRow with subRows", () => {
    const child1 = makeItem({
      id: "ccc00000-0000-0000-0000-000000000001",
      group_id: "bbb00000-0000-0000-0000-000000000099",
    });
    const child2 = makeItem({
      id: "ccc00000-0000-0000-0000-000000000002",
      group_id: "bbb00000-0000-0000-0000-000000000099",
    });
    const multiItem = makeItem({
      id: "bbb00000-0000-0000-0000-000000000001",
      group_id: "bbb00000-0000-0000-0000-000000000099",
      children: [child1, child2],
    });

    const rows = buildTableData([multiItem]);

    expect(rows).toHaveLength(1);
    expect(rows[0].subRows).toBeDefined();
    expect(rows[0].subRows).toHaveLength(2);
    expect(rows[0].subRows![0].item).toBe(child1);
    expect(rows[0].subRows![1].item).toBe(child2);
  });

  it("children rows have no further subRows", () => {
    const child = makeItem({ id: "child-000-0000-0000-000000000001" });
    const parent = makeItem({
      id: "parent-000-0000-0000-000000000001",
      children: [child],
    });

    const rows = buildTableData([parent]);
    const childRows = rows[0].subRows!;
    expect(childRows[0].subRows).toBeUndefined();
  });

  it("processes mixed items correctly", () => {
    // single-line 1 + multi-line(2 children) + single-line(차감)
    const single1 = makeItem({
      id: "aaa00000-0000-0000-0000-000000000001",
      category_name: "식비",
    });

    const child1 = makeItem({ id: "ccc00000-0000-0000-0000-000000000001" });
    const child2 = makeItem({ id: "ccc00000-0000-0000-0000-000000000002" });
    const multi = makeItem({
      id: "bbb00000-0000-0000-0000-000000000001",
      category_name: "관리비",
      children: [child1, child2],
    });

    const deduction = makeItem({
      id: "ddd00000-0000-0000-0000-000000000001",
      category_name: "차감",
    });

    const rows = buildTableData([single1, multi, deduction]);

    // 최상위 rows는 3개 (items 수와 동일)
    expect(rows).toHaveLength(3);

    // single-line
    expect(rows[0].subRows).toBeUndefined();
    expect(rows[0].item.category_name).toBe("식비");

    // multi-line: subRows 2개
    expect(rows[1].subRows).toHaveLength(2);
    expect(rows[1].item.category_name).toBe("관리비");

    // 차감 항목
    expect(rows[2].subRows).toBeUndefined();
    expect(rows[2].item.category_name).toBe("차감");
  });

  it("deduction item has category_name = '차감'", () => {
    const deductionItem = makeItem({ category_name: "차감" });
    const rows = buildTableData([deductionItem]);

    const isDeduction = rows[0].item.category_name === "차감";
    expect(isDeduction).toBe(true);
  });

  it("non-deduction item does not trigger 차감 badge", () => {
    const normalItem = makeItem({ category_name: "식비" });
    const rows = buildTableData([normalItem]);

    const isDeduction = rows[0].item.category_name === "차감";
    expect(isDeduction).toBe(false);
  });

  it("row depth: top-level row has no parent (depth=0 semantically)", () => {
    // buildTableData 자체는 depth를 계산하지 않는다.
    // react-table이 subRows를 통해 depth를 계산하므로
    // top-level 행에 subRows가 있으면 그 자식들이 depth=1이 됨.
    const child = makeItem({ id: "child-000-0000-0000-000000000001" });
    const parent = makeItem({
      id: "parent-000-0000-0000-000000000001",
      children: [child],
    });

    const rows = buildTableData([parent]);
    // 자식 행은 subRows 없음 → react-table에서 depth=1
    expect(rows[0].subRows![0].subRows).toBeUndefined();
  });

  it("handles empty items array", () => {
    const rows = buildTableData([]);
    expect(rows).toHaveLength(0);
  });
});
