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
import { DeductionDonut } from "../components/deduction-donut";
import { buildActorSlices, buildActorIncomeSlices } from "../lib/donut-data";
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
  const EMPTY_DONUT = { actorId: null, actorName: "공동", total: 0, slices: [] };

  function makeIncomeOne(actorId: string, amount: string): IncomeResponse {
    return {
      month: "2026-02",
      by_actor: [],
      total: "0",
      categories: [
        {
          category_id: "99999999-9999-9999-9999-999999999999",
          category_name: "급여",
          kind: "income",
          by_actor: [{ actor_id: actorId, actor_name: "공동", amount }],
          total: amount,
        },
      ],
    };
  }

  it("수입/지출 모두 비면 빈 placeholder", () => {
    render(<ActorDonut actorName="공동" expense={EMPTY_DONUT} income={EMPTY_DONUT} />);
    expect(screen.getByTestId("donut-empty")).toBeTruthy();
  });

  it("수입이 있으면 수입 도넛(차트 + 가운데 '수입 ₩X')을 렌더한다", () => {
    const incomeData = buildActorIncomeSlices(makeIncomeOne(ACTOR_A, "5741025"), ACTOR_A);
    render(<ActorDonut actorName="공동" expense={EMPTY_DONUT} income={incomeData} />);
    expect(screen.getByTestId("donut-income-chart")).toBeTruthy();
    const center = screen.getByTestId("donut-income-center");
    expect(center.textContent).toContain("수입");
    expect(center.textContent).toContain("5,741,025");
  });

  it("수입 = 0 일 때 수입 도넛 영역 미렌더", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: ACTOR_A, actor_name: "공동" }],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [{ actor_id: ACTOR_A, actor_name: "공동", amount: "1000" }],
          total: "1000",
        },
      ],
    };
    render(<ActorDonut actorName="공동" expense={buildActorSlices(data, ACTOR_A)} income={EMPTY_DONUT} />);
    expect(screen.queryByTestId("donut-income-chart")).toBeNull();
  });

  it("지출 중앙 라벨이 '지출' + 합계", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: ACTOR_A, actor_name: "공동" }],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [{ actor_id: ACTOR_A, actor_name: "공동", amount: "100000" }],
          total: "100000",
        },
      ],
    };
    render(<ActorDonut actorName="공동" expense={buildActorSlices(data, ACTOR_A)} income={EMPTY_DONUT} />);
    const center = screen.getByTestId("donut-center");
    expect(center.textContent).toContain("지출");
    expect(center.textContent).toContain("100,000");
  });

  it("지출 범례 % 분모는 Σ|value|", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: ACTOR_A, actor_name: "공동" }],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [{ actor_id: ACTOR_A, actor_name: "공동", amount: "1000" }],
          total: "1000",
        },
        {
          category_id: "22222222-2222-2222-2222-222222222222",
          category_name: "병원",
          kind: "expense",
          by_actor: [{ actor_id: ACTOR_A, actor_name: "공동", amount: "3000" }],
          total: "3000",
        },
      ],
    };
    render(<ActorDonut actorName="공동" expense={buildActorSlices(data, ACTOR_A)} income={EMPTY_DONUT} />);
    expect(screen.getByText(/병원/).parentElement?.parentElement?.textContent).toContain("75.0%");
    expect(screen.getByText(/외식/).parentElement?.parentElement?.textContent).toContain("25.0%");
  });

  it("수입은 있고 지출 슬라이스 0 → 수입 도넛 + '지출 없음' 텍스트", () => {
    const incomeData = buildActorIncomeSlices(makeIncomeOne(ACTOR_A, "1000"), ACTOR_A);
    render(<ActorDonut actorName="공동" expense={EMPTY_DONUT} income={incomeData} />);
    expect(screen.getByTestId("donut-income-chart")).toBeTruthy();
    expect(screen.getByTestId("donut-no-expense")).toBeTruthy();
    expect(screen.queryByTestId("donut-empty")).toBeNull();
  });

  it("수입 도넛에 카테고리 범례 <ul> 가 렌더되지 않는다", () => {
    const incomeData = buildActorIncomeSlices(makeIncomeOne(ACTOR_A, "5000000"), ACTOR_A);
    render(<ActorDonut actorName="공동" expense={EMPTY_DONUT} income={incomeData} />);
    // expense 가 비어있으므로 카드 전체에 범례 <ul> 가 존재하면 안 됨
    const stack = screen.getByTestId("donut-stack");
    expect(stack.querySelector("ul")).toBeNull();
  });
});

describe("DashboardDonuts (가구합계 / 아기 / 엉아 3장)", () => {
  const A = "00000000-0000-0000-0000-0000000000aa";
  const BABY = "00000000-0000-0000-0000-0000000000bb";
  const SPOUSE = "00000000-0000-0000-0000-0000000000cc";

  function makeIncome(): IncomeResponse {
    return {
      month: "2026-02",
      by_actor: [
        { actor_id: A, actor_name: "공동", total: "0" },
        { actor_id: BABY, actor_name: "아기", total: "5741025" },
        { actor_id: SPOUSE, actor_name: "엉아", total: "6475664" },
      ],
      total: "12216689",
    };
  }

  function makeSummary(): SummaryResponse {
    return {
      year: 2026,
      month: 2,
      actors: [
        { actor_id: A, actor_name: "공동" },
        { actor_id: BABY, actor_name: "아기" },
        { actor_id: SPOUSE, actor_name: "엉아" },
      ],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [
            { actor_id: A, actor_name: "공동", amount: "1000" },
            { actor_id: BABY, actor_name: "아기", amount: "200" },
            { actor_id: SPOUSE, actor_name: "엉아", amount: "300" },
          ],
          total: "1500",
        },
      ],
    };
  }

  it("data 가 null 이면 empty 카드", () => {
    render(<DashboardDonuts summary={null} income={null} />);
    expect(screen.getByTestId("dashboard-donuts-empty")).toBeTruthy();
  });

  it("3장의 카드를 고정 순서 가구합계 / 아기 / 엉아 로 렌더한다", () => {
    render(<DashboardDonuts summary={makeSummary()} income={makeIncome()} />);
    const cards = screen.getAllByTestId(/^actor-donut-/);
    // 첫 카드는 가구 합계, 다음은 아기, 다음은 엉아
    expect(cards[0].getAttribute("data-testid")).toBe("actor-donut-가구 합계");
    expect(cards[1].getAttribute("data-testid")).toBe("actor-donut-아기");
    expect(cards[2].getAttribute("data-testid")).toBe("actor-donut-엉아");
  });

  it("가구 합계 카드의 income 헤더는 income.total 을 사용한다", () => {
    render(<DashboardDonuts summary={makeSummary()} income={makeIncome()} />);
    const householdCard = screen.getByTestId("actor-donut-가구 합계");
    expect(householdCard.textContent).toContain("12,216,689");
  });

  it("아기 카드의 income 헤더는 by_actor 매치 값", () => {
    render(<DashboardDonuts summary={makeSummary()} income={makeIncome()} />);
    const babyCard = screen.getByTestId("actor-donut-아기");
    expect(babyCard.textContent).toContain("5,741,025");
  });

  it("income 0 인 아기 카드는 수입 헤더 미렌더", () => {
    const income: IncomeResponse = {
      month: "2026-02",
      by_actor: [
        { actor_id: BABY, actor_name: "아기", total: "0" },
        { actor_id: SPOUSE, actor_name: "엉아", total: "1000" },
      ],
      total: "1000",
    };
    render(<DashboardDonuts summary={makeSummary()} income={income} />);
    const babyCard = screen.getByTestId("actor-donut-아기");
    // 수입 헤더 testid 는 카드 내부에 없어야 함
    expect(babyCard.querySelector('[data-testid="donut-income"]')).toBeNull();
  });
});

describe("DeductionDonut", () => {
  const A = "00000000-0000-0000-0000-0000000000aa";
  const B = "00000000-0000-0000-0000-0000000000bb";

  it("차감 데이터가 없으면 null 반환 (DOM 미렌더)", () => {
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
    const { container } = render(<DeductionDonut summary={data} />);
    expect(container.firstChild).toBeNull();
  });

  it("차감 슬라이스를 액터 이름으로 렌더하고 중앙에 합계 표시", () => {
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
          category_name: "차감",
          kind: "expense",
          by_actor: [
            { actor_id: A, actor_name: "공동", amount: "300" },
            { actor_id: B, actor_name: "엉아", amount: "200" },
          ],
          total: "500",
        },
      ],
    };
    render(<DeductionDonut summary={data} />);
    expect(screen.getByTestId("deduction-donut")).toBeTruthy();
    expect(screen.getByText("공동")).toBeTruthy();
    expect(screen.getByText("엉아")).toBeTruthy();
    expect(screen.getByTestId("deduction-donut-center").textContent).toContain("500");
  });
});
