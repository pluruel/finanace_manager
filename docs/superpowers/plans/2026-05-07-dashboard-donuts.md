# Dashboard Donut-Grid Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the dashboard's category × actor pivot table and recent-transactions list with a multi-donut grid (one donut per actor, top-6 categories + 기타 + 차감) while slimming the settlement card to inline stats.

**Architecture:** Frontend-only change. The existing `GET /api/summary/:year/:month` already returns category × actor data. A new pure helper `web/lib/donut-data.ts` reshapes it into per-actor slice arrays (top-6 + 기타, with 차감 always pinned as its own slice). A new presentational `ActorDonut` component renders one `recharts` donut. A new server component `DashboardDonuts` orchestrates the grid. `page.tsx` swaps in the new section, drops `RecentTransactions`, and passes `compact` to `SettlementCard`.

**Tech Stack:** Next.js 15 App Router (server components), TypeScript, recharts (already in deps), shadcn/ui Card, vitest + @testing-library/react.

---

## File Structure

**Create:**
- `web/lib/donut-data.ts` — pure helper `buildActorSlices(data, actorId)`
- `web/components/actor-donut.tsx` — single donut card (presentational)
- `web/components/dashboard-donuts.tsx` — server component grid wrapper
- `web/__tests__/donut-data.test.ts` — unit tests for the helper

**Modify:**
- `web/components/settlement-card.tsx` — add optional `compact` prop
- `web/app/(app)/page.tsx` — drop `RecentTransactions`, swap `SummarySection` → `DashboardDonutsSection`, pass `compact` to `SettlementCard`
- `web/__tests__/dashboard.test.tsx` — drop pivot/recent tests, add donut + compact-settlement tests

**Delete:**
- `web/components/summary-pivot.tsx`

---

## Task 1: Slice-builder helper (TDD)

**Files:**
- Create: `web/lib/donut-data.ts`
- Test: `web/__tests__/donut-data.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/__tests__/donut-data.test.ts` with the full content:

```typescript
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
    // signed values: 1000, -300 → ranking by |x|: 외식(1000), 환불(300)
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd web && npx vitest run __tests__/donut-data.test.ts`
Expected: FAIL — module `../lib/donut-data` not found.

- [ ] **Step 3: Implement the helper**

Create `web/lib/donut-data.ts` with the full content:

```typescript
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

const PALETTE = [
  "#2563eb", // blue-600
  "#16a34a", // green-600
  "#f59e0b", // amber-500
  "#db2777", // pink-600
  "#7c3aed", // violet-600
  "#0891b2", // cyan-600
];
export const OTHER_COLOR = "#94a3b8"; // slate-400
export const DEDUCTION_COLOR = "#6b7280"; // gray-500

const TOP_N = 6;
const DEDUCTION_NAME = "차감";
const OTHER_NAME = "기타";

function signedNumber(amount: string, sign: number): number {
  const v = parseFloat(amount);
  if (Number.isNaN(v)) return 0;
  return sign === -1 ? -v : v;
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd web && npx vitest run __tests__/donut-data.test.ts`
Expected: PASS — 9 tests pass.

- [ ] **Step 5: Commit**

```bash
git add web/lib/donut-data.ts web/__tests__/donut-data.test.ts
git commit -m "feat(web): add donut slice builder for dashboard

Pure helper that turns the SummaryResponse + actor into a slice list:
top-6 categories by absolute amount, a 기타 bucket for the tail, and
차감 pinned as its own slice. Used by the upcoming dashboard donut grid.
"
```

---

## Task 2: ActorDonut presentational component

**Files:**
- Create: `web/components/actor-donut.tsx`

This task has no dedicated unit test — the component is exercised through `dashboard.test.tsx` in Task 4 (recharts is mocked, so component rendering, not chart geometry, is verified).

- [ ] **Step 1: Implement the component**

Create `web/components/actor-donut.tsx` with the full content:

```tsx
"use client";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from "recharts";
import { formatKRW } from "@/lib/utils";
import type { ActorDonutData, DonutSlice } from "@/lib/donut-data";

type Props = {
  data: ActorDonutData;
};

function pct(value: number, total: number): string {
  if (total === 0) return "0%";
  return `${((Math.abs(value) / Math.abs(total)) * 100).toFixed(1)}%`;
}

export function ActorDonut({ data }: Props) {
  const { actorName, total, slices } = data;
  const isEmpty = slices.length === 0;

  return (
    <Card data-testid={`actor-donut-${actorName}`}>
      <CardHeader>
        <CardTitle className="text-base">{actorName}</CardTitle>
      </CardHeader>
      <CardContent>
        {isEmpty ? (
          <p className="text-sm text-muted-foreground" data-testid="actor-donut-empty">
            이 달의 거래 내역이 없습니다.
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            <div className="relative h-44">
              <ResponsiveContainer width="100%" height="100%">
                <PieChart>
                  <Pie
                    data={slices.map((s) => ({ ...s, value: Math.abs(s.value) }))}
                    dataKey="value"
                    nameKey="name"
                    innerRadius={48}
                    outerRadius={72}
                    paddingAngle={1}
                    stroke="none"
                  >
                    {slices.map((s, i) => (
                      <Cell key={i} fill={s.color} />
                    ))}
                  </Pie>
                  <Tooltip
                    formatter={(v: number) => formatKRW(String(v))}
                    contentStyle={{ fontSize: 12 }}
                  />
                </PieChart>
              </ResponsiveContainer>
              <div className="absolute inset-0 pointer-events-none flex flex-col items-center justify-center">
                <span className="text-xs text-muted-foreground">합계</span>
                <span className="text-base font-semibold tabular-nums">
                  {formatKRW(String(total))}
                </span>
              </div>
            </div>
            <ul className="text-sm space-y-1">
              {slices.map((s: DonutSlice, i) => (
                <li
                  key={`${s.name}-${i}`}
                  className="flex items-center justify-between gap-2"
                >
                  <span className="flex items-center gap-2 truncate">
                    <span
                      className="inline-block h-2.5 w-2.5 rounded-sm shrink-0"
                      style={{ backgroundColor: s.color }}
                    />
                    <span className="truncate">{s.name}</span>
                  </span>
                  <span className="tabular-nums text-muted-foreground shrink-0">
                    {formatKRW(String(s.value))} · {pct(s.value, total)}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
```

- [ ] **Step 2: Verify TypeScript compiles**

Run: `cd web && npx tsc --noEmit`
Expected: PASS — no errors.

- [ ] **Step 3: Commit**

```bash
git add web/components/actor-donut.tsx
git commit -m "feat(web): add ActorDonut card

Presentational donut card with center-label total and side legend.
Consumes ActorDonutData from donut-data; recharts is mocked in tests.
"
```

---

## Task 3: SettlementCard `compact` prop

**Files:**
- Modify: `web/components/settlement-card.tsx`

The compact mode renders the breakdown as a horizontal stat strip without the wrapping `<Card>` chrome — letting the dashboard place it as a thin band above the donut grid.

- [ ] **Step 1: Add the `compact` prop**

Edit `web/components/settlement-card.tsx`. Replace the entire file with:

```tsx
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { TrendingUp } from "lucide-react";
import { formatKRW } from "@/lib/utils";
import type { Settlement } from "@/lib/schemas";

type Props = {
  year: number;
  month: number;
  data: Settlement | null;
  compact?: boolean;
};

export function SettlementCard({ year, month, data, compact = false }: Props) {
  const isEmpty =
    !data || (parseFloat(data.recognized_expense) === 0 && parseFloat(data.deducted_amount) === 0);

  if (compact) {
    return (
      <div
        className="rounded-md border bg-card px-4 py-3 flex flex-wrap items-baseline gap-x-4 gap-y-1"
        data-testid="settlement-compact"
      >
        <span className="text-sm font-medium flex items-center gap-1.5">
          <TrendingUp className="h-4 w-4" />
          {year}년 {month}월 정산
        </span>
        {isEmpty ? (
          <span className="text-sm text-muted-foreground" data-testid="settlement-empty">
            데이터가 없습니다.
          </span>
        ) : (
          <span
            className="flex flex-wrap items-baseline gap-x-2 gap-y-1 tabular-nums"
            data-testid="settlement-summary"
          >
            <span className="text-xs text-muted-foreground">경비인정</span>
            <span className="font-semibold">{formatKRW(data!.recognized_expense)}</span>
            <span className="text-muted-foreground">−</span>
            <span className="text-xs text-muted-foreground">차감</span>
            <span className="font-semibold">{formatKRW(data!.deducted_amount)}</span>
            <span className="text-muted-foreground">=</span>
            <span className="text-xs text-muted-foreground">입금액</span>
            <span className="font-bold text-primary">{formatKRW(data!.settlement_input)}</span>
          </span>
        )}
      </div>
    );
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <TrendingUp className="h-4 w-4" />
          {year}년 {month}월 정산
        </CardTitle>
      </CardHeader>
      <CardContent>
        {isEmpty ? (
          <p className="text-muted-foreground text-sm" data-testid="settlement-empty">
            {year}년 {month}월 정산 데이터가 없습니다.
          </p>
        ) : (
          <div
            className="flex flex-wrap items-baseline gap-x-3 gap-y-1 tabular-nums"
            data-testid="settlement-summary"
          >
            <span className="text-sm text-muted-foreground">경비인정</span>
            <span className="text-lg font-semibold">
              {formatKRW(data!.recognized_expense)}
            </span>
            <span className="text-muted-foreground">−</span>
            <span className="text-sm text-muted-foreground">차감</span>
            <span className="text-lg font-semibold">
              {formatKRW(data!.deducted_amount)}
            </span>
            <span className="text-muted-foreground">=</span>
            <span className="text-sm text-muted-foreground">입금액</span>
            <span className="text-xl font-bold text-primary">
              {formatKRW(data!.settlement_input)}
            </span>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
```

- [ ] **Step 2: Verify the existing settlement tests still pass**

Run: `cd web && npx vitest run __tests__/dashboard.test.tsx -t "SettlementCard"`
Expected: PASS — the 3 existing settlement tests still pass (default compact=false preserves behavior).

- [ ] **Step 3: Commit**

```bash
git add web/components/settlement-card.tsx
git commit -m "feat(web): add compact mode to SettlementCard

Opt-in horizontal layout for the dashboard, leaving default behavior
unchanged for any other consumer.
"
```

---

## Task 4: DashboardDonuts grid + page wiring + tests

**Files:**
- Create: `web/components/dashboard-donuts.tsx`
- Modify: `web/app/(app)/page.tsx`
- Modify: `web/__tests__/dashboard.test.tsx`
- Delete: `web/components/summary-pivot.tsx`

- [ ] **Step 1: Update `dashboard.test.tsx` (write the new failing tests)**

Replace the entire content of `web/__tests__/dashboard.test.tsx` with:

```tsx
/**
 * Dashboard tests — vitest + @testing-library/react.
 *
 * After the donut redesign:
 *  - MonthPicker URL sync (unchanged)
 *  - SettlementCard: default and compact modes
 *  - ActorDonut: empty state + populated state (recharts mocked)
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { MonthPicker } from "../components/month-picker";
import { SettlementCard } from "../components/settlement-card";
import { ActorDonut } from "../components/actor-donut";
import { buildActorSlices } from "../lib/donut-data";
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

// recharts has no jsdom layout — replace with passthroughs so testids stay assertable.
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

// ── 1. MonthPicker URL sync ──────────────────────────────────────────────────

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

// ── 2. SettlementCard default + compact ──────────────────────────────────────

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

// ── 3. ActorDonut rendering ──────────────────────────────────────────────────

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
            { actor_id: ACTOR_A, actor_name: "공동", amount: "100000", sign: 1 },
          ],
          total: "100000",
        },
        {
          category_id: "22222222-2222-2222-2222-222222222222",
          category_name: "차감",
          kind: "expense",
          by_actor: [
            { actor_id: ACTOR_A, actor_name: "공동", amount: "7500", sign: 1 },
          ],
          total: "7500",
        },
      ],
    };
    render(<ActorDonut data={buildActorSlices(data, ACTOR_A)} />);

    expect(screen.getByText("공동")).toBeTruthy();
    // Center label total = 107,500
    expect(screen.getByText("₩107,500")).toBeTruthy();
    // Legend rows
    expect(screen.getByText("외식")).toBeTruthy();
    expect(screen.getByText("차감")).toBeTruthy();
  });
});
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run: `cd web && npx vitest run __tests__/dashboard.test.tsx`
Expected: FAIL — `ActorDonut` import resolves (created in Task 2), but the suite still imports `SummaryPivot`? No — verify the new file no longer references `SummaryPivot`. The expected failure: ActorDonut tests pass; everything else passes. If ActorDonut tests fail because the recharts mock doesn't cover something used by `actor-donut.tsx`, fix the mock here.

If all green already, that's fine — proceed.

- [ ] **Step 3: Implement `DashboardDonuts`**

Create `web/components/dashboard-donuts.tsx` with the full content:

```tsx
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart } from "lucide-react";
import { ActorDonut } from "./actor-donut";
import { buildActorSlices } from "@/lib/donut-data";
import type { SummaryResponse } from "@/lib/schemas";

type Props = {
  data: SummaryResponse | null;
};

export function DashboardDonuts({ data }: Props) {
  if (!data || data.categories.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <PieChart className="h-4 w-4" />
            카테고리 분포
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            이 달의 거래 내역이 없습니다.
          </p>
        </CardContent>
      </Card>
    );
  }

  // Collect every actor_id that appears in any category cell, preserving the
  // order from data.actors first, then any extras (e.g., null) at the end.
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

  const donuts = ordered
    .map((actorId) => buildActorSlices(data, actorId))
    .filter((d) => d.slices.length > 0);

  if (donuts.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <PieChart className="h-4 w-4" />
            카테고리 분포
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            이 달의 거래 내역이 없습니다.
          </p>
        </CardContent>
      </Card>
    );
  }

  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-4" data-testid="dashboard-donuts">
      {donuts.map((d) => (
        <ActorDonut key={d.actorId ?? "unset"} data={d} />
      ))}
    </div>
  );
}
```

- [ ] **Step 4: Update `page.tsx` to use the new section**

Replace the entire content of `web/app/(app)/page.tsx` with:

```tsx
import { Suspense } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { apiFetch } from "@/lib/api";
import {
  SettlementSchema,
  Settlement,
  SummaryResponseSchema,
  SummaryResponse,
} from "@/lib/schemas";
import { LayoutDashboard, Download } from "lucide-react";
import { MonthPicker } from "@/components/month-picker";
import { SettlementCard } from "@/components/settlement-card";
import { DashboardDonuts } from "@/components/dashboard-donuts";

// ── Helpers ──────────────────────────────────────────────────────────────────

function parseYM(input: string | undefined): { year: number; month: number } {
  if (input && /^\d{4}-\d{2}$/.test(input)) {
    const [y, m] = input.split("-").map(Number);
    if (m >= 1 && m <= 12) return { year: y, month: m };
  }
  const now = new Date();
  return { year: now.getFullYear(), month: now.getMonth() + 1 };
}

// ── Data fetchers (server-side) ──────────────────────────────────────────────

async function fetchSettlement(year: number, month: number): Promise<Settlement | null> {
  try {
    return await apiFetch<Settlement>(`/api/settlement/${year}/${month}`, {
      schema: SettlementSchema,
    });
  } catch {
    return null;
  }
}

async function fetchSummary(year: number, month: number): Promise<SummaryResponse | null> {
  try {
    return await apiFetch<SummaryResponse>(`/api/summary/${year}/${month}`, {
      schema: SummaryResponseSchema,
    });
  } catch {
    return null;
  }
}

// ── Sections ─────────────────────────────────────────────────────────────────

async function SettlementSection({ year, month }: { year: number; month: number }) {
  const data = await fetchSettlement(year, month);
  return <SettlementCard year={year} month={month} data={data} compact />;
}

async function DashboardDonutsSection({ year, month }: { year: number; month: number }) {
  const data = await fetchSummary(year, month);
  return <DashboardDonuts data={data} />;
}

// ── Page ─────────────────────────────────────────────────────────────────────

interface PageProps {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}

export default async function DashboardPage({ searchParams }: PageProps) {
  const params = await searchParams;
  const ymRaw = typeof params.ym === "string" ? params.ym : undefined;
  const { year, month } = parseYM(ymRaw);

  const sectionKey = `${year}-${month}`;

  return (
    <div className="max-w-5xl mx-auto space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-3">
          <LayoutDashboard className="h-6 w-6" />
          <h1 className="text-2xl font-bold">대시보드</h1>
        </div>
        <div className="flex items-center gap-2">
          <a
            href={`/api/export-proxy/${year}/${month}`}
            download
            className="inline-flex items-center gap-1.5 h-9 px-3 rounded-md border border-input bg-background text-sm font-medium hover:bg-accent hover:text-accent-foreground transition-colors"
            data-testid="export-download-link"
          >
            <Download className="h-4 w-4" />
            Excel 다운로드
          </a>
          <MonthPicker year={year} month={month} />
        </div>
      </div>

      <Suspense
        key={`settlement-${sectionKey}`}
        fallback={<StripSkeleton />}
      >
        <SettlementSection year={year} month={month} />
      </Suspense>

      <Suspense
        key={`donuts-${sectionKey}`}
        fallback={<DonutsSkeleton />}
      >
        <DashboardDonutsSection year={year} month={month} />
      </Suspense>
    </div>
  );
}

function StripSkeleton() {
  return (
    <div className="rounded-md border bg-card px-4 py-3 animate-pulse">
      <div className="h-4 bg-muted rounded w-1/3" />
    </div>
  );
}

function DonutsSkeleton() {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">카테고리 분포</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="animate-pulse space-y-2">
          <div className="h-4 bg-muted rounded w-3/4" />
          <div className="h-4 bg-muted rounded w-1/2" />
        </div>
      </CardContent>
    </Card>
  );
}
```

- [ ] **Step 5: Delete the unused pivot component**

Run: `cd web && grep -rn "summary-pivot\|SummaryPivot" --include="*.ts" --include="*.tsx" .`
Expected: only matches inside `web/components/summary-pivot.tsx` itself (no remaining importers).

If the grep is clean, delete:
```bash
rm web/components/summary-pivot.tsx
```

- [ ] **Step 6: Run the full frontend test suite**

Run: `cd web && npm test`
Expected: PASS — every test green. Test count goes from 86 down (the old pivot snapshot tests are gone) and up (donut helper + ActorDonut + compact-settlement tests).

- [ ] **Step 7: Run the dev server and eyeball the dashboard**

Run: `cd web && npm run dev`
Visit `http://localhost:3000/?ym=2026-02`. Expected:
- Compact settlement strip at the top with `584,000 − 7,500 = 576,500` (Feb 2026 data)
- 3-up grid below: 공동, 엉아, 아기 donuts
- 차감 visible as a gray slice in the actor that has the deduction row
- Legend shows percentages and amounts

Stop the dev server with Ctrl-C.

- [ ] **Step 8: Commit**

```bash
git add web/components/dashboard-donuts.tsx web/components/actor-donut.tsx web/app/\(app\)/page.tsx web/__tests__/dashboard.test.tsx
git rm web/components/summary-pivot.tsx
git commit -m "feat(web): replace dashboard pivot with donut grid

- DashboardDonuts renders one ActorDonut per actor with data.
- Page drops RecentTransactions; settlement card switches to compact strip.
- Pivot component and its snapshot test removed.
"
```

---

## Task 5: Documentation update

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Append a 2026-05-07 entry to the Cumulative Context section**

Open `CLAUDE.md`. Find the `## Cumulative Context (Documentation Agent)` section and append, right after the most recent dated entry:

```markdown
- 2026-05-07: Dashboard donut redesign — `(app)/page.tsx` now renders a compact `SettlementCard` strip + `DashboardDonuts` grid (one `ActorDonut` per actor, top-6 categories + 기타 + 차감 pinned). New: `web/lib/donut-data.ts` (pure slice builder), `web/components/{actor-donut,dashboard-donuts}.tsx`. Removed: `summary-pivot.tsx`, recent-transactions section. Frontend tests: pivot/recent suites replaced by `donut-data.test.ts` (9) and donut/compact-settlement assertions in `dashboard.test.tsx`. No backend changes.
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: log dashboard donut redesign in cumulative context"
```

---

## Acceptance Criteria

After Task 5:

1. `cd web && npm test` — all tests pass.
2. `cd web && npx tsc --noEmit` — no type errors.
3. Visiting `/?ym=2026-02` shows: compact settlement strip; 3 donut cards (공동, 엉아, 아기); each donut has center total, side legend, 차감 in gray when present; no recent-transactions section.
4. Visiting `/?ym=2099-01` (a month with no data) shows: compact settlement empty state; single empty-state card where the donut grid would be.
5. `git log --oneline` shows 4 commits from this plan in order: helper, actor-donut, settlement compact, dashboard wiring + cleanup, doc update.
