import { describe, it, expect } from "vitest";
import {
  buildActorSlices,
  buildHouseholdSlices,
  buildDeductionByActor,
  incomeFor,
  collectOrderedActorIds,
  EXPENSE_PALETTE,
  OTHER_COLOR,
  DEDUCTION_PALETTE,
} from "../lib/donut-data";
import type { SummaryResponse, IncomeResponse } from "../lib/schemas";

const ACTOR_A = "00000000-0000-0000-0000-0000000000aa";
const ACTOR_B = "00000000-0000-0000-0000-0000000000bb";

function makeData(
  categories: Array<{ name: string; cells: Array<{ actor: string; amount: string }> }>,
): SummaryResponse {
  return {
    year: 2026,
    month: 2,
    actors: [
      { actor_id: ACTOR_A, actor_name: "공동" },
      { actor_id: ACTOR_B, actor_name: "엉아" },
    ],
    categories: categories.map((c, i) => ({
      category_id: `${"1".repeat(8)}-1111-1111-1111-${String(i).padStart(12, "0")}`,
      category_name: c.name,
      kind: "expense",
      by_actor: c.cells.map((cell) => ({
        actor_id: cell.actor,
        actor_name: cell.actor === ACTOR_A ? "공동" : "엉아",
        amount: cell.amount,
      })),
      total: "0",
    })),
  };
}

describe("buildActorSlices (지출 전용)", () => {
  it("차감 카테고리는 슬라이스에서 제외된다", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "차감", cells: [{ actor: ACTOR_A, amount: "9999" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices.map((s) => s.name)).toEqual(["외식"]);
    expect(result.total).toBe(1000);
  });

  it("expense 색상 팔레트(EXPENSE_PALETTE)를 사용한다", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices[0].color).toBe(EXPENSE_PALETTE[0]);
  });

  it("7개 비차감 카테고리 → top-6 + 기타", () => {
    const data = makeData([
      { name: "c1", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "c2", cells: [{ actor: ACTOR_A, amount: "2000" }] },
      { name: "c3", cells: [{ actor: ACTOR_A, amount: "3000" }] },
      { name: "c4", cells: [{ actor: ACTOR_A, amount: "4000" }] },
      { name: "c5", cells: [{ actor: ACTOR_A, amount: "5000" }] },
      { name: "c6", cells: [{ actor: ACTOR_A, amount: "6000" }] },
      { name: "c7", cells: [{ actor: ACTOR_A, amount: "700" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices.map((s) => s.name)).toEqual([
      "c6", "c5", "c4", "c3", "c2", "c1", "기타",
    ]);
    expect(result.slices[6].color).toBe(OTHER_COLOR);
  });

  it("환불(음수) 슬라이스는 보존되지만 합계를 깎는다", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "환불", cells: [{ actor: ACTOR_A, amount: "-300" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.total).toBe(700);
    expect(result.slices.map((s) => s.name).sort()).toEqual(["환불", "외식"].sort());
  });

  it("액터에 expense 행이 없으면 빈 슬라이스 + 0 total", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_B, amount: "100" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices).toEqual([]);
    expect(result.total).toBe(0);
  });
});

describe("buildHouseholdSlices", () => {
  it("모든 액터의 expense 를 카테고리별로 합산한다 (차감 제외)", () => {
    const data = makeData([
      { name: "외식", cells: [
        { actor: ACTOR_A, amount: "1000" },
        { actor: ACTOR_B, amount: "2000" },
      ]},
      { name: "병원", cells: [
        { actor: ACTOR_A, amount: "500" },
      ]},
      { name: "차감", cells: [
        { actor: ACTOR_A, amount: "200" },
      ]},
    ]);
    const result = buildHouseholdSlices(data);
    const map = new Map(result.slices.map((s) => [s.name, s.value]));
    expect(map.get("외식")).toBe(3000);
    expect(map.get("병원")).toBe(500);
    expect(map.has("차감")).toBe(false);
    expect(result.actorName).toBe("가구 합계");
    expect(result.total).toBe(3500);
  });

  it("동일 카테고리의 actor 합산 후 top-6 + 기타 적용", () => {
    const data = makeData([
      { name: "c1", cells: [{ actor: ACTOR_A, amount: "1000" }, { actor: ACTOR_B, amount: "100" }] },
      { name: "c2", cells: [{ actor: ACTOR_A, amount: "2000" }] },
      { name: "c3", cells: [{ actor: ACTOR_A, amount: "3000" }] },
      { name: "c4", cells: [{ actor: ACTOR_A, amount: "4000" }] },
      { name: "c5", cells: [{ actor: ACTOR_A, amount: "5000" }] },
      { name: "c6", cells: [{ actor: ACTOR_A, amount: "6000" }] },
      { name: "c7", cells: [{ actor: ACTOR_A, amount: "700" }] },
    ]);
    const result = buildHouseholdSlices(data);
    const names = result.slices.map((s) => s.name);
    expect(names[0]).toBe("c6"); // 6000 (가장 큼)
    expect(names).toContain("기타");
    expect(names.length).toBe(7); // top-6 + 기타
  });
});

describe("buildDeductionByActor", () => {
  it("차감 카테고리를 액터별 슬라이스로 분해한다", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "차감", cells: [
        { actor: ACTOR_A, amount: "300" },
        { actor: ACTOR_B, amount: "200" },
      ]},
    ]);
    const result = buildDeductionByActor(data);
    expect(result.slices.length).toBe(2);
    const map = new Map(result.slices.map((s) => [s.name, s.value]));
    expect(map.get("공동")).toBe(300);
    expect(map.get("엉아")).toBe(200);
    expect(result.total).toBe(500);
    // 회색조 팔레트 사용
    for (const s of result.slices) {
      expect(DEDUCTION_PALETTE).toContain(s.color);
    }
  });

  it("차감 0 인 액터는 슬라이스에서 제외", () => {
    const data = makeData([
      { name: "차감", cells: [{ actor: ACTOR_A, amount: "300" }] },
    ]);
    const result = buildDeductionByActor(data);
    expect(result.slices.map((s) => s.name)).toEqual(["공동"]);
  });

  it("차감 카테고리 자체가 없으면 빈 결과", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
    ]);
    const result = buildDeductionByActor(data);
    expect(result.slices).toEqual([]);
    expect(result.total).toBe(0);
  });
});

describe("incomeFor", () => {
  const income: IncomeResponse = {
    month: "2026-02",
    by_actor: [
      { actor_id: ACTOR_A, actor_name: "공동", total: "0" },
      { actor_id: ACTOR_B, actor_name: "엉아", total: "1000" },
    ],
    total: "1000",
  };

  it("'household' 키워드는 전체 합계를 반환한다", () => {
    expect(incomeFor(income, "household")).toBe(1000);
  });

  it("actor_id 매치 시 해당 액터 income 반환", () => {
    expect(incomeFor(income, ACTOR_B)).toBe(1000);
    expect(incomeFor(income, ACTOR_A)).toBe(0);
  });

  it("매치 없으면 0 반환", () => {
    expect(incomeFor(income, "nonexistent-id")).toBe(0);
  });

  it("income 데이터가 null 이면 0 반환", () => {
    expect(incomeFor(null, "household")).toBe(0);
    expect(incomeFor(null, ACTOR_A)).toBe(0);
  });
});

describe("collectOrderedActorIds", () => {
  const A = "00000000-0000-0000-0000-0000000000aa";
  const B = "00000000-0000-0000-0000-0000000000bb";

  it("returns empty array when data is null", () => {
    expect(collectOrderedActorIds(null)).toEqual([]);
  });

  it("preserves data.actors order", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [
        { actor_id: A, actor_name: "공동" },
        { actor_id: B, actor_name: "엉아" },
      ],
      categories: [],
    };
    expect(collectOrderedActorIds(data)).toEqual([A, B]);
  });
});
