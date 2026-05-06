import { describe, it, expect } from "vitest";
import { buildActorSlices, DEDUCTION_COLOR, OTHER_COLOR } from "../lib/donut-data";
import type { SummaryResponse } from "../lib/schemas";

const ACTOR_A = "00000000-0000-0000-0000-0000000000aa";
const ACTOR_B = "00000000-0000-0000-0000-0000000000bb";

function makeData(
  categories: Array<{ name: string; cells: Array<{ actor: string; amount: string; sign?: number }> }>,
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
        sign: cell.sign ?? 1,
      })),
      total: "0",
    })),
  };
}

describe("buildActorSlices", () => {
  it("returns empty slices and zero total when actor has no rows", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_B, amount: "1000" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices).toEqual([]);
    expect(result.total).toBe(0);
    expect(result.actorName).toBe("공동");
  });

  it("returns a single slice when actor has one non-deduction category", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "5000" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices.map((s) => s.name)).toEqual(["외식"]);
    expect(result.total).toBe(5000);
  });

  it("groups categories beyond top-6 into a 기타 slice, sorted by absolute amount desc", () => {
    const data = makeData([
      { name: "c1", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "c2", cells: [{ actor: ACTOR_A, amount: "2000" }] },
      { name: "c3", cells: [{ actor: ACTOR_A, amount: "3000" }] },
      { name: "c4", cells: [{ actor: ACTOR_A, amount: "4000" }] },
      { name: "c5", cells: [{ actor: ACTOR_A, amount: "5000" }] },
      { name: "c6", cells: [{ actor: ACTOR_A, amount: "6000" }] },
      { name: "c7", cells: [{ actor: ACTOR_A, amount: "700" }] },
      { name: "c8", cells: [{ actor: ACTOR_A, amount: "300" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices.map((s) => s.name)).toEqual(["c6", "c5", "c4", "c3", "c2", "c1", "기타"]);
    expect(result.slices[6].value).toBe(1000); // 700 + 300
    expect(result.slices[6].isOther).toBe(true);
    expect(result.slices[6].color).toBe(OTHER_COLOR);
    expect(result.total).toBe(1000 + 2000 + 3000 + 4000 + 5000 + 6000 + 700 + 300);
  });

  it("does not produce a 기타 slice when there are exactly 6 non-deduction categories", () => {
    const data = makeData([
      { name: "c1", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "c2", cells: [{ actor: ACTOR_A, amount: "2000" }] },
      { name: "c3", cells: [{ actor: ACTOR_A, amount: "3000" }] },
      { name: "c4", cells: [{ actor: ACTOR_A, amount: "4000" }] },
      { name: "c5", cells: [{ actor: ACTOR_A, amount: "5000" }] },
      { name: "c6", cells: [{ actor: ACTOR_A, amount: "6000" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices.map((s) => s.name)).toEqual(["c6", "c5", "c4", "c3", "c2", "c1"]);
    expect(result.slices.some((s) => s.isOther)).toBe(false);
  });

  it("always pins 차감 as its own slice at the end with the deduction color, regardless of rank", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "차감", cells: [{ actor: ACTOR_A, amount: "9999999" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices.map((s) => s.name)).toEqual(["외식", "차감"]);
    const deduction = result.slices[1];
    expect(deduction.isDeduction).toBe(true);
    expect(deduction.color).toBe(DEDUCTION_COLOR);
    expect(deduction.value).toBe(9999999);
  });

  it("excludes 차감 from the top-6 ranking — 7 non-deduction + 차감 yields top-6 + 기타 + 차감", () => {
    const data = makeData([
      { name: "c1", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "c2", cells: [{ actor: ACTOR_A, amount: "2000" }] },
      { name: "c3", cells: [{ actor: ACTOR_A, amount: "3000" }] },
      { name: "c4", cells: [{ actor: ACTOR_A, amount: "4000" }] },
      { name: "c5", cells: [{ actor: ACTOR_A, amount: "5000" }] },
      { name: "c6", cells: [{ actor: ACTOR_A, amount: "6000" }] },
      { name: "c7", cells: [{ actor: ACTOR_A, amount: "7000" }] },
      { name: "차감", cells: [{ actor: ACTOR_A, amount: "500" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices.map((s) => s.name)).toEqual(["c7", "c6", "c5", "c4", "c3", "c2", "기타", "차감"]);
    expect(result.slices[6].isOther).toBe(true);
    expect(result.slices[7].isDeduction).toBe(true);
  });

  it("total includes 차감 and 기타 (signed sum of every original cell)", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "차감", cells: [{ actor: ACTOR_A, amount: "200" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.total).toBe(1200);
  });

  it("respects sign=-1 (refund / negative line) when summing", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000", sign: 1 }] },
      { name: "환불", cells: [{ actor: ACTOR_A, amount: "300", sign: -1 }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices.map((s) => s.name)).toEqual(["외식", "환불"]);
    expect(result.slices[1].value).toBe(-300);
    expect(result.total).toBe(700);
  });

  it("returns actorName='미지정' for null actor_id when name is absent in actors[]", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [{ actor_id: null, actor_name: "미지정", amount: "100", sign: 1 }],
          total: "100",
        },
      ],
    };
    const result = buildActorSlices(data, null);
    expect(result.actorName).toBe("미지정");
    expect(result.slices.map((s) => s.name)).toEqual(["외식"]);
  });
});
