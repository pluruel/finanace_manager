/**
 * Dashboard (M2 Step D) — vitest + @testing-library/react
 *
 * Covers:
 * 1. MonthPicker → URL sync (writes ?ym=YYYY-MM via router.push)
 * 2. SettlementCard empty-state (no data / zero recognized + zero deduction)
 * 3. SummaryPivot snapshot for Feb 2026 mocked data — 차감 row present,
 *    actor totals + grand total computed correctly.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { MonthPicker } from "../components/month-picker";
import { SettlementCard } from "../components/settlement-card";
import { SummaryPivot } from "../components/summary-pivot";
import type { SummaryResponse, Settlement } from "../lib/schemas";

// ── next/navigation mock ─────────────────────────────────────────────────────

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

beforeEach(() => {
  mockPush.mockClear();
});

// ── 1. MonthPicker URL sync ──────────────────────────────────────────────────

describe("MonthPicker URL sync", () => {
  it("clicking next-month pushes ?ym=YYYY-MM with the next month", async () => {
    const user = userEvent.setup();
    render(<MonthPicker year={2026} month={2} />);

    await user.click(screen.getByLabelText("다음 달"));

    expect(mockPush).toHaveBeenCalledTimes(1);
    expect(mockPush.mock.calls[0][0]).toBe("/?ym=2026-03");
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
    // type=month inputs do not accept userEvent.type; use fireEvent.change to set value.
    fireEvent.change(input, { target: { value: "2026-04" } });

    expect(mockPush).toHaveBeenCalledWith("/?ym=2026-04");
  });

  it("renders current YM in the input", () => {
    render(<MonthPicker year={2026} month={5} />);
    const input = screen.getByLabelText("월 선택") as HTMLInputElement;
    expect(input.value).toBe("2026-05");
  });
});

// ── 2. SettlementCard empty state ────────────────────────────────────────────

describe("SettlementCard empty state", () => {
  it("renders empty message when data is null", () => {
    render(<SettlementCard year={2026} month={3} data={null} />);
    expect(screen.getByTestId("settlement-empty").textContent).toContain(
      "2026년 3월 정산 데이터가 없습니다",
    );
  });

  it("renders empty message when both recognized and deduction are zero", () => {
    const data: Settlement = {
      year: 2026,
      month: 3,
      recognized_expense: "0",
      deducted_amount: "0",
      settlement_input: "0",
    };
    render(<SettlementCard year={2026} month={3} data={data} />);
    expect(screen.queryByTestId("settlement-empty")).toBeTruthy();
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

// ── 3. SummaryPivot snapshot for Feb 2026 ────────────────────────────────────

describe("SummaryPivot Feb 2026 mocked snapshot", () => {
  it("renders categories × actors with totals; 차감 appears as a normal row", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [
        { actor_id: "00000000-0000-0000-0000-000000000001", actor_name: "공동" },
        { actor_id: "00000000-0000-0000-0000-000000000002", actor_name: "엉아" },
      ],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [
            {
              actor_id: "00000000-0000-0000-0000-000000000001",
              actor_name: "공동",
              amount: "100000",
              sign: 1,
            },
            {
              actor_id: "00000000-0000-0000-0000-000000000002",
              actor_name: "엉아",
              amount: "20000",
              sign: 1,
            },
          ],
          total: "120000",
        },
        {
          category_id: "22222222-2222-2222-2222-222222222222",
          category_name: "차감",
          kind: "expense",
          by_actor: [
            {
              actor_id: "00000000-0000-0000-0000-000000000001",
              actor_name: "공동",
              amount: "7500",
              sign: 1,
            },
          ],
          total: "7500",
        },
      ],
    };

    render(<SummaryPivot data={data} />);

    // 차감 row present + badged
    expect(screen.getAllByText("차감").length).toBeGreaterThan(0);
    expect(screen.getAllByText("정산 차감").length).toBeGreaterThan(0);

    // Cells: 100,000 (공동/외식), 20,000 (엉아/외식), 7,500 (공동/차감)
    expect(screen.getAllByText("₩100,000").length).toBeGreaterThan(0);
    expect(screen.getAllByText("₩20,000").length).toBeGreaterThan(0);
    expect(screen.getAllByText("₩7,500").length).toBeGreaterThan(0);

    // Footer totals: 공동 = 100,000 + 7,500 = 107,500; 엉아 = 20,000; grand = 127,500
    expect(screen.getAllByText("₩107,500").length).toBeGreaterThan(0);
    expect(screen.getAllByText("₩127,500").length).toBeGreaterThan(0);
  });

  it("renders empty state when no categories", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 3,
      actors: [],
      categories: [],
    };
    render(<SummaryPivot data={data} />);
    expect(screen.getByText(/이 달의 거래 내역이 없습니다/)).toBeTruthy();
  });

  it("renders empty state when data is null", () => {
    render(<SummaryPivot data={null} />);
    expect(screen.getByText(/이 달의 거래 내역이 없습니다/)).toBeTruthy();
  });
});
