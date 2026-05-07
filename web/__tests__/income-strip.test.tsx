import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { IncomeStrip } from "@/components/income-strip";
import type { IncomeResponse } from "@/lib/schemas";

const data: IncomeResponse = {
  month: "2026-02",
  by_actor: [
    { actor_id: "00000000-0000-0000-0000-000000000001", actor_name: "공동", total: "0" },
    { actor_id: "00000000-0000-0000-0000-000000000002", actor_name: "엉아", total: "3500000" },
    { actor_id: "00000000-0000-0000-0000-000000000003", actor_name: "아기", total: "0" },
  ],
  total: "3500000",
};

describe("IncomeStrip", () => {
  it("액터별 수입을 표시한다", () => {
    render(<IncomeStrip data={data} />);
    expect(screen.getByText("공동")).toBeTruthy();
    expect(screen.getByText("엉아")).toBeTruthy();
    expect(screen.getByText("아기")).toBeTruthy();
    expect(screen.getAllByText(/3,500,000/).length).toBeGreaterThanOrEqual(1);
  });

  it("거래 없는 액터도 ₩0 으로 보여준다", () => {
    render(<IncomeStrip data={data} />);
    const zeroLabels = screen.getAllByText(/₩\s*0\b/);
    expect(zeroLabels.length).toBeGreaterThanOrEqual(2);
  });

  it("data=null 이면 아무것도 렌더하지 않는다", () => {
    const { container } = render(<IncomeStrip data={null} />);
    expect(container.firstChild).toBeNull();
  });
});
