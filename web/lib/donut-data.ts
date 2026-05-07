import type { SummaryResponse, IncomeResponse } from "./schemas";

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

// 카테고리별 슬라이스 구분이 잘 되도록 채도 균형 잡힌 qualitative 팔레트.
export const EXPENSE_PALETTE = [
  "#6366f1", // indigo-500
  "#10b981", // emerald-500
  "#f59e0b", // amber-500
  "#06b6d4", // cyan-500
  "#a855f7", // purple-500
  "#f97316", // orange-500
  "#14b8a6", // teal-500
  "#ec4899", // pink-500
] as const;

export const OTHER_COLOR = "#94a3b8"; // slate-400
export const DEDUCTION_PALETTE = [
  "#be123c", // rose-700
  "#c2410c", // orange-700
  "#a16207", // yellow-700
  "#65a30d", // lime-600
] as const;

const TOP_N = 6;
const DEDUCTION_NAME = "차감";
const OTHER_NAME = "기타";
const HOUSEHOLD_NAME = "가구 합계";

function actorNameFor(data: SummaryResponse, actorId: string | null): string {
  const fromActors = data.actors.find((a) => a.actor_id === actorId);
  if (fromActors) return fromActors.actor_name;
  for (const cat of data.categories) {
    const cell = cat.by_actor.find((e) => e.actor_id === actorId);
    if (cell) return cell.actor_name;
  }
  return actorId ?? "미지정";
}

type ExpenseRaw = { name: string; value: number };

function topNWithOther(rest: ExpenseRaw[]): DonutSlice[] {
  const sorted = [...rest].sort((a, b) => Math.abs(b.value) - Math.abs(a.value));
  const top = sorted.slice(0, TOP_N);
  const tail = sorted.slice(TOP_N);

  const slices: DonutSlice[] = top.map((r, i) => ({
    name: r.name,
    value: r.value,
    color: EXPENSE_PALETTE[i % EXPENSE_PALETTE.length],
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
  return slices;
}

/**
 * 단일 액터의 expense 슬라이스 (차감 제외).
 */
export function buildActorSlices(
  data: SummaryResponse,
  actorId: string | null,
): ActorDonutData {
  const actorName = actorNameFor(data, actorId);
  const raws: ExpenseRaw[] = [];

  for (const cat of data.categories) {
    if (cat.category_name === DEDUCTION_NAME) continue;
    const cell = cat.by_actor.find((e) => e.actor_id === actorId);
    if (!cell) continue;
    const v = parseFloat(cell.amount);
    if (Number.isNaN(v) || v === 0) continue;
    raws.push({ name: cat.category_name, value: v });
  }

  const total = raws.reduce((acc, r) => acc + r.value, 0);
  const slices = topNWithOther(raws);
  return { actorId, actorName, total, slices };
}

/**
 * 가구 전체 합계 — 모든 액터의 expense 를 카테고리별로 합산 (차감 제외).
 */
export function buildHouseholdSlices(data: SummaryResponse): ActorDonutData {
  const sums = new Map<string, number>();
  for (const cat of data.categories) {
    if (cat.category_name === DEDUCTION_NAME) continue;
    let agg = 0;
    for (const cell of cat.by_actor) {
      const v = parseFloat(cell.amount);
      if (!Number.isNaN(v)) agg += v;
    }
    if (agg !== 0) sums.set(cat.category_name, agg);
  }

  const raws: ExpenseRaw[] = Array.from(sums.entries()).map(([name, value]) => ({
    name,
    value,
  }));
  const total = raws.reduce((acc, r) => acc + r.value, 0);
  const slices = topNWithOther(raws);

  return {
    actorId: "household",
    actorName: HOUSEHOLD_NAME,
    total,
    slices,
  };
}

/**
 * 차감 카테고리만 액터별로 분해 → 액터당 1 슬라이스.
 * 0 인 액터는 슬라이스에서 제외. 회색조 팔레트.
 */
export function buildDeductionByActor(data: SummaryResponse): ActorDonutData {
  const deductionCat = data.categories.find((c) => c.category_name === DEDUCTION_NAME);
  if (!deductionCat) {
    return { actorId: null, actorName: DEDUCTION_NAME, total: 0, slices: [] };
  }

  type Row = { actorName: string; value: number };
  const rows: Row[] = [];
  for (const cell of deductionCat.by_actor) {
    const v = parseFloat(cell.amount);
    if (Number.isNaN(v) || v === 0) continue;
    rows.push({ actorName: cell.actor_name, value: v });
  }

  rows.sort((a, b) => Math.abs(b.value) - Math.abs(a.value));

  const slices: DonutSlice[] = rows.map((r, i) => ({
    name: r.actorName,
    value: r.value,
    color: DEDUCTION_PALETTE[i % DEDUCTION_PALETTE.length],
    isDeduction: true,
    isOther: false,
  }));

  const total = rows.reduce((acc, r) => acc + r.value, 0);
  return { actorId: null, actorName: DEDUCTION_NAME, total, slices };
}

/**
 * income lookup. actorRef 가 "household" 면 전체 합계, 그 외엔 actor_id 매치.
 */
export function incomeFor(
  income: IncomeResponse | null,
  actorRef: string | "household" | null,
): number {
  if (!income) return 0;
  if (actorRef === "household") {
    const v = parseFloat(income.total);
    return Number.isNaN(v) ? 0 : v;
  }
  const row = income.by_actor.find((e) => e.actor_id === actorRef);
  if (!row) return 0;
  const v = parseFloat(row.total);
  return Number.isNaN(v) ? 0 : v;
}

export function collectOrderedActorIds(
  data: SummaryResponse | null,
): Array<string | null> {
  if (!data) return [];
  const seen = new Set<string | null>();
  const ordered: Array<string | null> = [];
  for (const a of data.actors) {
    if (!seen.has(a.actor_id)) {
      seen.add(a.actor_id);
      ordered.push(a.actor_id);
    }
  }
  for (const cat of data.categories) {
    for (const cell of cat.by_actor) {
      if (!seen.has(cell.actor_id)) {
        seen.add(cell.actor_id);
        ordered.push(cell.actor_id);
      }
    }
  }
  return ordered;
}

/**
 * 단일 액터의 수입 슬라이스. expense 와 다른 점:
 *   - 차감 제외 로직 불필요 (income kind 만 들어옴)
 *   - 부호 그대로 (양수)
 */
export function buildActorIncomeSlices(
  income: IncomeResponse | null,
  actorId: string | null,
): ActorDonutData {
  const resolveName = (): string => {
    if (!income) return actorId ?? "미지정";
    const fromTotals = income.by_actor.find((a) => a.actor_id === actorId);
    if (fromTotals) return fromTotals.actor_name;
    for (const cat of income.categories) {
      const cell = cat.by_actor.find((e) => e.actor_id === actorId);
      if (cell) return cell.actor_name;
    }
    return actorId ?? "미지정";
  };

  if (!income) {
    return { actorId, actorName: resolveName(), total: 0, slices: [] };
  }
  const raws: ExpenseRaw[] = [];
  for (const cat of income.categories) {
    const cell = cat.by_actor.find((e) => e.actor_id === actorId);
    if (!cell) continue;
    const v = parseFloat(cell.amount);
    if (Number.isNaN(v) || v === 0) continue;
    raws.push({ name: cat.category_name, value: v });
  }
  const total = raws.reduce((acc, r) => acc + r.value, 0);
  const slices = topNWithOther(raws);
  return { actorId, actorName: resolveName(), total, slices };
}

/**
 * 가구 전체 수입 — 모든 액터의 income 을 카테고리별로 합산.
 */
export function buildHouseholdIncomeSlices(
  income: IncomeResponse | null,
): ActorDonutData {
  if (!income) {
    return { actorId: "household", actorName: HOUSEHOLD_NAME, total: 0, slices: [] };
  }
  const sums = new Map<string, number>();
  for (const cat of income.categories) {
    let agg = 0;
    for (const cell of cat.by_actor) {
      const v = parseFloat(cell.amount);
      if (!Number.isNaN(v)) agg += v;
    }
    if (agg !== 0) sums.set(cat.category_name, agg);
  }
  const raws: ExpenseRaw[] = Array.from(sums.entries()).map(([name, value]) => ({
    name,
    value,
  }));
  const total = raws.reduce((acc, r) => acc + r.value, 0);
  const slices = topNWithOther(raws);
  return {
    actorId: "household",
    actorName: HOUSEHOLD_NAME,
    total,
    slices,
  };
}
