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
