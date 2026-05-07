"use client";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from "recharts";
import type { ActorDonutData, DonutSlice } from "@/lib/donut-data";
import { INCOME_COLOR } from "@/lib/donut-data";

type Props = {
  data: ActorDonutData;
  income: number;
};

function fmtSigned(v: number): string {
  const abs = Math.abs(v).toLocaleString("ko-KR");
  return v < 0 ? `-₩${abs}` : `₩${abs}`;
}

function pctOfAbs(value: number, slices: DonutSlice[]): string {
  const denom = slices.reduce((acc, s) => acc + Math.abs(s.value), 0);
  if (denom === 0) return "0%";
  return `${((Math.abs(value) / denom) * 100).toFixed(1)}%`;
}

export function ActorDonut({ data, income }: Props) {
  const { actorName, total, slices } = data;
  const hasIncome = income > 0;
  const hasSlices = slices.length > 0;
  const hasNothing = !hasIncome && !hasSlices;

  return (
    <Card data-testid={`actor-donut-${actorName}`}>
      <CardHeader>
        <CardTitle className="text-base">{actorName}</CardTitle>
      </CardHeader>
      <CardContent>
        {hasNothing ? (
          <p className="text-sm text-muted-foreground" data-testid="actor-donut-empty">
            이 달의 거래 내역이 없습니다.
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            {hasIncome && (
              <div
                data-testid="actor-donut-income"
                className="flex items-center justify-between text-sm"
              >
                <span className="font-medium" style={{ color: INCOME_COLOR }}>
                  수입
                </span>
                <span
                  className="font-mono font-semibold tabular-nums"
                  style={{ color: INCOME_COLOR }}
                >
                  {fmtSigned(income)}
                </span>
              </div>
            )}

            {hasSlices ? (
              <>
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
                        formatter={(v: number) => `₩${v.toLocaleString("ko-KR")}`}
                        contentStyle={{ fontSize: 12 }}
                      />
                    </PieChart>
                  </ResponsiveContainer>
                  <div
                    data-testid="actor-donut-center"
                    className="absolute inset-0 pointer-events-none flex flex-col items-center justify-center"
                  >
                    <span className="text-xs text-muted-foreground">지출</span>
                    <span className="text-base font-semibold tabular-nums">
                      {fmtSigned(total)}
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
                          aria-hidden="true"
                          className="inline-block h-2.5 w-2.5 rounded-sm shrink-0"
                          style={{ backgroundColor: s.color }}
                        />
                        <span className="truncate">{s.name}</span>
                      </span>
                      <span className="tabular-nums text-muted-foreground shrink-0">
                        {fmtSigned(s.value)} · {pctOfAbs(s.value, slices)}
                      </span>
                    </li>
                  ))}
                </ul>
              </>
            ) : (
              <p
                data-testid="actor-donut-no-expense"
                className="text-sm text-muted-foreground"
              >
                이 달 지출 없음.
              </p>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
