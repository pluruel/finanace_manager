"use client";

import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { PriceHistoryResponse } from "@/lib/schemas";

function formatKRW(n: number): string {
  return `₩${n.toLocaleString("ko-KR")}`;
}

export function PriceHistoryChart({ data }: { data: PriceHistoryResponse }) {
  const series = data.points.map((p) => ({
    occurred_on: p.occurred_on,
    unit_price: Number(p.unit_price),
    merchant: p.merchant_name ?? "",
    memo: p.memo ?? "",
  }));

  if (series.length === 0) {
    return (
      <p
        data-testid="price-history-empty"
        className="text-sm text-muted-foreground p-4"
      >
        선택한 상품에 단가 시계열이 없습니다.
      </p>
    );
  }

  return (
    <div data-testid="price-history-chart" className="w-full h-72">
      <ResponsiveContainer width="100%" height="100%">
        <LineChart data={series} margin={{ top: 16, right: 24, left: 8, bottom: 8 }}>
          <CartesianGrid strokeDasharray="3 3" className="stroke-muted" />
          <XAxis dataKey="occurred_on" tick={{ fontSize: 12 }} />
          <YAxis
            tick={{ fontSize: 12 }}
            tickFormatter={(v: number) => formatKRW(v)}
            width={80}
          />
          <Tooltip
            formatter={(v: number) => [formatKRW(v), "단가"]}
            labelFormatter={(label) => label}
          />
          <Line
            type="monotone"
            dataKey="unit_price"
            stroke="#2563eb"
            strokeWidth={2}
            dot={{ r: 3 }}
            activeDot={{ r: 5 }}
          />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
}
