/**
 * /price-history page (M3) — vitest + @testing-library/react.
 *
 * Covers:
 *  - PriceHistoryControls: tab switch URL push, product/merchant selection URL
 *    push, memo-less toggle URL push.
 *  - PriceHistoryChart / MerchantStatsChart empty-state rendering. Recharts is
 *    mocked out (no jsdom layout) so the chart bodies are not verified — the
 *    page-level tests guard against schema drift via the chart component
 *    contract (a test-id wrapper is rendered iff data is non-empty).
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { PriceHistoryControls } from "../components/price-history-controls";
import { PriceHistoryChart } from "../components/price-history-chart";
import { MerchantStatsChart } from "../components/merchant-stats-chart";
import type {
  ProductItem,
  MerchantItem,
  PriceHistoryResponse,
  MerchantStatsResponse,
} from "../lib/schemas";

// ── next/navigation mock ─────────────────────────────────────────────────────

const mockPush = vi.fn();
const mockSearchParams = { current: new URLSearchParams() };

vi.mock("next/navigation", () => ({
  useRouter: () => ({
    push: mockPush,
    replace: vi.fn(),
    refresh: vi.fn(),
  }),
  usePathname: () => "/price-history",
  useSearchParams: () => mockSearchParams.current,
}));

// Recharts uses ResizeObserver which jsdom doesn't expose, and its layout
// engine measures DOM rects. We replace ResponsiveContainer + chart primitives
// with plain divs so the wrapper data-testid remains assertable.
vi.mock("recharts", () => {
  const Passthrough = ({ children }: { children?: React.ReactNode }) => (
    <div>{children}</div>
  );
  const Empty = () => null;
  return {
    ResponsiveContainer: Passthrough,
    LineChart: Passthrough,
    BarChart: Passthrough,
    Line: Empty,
    Bar: Empty,
    XAxis: Empty,
    YAxis: Empty,
    CartesianGrid: Empty,
    Tooltip: Empty,
    Legend: Empty,
  };
});

beforeEach(() => {
  mockPush.mockClear();
  mockSearchParams.current = new URLSearchParams();
});

// ── Fixtures ─────────────────────────────────────────────────────────────────

const products: ProductItem[] = [
  {
    id: "11111111-1111-1111-1111-111111111111",
    name: "아이스아메리카노",
    merchant_id: "22222222-2222-2222-2222-222222222222",
    merchant_name: "고덕방",
    review_state: "confirmed",
    transaction_count: 6,
  },
  {
    id: "33333333-3333-3333-3333-333333333333",
    name: "조닌끼안티",
    merchant_id: null,
    merchant_name: null,
    review_state: "pending",
    transaction_count: 0,
  },
];

const merchants: MerchantItem[] = [
  {
    id: "22222222-2222-2222-2222-222222222222",
    name: "고덕방",
    review_state: "confirmed",
  },
  {
    id: "44444444-4444-4444-4444-444444444444",
    name: "이마트",
    review_state: "confirmed",
  },
];

// ── Controls ────────────────────────────────────────────────────────────────

describe("PriceHistoryControls", () => {
  it("clicking the Merchants tab pushes ?view=merchants and resets product_id", async () => {
    mockSearchParams.current = new URLSearchParams("view=products&product_id=11111111-1111-1111-1111-111111111111");
    const user = userEvent.setup();
    render(
      <PriceHistoryControls
        view="products"
        productId="11111111-1111-1111-1111-111111111111"
        merchantId={null}
        memoLessOnly={false}
        products={products}
        merchants={merchants}
      />,
    );

    await user.click(screen.getByTestId("tab-merchants"));

    // Radix Tabs may fire onValueChange more than once for a single click; the
    // contract we care about is that *at least one* push targets the merchants
    // view with the product_id cleared (reset between tabs).
    expect(mockPush).toHaveBeenCalled();
    const matched = mockPush.mock.calls.some((call) => {
      const target = call[0];
      if (typeof target !== "string" || !target.startsWith("/price-history?")) return false;
      const sp = new URLSearchParams(target.split("?")[1]);
      return sp.get("view") === "merchants" && sp.get("product_id") === null;
    });
    expect(matched).toBe(true);
  });

  it("selecting a product in Products tab pushes ?product_id=…", async () => {
    const user = userEvent.setup();
    render(
      <PriceHistoryControls
        view="products"
        productId={null}
        merchantId={null}
        memoLessOnly={false}
        products={products}
        merchants={merchants}
      />,
    );

    const picker = screen.getByTestId("product-picker") as HTMLSelectElement;
    await user.selectOptions(picker, products[0].id);

    expect(mockPush).toHaveBeenCalledTimes(1);
    const target = mockPush.mock.calls[0][0] as string;
    const sp = new URLSearchParams(target.split("?")[1]);
    expect(sp.get("product_id")).toBe(products[0].id);
  });

  it("toggling memo-less in Merchants tab pushes ?memo_less=1 and removing it clears the param", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <PriceHistoryControls
        view="merchants"
        productId={null}
        merchantId={merchants[0].id}
        memoLessOnly={false}
        products={products}
        merchants={merchants}
      />,
    );

    await user.click(screen.getByTestId("memo-less-toggle"));
    expect(mockPush).toHaveBeenCalledTimes(1);
    {
      const sp = new URLSearchParams((mockPush.mock.calls[0][0] as string).split("?")[1]);
      expect(sp.get("memo_less")).toBe("1");
    }

    // Now untoggle (component re-rendered with the new state from server).
    mockSearchParams.current = new URLSearchParams("view=merchants&memo_less=1");
    rerender(
      <PriceHistoryControls
        view="merchants"
        productId={null}
        merchantId={merchants[0].id}
        memoLessOnly={true}
        products={products}
        merchants={merchants}
      />,
    );
    await user.click(screen.getByTestId("memo-less-toggle"));
    expect(mockPush).toHaveBeenCalledTimes(2);
    const target = mockPush.mock.calls[1][0] as string;
    // memo_less should be removed (URL has no '?' or no memo_less key).
    if (target.includes("?")) {
      const sp = new URLSearchParams(target.split("?")[1]);
      expect(sp.get("memo_less")).toBeNull();
    } else {
      // bare path means all params cleared
      expect(target).toBe("/price-history");
    }
  });
});

// ── Charts: empty states ────────────────────────────────────────────────────

describe("PriceHistoryChart", () => {
  it("renders the empty-state placeholder when points is []", () => {
    const data: PriceHistoryResponse = {
      product_id: products[0].id,
      product_name: "아이스아메리카노",
      merchant_id: products[0].merchant_id,
      merchant_name: products[0].merchant_name,
      points: [],
      total: 0,
      min_unit_price: null,
      max_unit_price: null,
      avg_unit_price: null,
    };
    render(<PriceHistoryChart data={data} />);
    expect(screen.getByTestId("price-history-empty")).toBeTruthy();
    expect(screen.queryByTestId("price-history-chart")).toBeNull();
  });

  it("renders the chart wrapper when points are present", () => {
    const data: PriceHistoryResponse = {
      product_id: products[0].id,
      product_name: "아이스아메리카노",
      merchant_id: products[0].merchant_id,
      merchant_name: products[0].merchant_name,
      points: [
        {
          transaction_id: "55555555-5555-5555-5555-555555555555",
          occurred_on: "2026-02-01",
          unit_price: "3400",
          quantity: "1",
          line_amount: "3400",
          merchant_id: products[0].merchant_id,
          merchant_name: products[0].merchant_name,
          memo: "아이스아메리카노",
        },
      ],
      total: 1,
      min_unit_price: "3400",
      max_unit_price: "3400",
      avg_unit_price: "3400",
    };
    render(<PriceHistoryChart data={data} />);
    expect(screen.getByTestId("price-history-chart")).toBeTruthy();
    expect(screen.queryByTestId("price-history-empty")).toBeNull();
  });
});

describe("MerchantStatsChart", () => {
  it("renders the empty-state placeholder when points is []", () => {
    const data: MerchantStatsResponse = {
      merchant_id: merchants[0].id,
      merchant_name: merchants[0].name,
      points: [],
      grand_total: "0",
      transaction_count: 0,
      memo_less_count: 0,
    };
    render(<MerchantStatsChart data={data} />);
    expect(screen.getByTestId("merchant-stats-empty")).toBeTruthy();
    expect(screen.queryByTestId("merchant-stats-chart")).toBeNull();
  });

  it("renders the chart wrapper when points are present", () => {
    const data: MerchantStatsResponse = {
      merchant_id: merchants[0].id,
      merchant_name: merchants[0].name,
      points: [
        {
          month: "2026-02-01",
          total: "20400",
          transaction_count: 6,
          memo_less_count: 0,
        },
      ],
      grand_total: "20400",
      transaction_count: 6,
      memo_less_count: 0,
    };
    render(<MerchantStatsChart data={data} />);
    expect(screen.getByTestId("merchant-stats-chart")).toBeTruthy();
  });
});
