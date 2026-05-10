"use client";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from "recharts";
import { buildDeductionByActor } from "@/lib/donut-data";
import type { SummaryResponse } from "@/lib/schemas";

type Props = {
  summary: SummaryResponse | null;
};

function fmt(v: number): string {
  return `₩${Math.abs(v).toLocaleString("ko-KR")}`;
}

export function DeductionDonut({ summary }: Props) {
  if (!summary) return null;
  const { slices, total } = buildDeductionByActor(summary);
  if (slices.length === 0) return null;

  const denom = slices.reduce((acc, s) => acc + Math.abs(s.value), 0);

  return (
    <Card data-testid="deduction-donut">
      <CardHeader>
        <CardTitle className="text-base">차감</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4 items-center">
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
                  {slices.map((s) => (
                    <Cell key={s.name} fill={s.color} />
                  ))}
                </Pie>
                <Tooltip
                  formatter={(v: number) => fmt(v)}
                  contentStyle={{ fontSize: 12 }}
                />
              </PieChart>
            </ResponsiveContainer>
            <div
              data-testid="deduction-donut-center"
              className="absolute inset-0 pointer-events-none flex flex-col items-center justify-center"
            >
              <span className="text-xs text-muted-foreground">차감 합계</span>
              <span className="text-base font-semibold tabular-nums">{fmt(total)}</span>
            </div>
          </div>
          <ul className="text-sm space-y-1">
            {slices.map((s, i) => (
              <li key={`${s.name}-${i}`} className="flex items-center justify-between gap-2">
                <span className="flex items-center gap-2 truncate">
                  <span
                    aria-hidden="true"
                    className="inline-block h-2.5 w-2.5 rounded-sm shrink-0"
                    style={{ backgroundColor: s.color }}
                  />
                  <span className="truncate">{s.name}</span>
                </span>
                <span className="tabular-nums text-muted-foreground shrink-0">
                  {fmt(s.value)}
                  {denom > 0 && ` · ${((Math.abs(s.value) / denom) * 100).toFixed(1)}%`}
                </span>
              </li>
            ))}
          </ul>
        </div>
      </CardContent>
    </Card>
  );
}
