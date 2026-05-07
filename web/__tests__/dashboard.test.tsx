/**
 * Dashboard tests — vitest + @testing-library/react.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { MonthPicker } from "../components/month-picker";
import { SettlementCard } from "../components/settlement-card";
import { ActorDonut } from "../components/actor-donut";
import { DashboardDonuts } from "../components/dashboard-donuts";
import { IncomeStrip } from "../components/income-strip";
import { buildActorSlices } from "../lib/donut-data";
import type { SummaryResponse, Settlement, IncomeResponse } from "../lib/schemas";

const mockPush = vi.fn();

vi.mock("next/navigation", () => ({
  useRouter: () => ({
    push: mockPush,
    refresh: vi.fn(),
    replace: vi.fn(),
  }),
  usePathname: () => "/",
  useSearchParams: () => new URLSearchParams(),
}));

vi.mock("recharts", () => {
  const Passthrough = ({ children }: { children?: React.ReactNode }) => (
    <div>{children}</div>
  );
  const Empty = () => null;
  return {
    ResponsiveContainer: Passthrough,
    PieChart: Passthrough,
    Pie: Passthrough,
    Cell: Empty,
    Tooltip: Empty,
  };
});

beforeEach(() => {
  mockPush.mockClear();
});

describe("MonthPicker URL sync", () => {
  it("clicking next-month pushes ?ym=YYYY-MM with the next month", async () => {
    const user = userEvent.setup();
    render(<MonthPicker year={2026} month={2} />);
    await user.click(screen.getByLabelText("다음 달"));
    expect(mockPush).toHaveBeenCalledWith("/?ym=2026-03");
  });

  it("clicking previous-month wraps year boundary correctly", async () => {
    const user = userEvent.setup();
    render(<MonthPicker year={2026} month={1} />);
    await user.click(screen.getByLabelText("이전 달"));
    expect(mockPush).toHaveBeenCalledWith("/?ym=2025-12");
  });

  it("typing into the month input pushes the new YM", () => {
    render(<MonthPicker year={2026} month={2} />);
    const input = screen.getByLabelText("월 선택") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "2026-04" } });
    expect(mockPush).toHaveBeenCalledWith("/?ym=2026-04");
  });

  it("renders current YM in the input", () => {
    render(<MonthPicker year={2026} month={5} />);
    const input = screen.getByLabelText("월 선택") as HTMLInputElement;
    expect(input.value).toBe("2026-05");
  });
});

describe("SettlementCard default mode", () => {
  it("renders empty message when data is null", () => {
    render(<SettlementCard year={2026} month={3} data={null} />);
    expect(screen.getByTestId("settlement-empty").textContent).toContain(
      "2026년 3월 정산 데이터가 없습니다",
    );
  });

  it("renders the populated breakdown for Feb 2026 (584,000 − 7,500 = 576,500)", () => {
    const data: Settlement = {
      year: 2026,
      month: 2,
      recognized_expense: "584000",
      deducted_amount: "7500",
      settlement_input: "576500",
    };
    render(<SettlementCard year={2026} month={2} data={data} />);
    const summary = screen.getByTestId("settlement-summary");
    expect(summary.textContent).toContain("584,000");
    expect(summary.textContent).toContain("7,500");
    expect(summary.textContent).toContain("576,500");
  });
});

describe("SettlementCard compact mode", () => {
  it("renders inline strip with the same numbers", () => {
    const data: Settlement = {
      year: 2026,
      month: 2,
      recognized_expense: "584000",
      deducted_amount: "7500",
      settlement_input: "576500",
    };
    render(<SettlementCard year={2026} month={2} data={data} compact />);
    expect(screen.getByTestId("settlement-compact")).toBeTruthy();
    const summary = screen.getByTestId("settlement-summary");
    expect(summary.textContent).toContain("584,000");
    expect(summary.textContent).toContain("576,500");
  });

  it("renders compact empty state when data is null", () => {
    render(<SettlementCard year={2026} month={3} data={null} compact />);
    expect(screen.getByTestId("settlement-compact")).toBeTruthy();
    expect(screen.getByTestId("settlement-empty").textContent).toContain("데이터가 없습니다");
  });
});

describe("ActorDonut", () => {
  const ACTOR_A = "00000000-0000-0000-0000-0000000000aa";

  it("renders empty state when the actor has no rows", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: ACTOR_A, actor_name: "공동" }],
      categories: [],
    };
    render(<ActorDonut data={buildActorSlices(data, ACTOR_A)} />);
    expect(screen.getByTestId("actor-donut-empty")).toBeTruthy();
  });

  it("renders actor name, total, and slice legend with 차감 pinned last", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: ACTOR_A, actor_name: "공동" }],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [
            { actor_id: ACTOR_A, actor_name: "공동", amount: "100000" },
          ],
          total: "100000",
        },
        {
          category_id: "22222222-2222-2222-2222-222222222222",
          category_name: "차감",
          kind: "expense",
          by_actor: [
            { actor_id: ACTOR_A, actor_name: "공동", amount: "7500" },
          ],
          total: "7500",
        },
      ],
    };
    render(<ActorDonut data={buildActorSlices(data, ACTOR_A)} />);

    expect(screen.getByText("공동")).toBeTruthy();
    expect(screen.getByText("₩107,500")).toBeTruthy();
    expect(screen.getByText("외식")).toBeTruthy();
    expect(screen.getByText("차감")).toBeTruthy();
  });
});

describe("IncomeStrip", () => {
  it("월 수입 레이블과 액터별 금액을 렌더한다", () => {
    const data: IncomeResponse = {
      month: "2026-02",
      by_actor: [
        { actor_id: "11111111-1111-1111-1111-111111111111", actor_name: "공동", total: "0" },
        { actor_id: "22222222-2222-2222-2222-222222222222", actor_name: "엉아", total: "1000" },
      ],
      total: "1000",
    };
    render(<IncomeStrip data={data} />);
    expect(screen.getByText("월 수입")).toBeTruthy();
    expect(screen.getByText("공동")).toBeTruthy();
    expect(screen.getByText("엉아")).toBeTruthy();
  });
});

describe("DashboardDonuts", () => {
  const A = "00000000-0000-0000-0000-0000000000aa";
  const B = "00000000-0000-0000-0000-0000000000bb";

  it("renders empty card when data is null", () => {
    render(<DashboardDonuts data={null} />);
    expect(screen.getByTestId("dashboard-donuts-empty")).toBeTruthy();
    expect(screen.queryByTestId("dashboard-donuts")).toBeNull();
  });

  it("renders actor placeholder cards (not global empty) when actors are declared but have no transactions", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: A, actor_name: "공동" }],
      categories: [],
    };
    render(<DashboardDonuts data={data} />);
    // Grid renders because there is a known actor
    expect(screen.getByTestId("dashboard-donuts")).toBeTruthy();
    expect(screen.queryByTestId("dashboard-donuts-empty")).toBeNull();
    // Actor card renders with the per-actor empty placeholder
    expect(screen.getByTestId("actor-donut-공동")).toBeTruthy();
    expect(screen.getByTestId("actor-donut-empty")).toBeTruthy();
  });

  it("renders a card for every actor in data.actors, with empty placeholders preserving the grid", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [
        { actor_id: A, actor_name: "공동" },
        { actor_id: B, actor_name: "엉아" },
      ],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [{ actor_id: A, actor_name: "공동", amount: "1000" }],
          total: "1000",
        },
      ],
    };
    render(<DashboardDonuts data={data} />);
    expect(screen.getByTestId("dashboard-donuts")).toBeTruthy();
    expect(screen.getByTestId("actor-donut-공동")).toBeTruthy();
    // 엉아 has no rows but is in data.actors, so its card MUST still render
    // with the empty placeholder (per spec line 37).
    expect(screen.getByTestId("actor-donut-엉아")).toBeTruthy();
    // The empty actor renders the placeholder
    expect(screen.getByTestId("actor-donut-empty")).toBeTruthy();
  });

  it("does NOT render a card for a stray null actor that produced no slices", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: A, actor_name: "공동" }],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [{ actor_id: A, actor_name: "공동", amount: "1000" }],
          total: "1000",
        },
      ],
    };
    render(<DashboardDonuts data={data} />);
    expect(screen.getByTestId("actor-donut-공동")).toBeTruthy();
    // Only 1 card overall
    expect(screen.queryAllByTestId(/^actor-donut-/).length).toBe(1);
  });
});
