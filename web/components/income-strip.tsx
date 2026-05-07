import type { IncomeResponse } from "@/lib/schemas";
import { Card, CardContent } from "@/components/ui/card";

interface Props {
  data: IncomeResponse | null;
}

function formatKRW(amount: string): string {
  const v = parseFloat(amount);
  if (Number.isNaN(v)) return "₩0";
  return `₩${Math.round(v).toLocaleString()}`;
}

export function IncomeStrip({ data }: Props) {
  if (!data) return null;

  return (
    <Card>
      <CardContent className="py-3">
        <div className="flex items-center gap-6 flex-wrap">
          <span className="text-sm font-medium text-muted-foreground">월 수입</span>
          <div className="flex items-center gap-4 flex-wrap">
            {data.by_actor.map((row) => (
              <div
                key={row.actor_id ?? row.actor_name}
                className="flex items-center gap-1.5"
              >
                <span className="text-sm text-muted-foreground">{row.actor_name}</span>
                <span className="text-sm font-mono">{formatKRW(row.total)}</span>
              </div>
            ))}
          </div>
          <div className="ml-auto text-sm font-mono font-semibold">
            합계 {formatKRW(data.total)}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
