import type { SummaryResponse } from "./schemas";

export type DonutSlice = {
  name: string;
  value: number;
  color: string;
  isDeduction: boolean;
  isOther: boolean;
};

export type ActorDonutData = {
  actorId: string | null;
  actorName: string;
  total: number;
  slices: DonutSlice[];
};

export const PALETTE = [
  "#2563eb",
  "#16a34a",
  "#f59e0b",
  "#db2777",
  "#7c3aed",
  "#0891b2",
] as const;
export const OTHER_COLOR = "#94a3b8";
export const DEDUCTION_COLOR = "#6b7280";

const TOP_N = 6;
const DEDUCTION_NAME = "차감";
const OTHER_NAME = "기타";

function signedNumber(amount: string, sign: number): number {
  const v = parseFloat(amount);
  if (Number.isNaN(v)) return 0;
  return sign < 0 ? -v : v;
}

function actorNameFor(data: SummaryResponse, actorId: string | null): string {
  const fromActors = data.actors.find((a) => a.actor_id === actorId);
  if (fromActors) return fromActors.actor_name;
  for (const cat of data.categories) {
    const cell = cat.by_actor.find((e) => e.actor_id === actorId);
    if (cell) return cell.actor_name;
  }
  return actorId ?? "미지정";
}

export function buildActorSlices(
  data: SummaryResponse,
  actorId: string | null,
): ActorDonutData {
  const actorName = actorNameFor(data, actorId);

  type Raw = { name: string; value: number; isDeduction: boolean };
  const raws: Raw[] = [];

  for (const cat of data.categories) {
    const cell = cat.by_actor.find((e) => e.actor_id === actorId);
    if (!cell) continue;
    const v = signedNumber(cell.amount, cell.sign);
    if (v === 0) continue;
    raws.push({
      name: cat.category_name,
      value: v,
      isDeduction: cat.category_name === DEDUCTION_NAME,
    });
  }

  const total = raws.reduce((acc, r) => acc + r.value, 0);

  const deductions = raws.filter((r) => r.isDeduction);
  const rest = raws
    .filter((r) => !r.isDeduction)
    .sort((a, b) => Math.abs(b.value) - Math.abs(a.value));

  const top = rest.slice(0, TOP_N);
  const tail = rest.slice(TOP_N);

  const slices: DonutSlice[] = top.map((r, i) => ({
    name: r.name,
    value: r.value,
    color: PALETTE[i % PALETTE.length],
    isDeduction: false,
    isOther: false,
  }));

  if (tail.length > 0) {
    slices.push({
      name: OTHER_NAME,
      value: tail.reduce((acc, r) => acc + r.value, 0),
      color: OTHER_COLOR,
      isDeduction: false,
      isOther: true,
    });
  }

  for (const d of deductions) {
    slices.push({
      name: d.name,
      value: d.value,
      color: DEDUCTION_COLOR,
      isDeduction: true,
      isOther: false,
    });
  }

  return { actorId, actorName, total, slices };
}
