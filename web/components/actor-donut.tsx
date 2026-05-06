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
