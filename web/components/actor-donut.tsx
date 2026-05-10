"use client";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from "recharts";
import type { ActorDonutData, DonutSlice } from "@/lib/donut-data";

type Props = {
  actorName: string;
  expense: ActorDonutData;
  income: ActorDonutData;
};

function fmtSigned(v: number): string {
  const abs = Math.abs(v).toLocaleString("ko-KR");
  return v < 0 ? `-₩${abs}` : `₩${abs}`;
}

function DonutChart({
  slices,
  chartTestId,
  centerTestId,
  centerLabel,
  centerValue,
}: {
  slices: DonutSlice[];
  chartTestId: string;
  centerTestId: string;
  centerLabel: string;
  centerValue: number;
}) {
  return (
    <div className="relative h-44" data-testid={chartTestId}>
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
        data-testid={centerTestId}
        className="absolute inset-0 pointer-events-none flex flex-col items-center justify-center"
      >
        <span className="text-xs text-muted-foreground">{centerLabel}</span>
        <span className="text-base font-semibold tabular-nums">
          {fmtSigned(centerValue)}
        </span>
      </div>
    </div>
  );
}

export function ActorDonut({ actorName, expense, income }: Props) {
  const expenseDenom = expense.slices.reduce((acc, s) => acc + Math.abs(s.value), 0);
  const hasIncome = income.slices.length > 0;
  const hasExpense = expense.slices.length > 0;
  const hasNothing = !hasIncome && !hasExpense;

  return (
    <Card data-testid={`actor-donut-${actorName}`}>
      <CardHeader>
        <CardTitle className="text-base">{actorName}</CardTitle>
      </CardHeader>
      <CardContent>
        {hasNothing ? (
          <p className="text-sm text-muted-foreground" data-testid="donut-empty">
            이 달의 거래 내역이 없습니다.
          </p>
        ) : (
          <div className="flex flex-col gap-3" data-testid="donut-stack">
            {hasIncome && (
              <DonutChart
                slices={income.slices}
                chartTestId="donut-income-chart"
                centerTestId="donut-income-center"
                centerLabel="수입"
                centerValue={income.total}
              />
            )}

            {hasExpense ? (
              <>
                <DonutChart
                  slices={expense.slices}
                  chartTestId="donut-expense-chart"
                  centerTestId="donut-center"
                  centerLabel="지출"
                  centerValue={expense.total}
                />
                <ul className="text-sm space-y-1">
                  {expense.slices.map((s: DonutSlice, i) => (
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
                        {fmtSigned(s.value)} · {expenseDenom === 0 ? "0%" : `${((Math.abs(s.value) / expenseDenom) * 100).toFixed(1)}%`}
                      </span>
                    </li>
                  ))}
                </ul>
              </>
            ) : (
              <p
                data-testid="donut-no-expense"
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
