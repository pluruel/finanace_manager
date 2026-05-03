"use client";

import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { MerchantStatsResponse } from "@/lib/schemas";

function formatKRW(n: number): string {
  return `₩${n.toLocaleString("ko-KR")}`;
}

export function MerchantStatsChart({ data }: { data: MerchantStatsResponse }) {
  const series = data.points.map((p) => ({
    month: p.month.slice(0, 7),
    total: Number(p.total),
    transaction_count: p.transaction_count,
    memo_less_count: p.memo_less_count,
  }));

  if (series.length === 0) {
    return (
      <p
        data-testid="merchant-stats-empty"
        className="text-sm text-muted-foreground p-4"
      >
        선택한 구매처의 월별 집계가 없습니다.
      </p>
    );
  }

  return (
    <div data-testid="merchant-stats-chart" className="w-full h-72">
      <ResponsiveContainer width="100%" height="100%">
        <BarChart data={series} margin={{ top: 16, right: 24, left: 8, bottom: 8 }}>
          <CartesianGrid strokeDasharray="3 3" className="stroke-muted" />
          <XAxis dataKey="month" tick={{ fontSize: 12 }} />
          <YAxis
            tick={{ fontSize: 12 }}
            tickFormatter={(v: number) => formatKRW(v)}
            width={80}
          />
          <Tooltip
            formatter={(v: number, name: string) => {
              if (name === "total") return [formatKRW(v), "월 합계"];
              return [v, name];
            }}
          />
          <Legend />
          <Bar dataKey="total" name="월 합계" fill="#2563eb" />
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
}
