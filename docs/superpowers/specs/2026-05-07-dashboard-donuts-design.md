# Dashboard Redesign: Round Charts (Donut Grid)

**Status:** Approved 2026-05-07
**Owner:** Junnoh Lee
**Scope:** `web/` only — no backend changes

## Motivation

The current dashboard at `web/app/(app)/page.tsx` is dominated by the dense `SummaryPivot` table (category × actor) and a 10-row recent-transactions list. Both are tabular and hard to scan at a glance. The user wants a chart-first dashboard where spending patterns are visible in a single look.

## Goals

- Replace the category × actor pivot **table** with a **multi-donut grid** — one donut per actor showing the actor's category breakdown.
- Keep the settlement card (slimmed to inline stats).
- Drop recent-transactions from the dashboard. The existing `/transactions` page is the canonical place.

## Non-goals

- No backend changes. Reuses `GET /api/summary/:year/:month`.
- No new charting library. Reuses `recharts`, already used by `/price-history`.
- No multi-month comparison (deferred, consistent with M3/M4).

## Layout

Top → bottom on a single page:

1. **Header row** (unchanged): "대시보드" title · `MonthPicker` · "Excel 다운로드" link
2. **Settlement card** (slimmed): the existing settlement numbers rendered as inline stat tiles instead of the current taller card
3. **Donut grid**: 3 cards side-by-side on desktop (1-col on mobile), one per actor — `공동`, `엉아`, `아기`. A 4th card is appended only if the legacy `null` actor has data.

## Per-donut spec

- **Slices**: top 6 categories by absolute amount + a single `기타` slice aggregating the rest.
- **차감**: always rendered as its own slice in a distinct gray, regardless of its amount rank. It is settlement-relevant and must remain visible.
- **Center label**: actor name (large) + total signed amount (small).
- **Legend**: right-side list of `slice → ₩amount · %`, sorted by amount desc, with 차감 pinned last.
- **Empty actor** (no transactions in that month for that actor): render the card with the placeholder "이 달의 거래 내역이 없습니다" — do not omit, so the 3-column grid layout stays stable.

## Slice-grouping rule (precise)

Given a `SummaryResponse` and a target `actor_id`:

1. For each `cat` in `data.categories`, compute `signed = signedNumber(cat.by_actor[actor_id].amount, sign)` (reuse existing helper).
2. Partition into:
   - `deduction`: rows where `cat.category_name === "차감"`
   - `rest`: everything else
3. Sort `rest` by `Math.abs(signed)` desc.
4. Take `top = rest.slice(0, 6)`. If `rest.length > 6`, sum the remainder into a single `기타` slice.
5. Final slice list = `[...top, 기타?, ...deduction]` (deduction pinned last).
6. Total = sum of every original `signed` value (including 차감 and the rest), so the center label always matches the actor's column total in the old pivot.

## Components

### New

- **`web/lib/donut-data.ts`** — pure aggregator. Exports `buildActorSlices(data: SummaryResponse, actorId: string | null): { actorName: string; total: number; slices: { name: string; value: number; isDeduction: boolean; isOther: boolean; color: string }[] }`. Color assignment uses a fixed palette indexed by sorted rank; 차감 is always the gray; 기타 is always neutral.
- **`web/components/actor-donut.tsx`** — presentational. Props: `{ actorName: string; total: number; slices: Slice[] }`. Renders a `recharts` `PieChart` with `innerRadius` for donut, center label, and side legend. No data fetching.
- **`web/components/dashboard-donuts.tsx`** — server component (mirrors `SummarySection`). Fetches summary, calls `buildActorSlices` per actor, renders a responsive grid of `ActorDonut` cards.

### Modified

- **`web/app/(app)/page.tsx`** — drop `RecentTransactions` and its `<Card>` wrapper; replace `SummarySection` with `DashboardDonutsSection`. Pass a `compact` prop (or replace usage) so `SettlementCard` renders as inline stat tiles.
- **`web/components/settlement-card.tsx`** — accept a `compact?: boolean` prop that switches to a horizontal stat-tile layout. Default behavior unchanged so any other consumer keeps working.

### Deleted

- **`web/components/summary-pivot.tsx`** — no remaining consumers after `page.tsx` is updated. Verify with `grep` before deleting.

## Data flow

```
page.tsx
  └─ DashboardDonutsSection (server, fetches /api/summary/:y/:m)
       └─ DashboardDonuts (renders grid)
            └─ ActorDonut × N (one per actor with data)
                 ↑ slices from buildActorSlices(data, actorId)
```

No new endpoints, no schema changes.

## Tests

### New

- **`web/__tests__/donut-data.test.ts`** — unit tests for `buildActorSlices`:
  - Top-6 + 기타 grouping when there are >6 non-deduction categories
  - 차감 is always present as its own slice when the actor has any 차감 row
  - 차감 never counts toward the top-6 ranking
  - Empty actor returns `slices: []` and `total: 0`
  - Total equals sum across all original slices (including 차감 and 기타)

### Modified

- **`web/__tests__/dashboard.test.tsx`** — remove pivot-table and recent-tx assertions; add:
  - 3 donut cards rendered for a 3-actor month
  - Each card shows the actor name and a total
  - Recharts is mocked the same way `price-history.test.tsx` mocks it

## Edge cases

| Case | Behavior |
|------|----------|
| Month with zero transactions | Each donut card shows existing empty-state copy; settlement card unchanged from current behavior |
| Actor with only 차감 rows | Single gray slice + total |
| Actor with only one non-차감 category | One colored slice; no 기타 |
| `null` actor_id present | Render as 4th card only if it has rows; otherwise omit |
| Negative-only categories (refunds) | `Math.abs` sort handles ranking; sign preserved in display |

## Risks / open questions

- Recharts pie label rendering can clash with small slices. Mitigation: legend on the side rather than slice labels.
- The slimmed settlement card must not regress visually for users who already rely on the current layout. Mitigation: the `compact` prop is opt-in; the dashboard is the only known consumer.

## Out of scope (explicit)

- Multi-month overlay charts (still deferred per M3/M4).
- Replacing `/transactions` or `/price-history` visuals.
- Backend aggregation changes (top-N could move server-side later, but not now).
