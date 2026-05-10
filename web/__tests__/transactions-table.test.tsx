/**
 * Transactions table — expansion / collapse repro.
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

import { TransactionsTable } from "../components/transactions-table";
import type { TransactionItem } from "../lib/schemas";

vi.mock("next/navigation", () => ({
  useRouter: () => ({
    push: vi.fn(),
    refresh: vi.fn(),
    replace: vi.fn(),
  }),
}));

function makeItem(overrides: Partial<TransactionItem> = {}): TransactionItem {
  return {
    id: crypto.randomUUID(),
    group_id: crypto.randomUUID(),
    occurred_on: "2026-02-01",
    merchant_id: null,
    merchant_name: "테스트",
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

describe("TransactionsTable expansion", () => {
  it("expands a multi-line group when the chevron is clicked", () => {
    const groupId = crypto.randomUUID();
    const child1 = makeItem({ group_id: groupId, memo: "child-A" });
    const child2 = makeItem({ group_id: groupId, memo: "child-B" });
    const parent = makeItem({
      group_id: groupId,
      memo: "parent",
      children: [child1, child2],
    });
    const single = makeItem({ memo: "single-row" });

    render(
      <TransactionsTable
        items={[parent, single]}
        total={2}
        searchParams={{}}
      />,
    );

    expect(screen.queryByText("child-A")).toBeNull();
    fireEvent.click(screen.getByLabelText("펼치기"));
    expect(screen.getByText("child-A")).toBeTruthy();
    expect(screen.getByText("child-B")).toBeTruthy();
  });

  it("expands a 17-child group quickly without rerender storm", () => {
    const groupId = crypto.randomUUID();
    const children = Array.from({ length: 17 }, (_, i) =>
      makeItem({ group_id: groupId, memo: `c${i}` }),
    );
    const parent = makeItem({ group_id: groupId, memo: "P", children });

    render(
      <TransactionsTable
        items={[parent]}
        total={1}
        searchParams={{}}
      />,
    );
    fireEvent.click(screen.getByLabelText("펼치기"));
    for (let i = 0; i < 17; i++) {
      expect(screen.getByText(`c${i}`)).toBeTruthy();
    }
  });
});
