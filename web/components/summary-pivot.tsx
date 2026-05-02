import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Table2 } from "lucide-react";
import { cn, formatKRW } from "@/lib/utils";
import type { SummaryResponse } from "@/lib/schemas";

type Props = {
  data: SummaryResponse | null;
};

function signedNumber(amount: string, sign: number): number {
  const v = parseFloat(amount);
  if (isNaN(v)) return 0;
  return sign === -1 ? -v : v;
}

function formatSigned(value: number): string {
  if (value === 0) return "-";
  if (value < 0) return `-${formatKRW(String(Math.abs(value)))}`;
  return formatKRW(String(value));
}

export function SummaryPivot({ data }: Props) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <Table2 className="h-4 w-4" />
          카테고리 × 액터 집계
        </CardTitle>
      </CardHeader>
      <CardContent className="p-0">
        {!data || data.categories.length === 0 ? (
          <p className="text-sm text-muted-foreground p-6">
            이 달의 거래 내역이 없습니다.
          </p>
        ) : (
          <PivotTable data={data} />
        )}
      </CardContent>
    </Card>
  );
}

function PivotTable({ data }: { data: SummaryResponse }) {
  const actors = data.actors;

  // Build a lookup: category_id → (actor_id|null → signed number)
  const cellLookup = new Map<string, Map<string | null, number>>();
  for (const cat of data.categories) {
    const m = new Map<string | null, number>();
    for (const entry of cat.by_actor) {
      m.set(entry.actor_id ?? null, signedNumber(entry.amount, entry.sign));
    }
    cellLookup.set(cat.category_id, m);
  }

  // Per-actor total across categories.
  const actorTotals = new Map<string | null, number>();
  for (const a of actors) actorTotals.set(a.actor_id, 0);

  for (const cat of data.categories) {
    const cells = cellLookup.get(cat.category_id)!;
    for (const a of actors) {
      const v = cells.get(a.actor_id) ?? 0;
      actorTotals.set(a.actor_id, (actorTotals.get(a.actor_id) ?? 0) + v);
    }
  }

  const grandTotal = Array.from(actorTotals.values()).reduce((a, b) => a + b, 0);

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="bg-muted/50 border-b">
            <th className="px-3 py-2 text-left text-xs font-medium text-muted-foreground">
              카테고리
            </th>
            {actors.map((a) => (
              <th
                key={a.actor_id ?? "unset"}
                className="px-3 py-2 text-right text-xs font-medium text-muted-foreground"
              >
                {a.actor_name}
              </th>
            ))}
            <th className="px-3 py-2 text-right text-xs font-medium text-muted-foreground">
              합계
            </th>
          </tr>
        </thead>
        <tbody>
          {data.categories.map((cat) => {
            const cells = cellLookup.get(cat.category_id)!;
            const isDeduction = cat.category_name === "차감";
            const rowTotal = signedNumber(cat.total, 1);
            return (
              <tr
                key={cat.category_id}
                className={cn(
                  "border-b hover:bg-muted/30",
                  isDeduction && "bg-muted/40 text-muted-foreground",
                )}
              >
                <td className="px-3 py-2">
                  <div className="flex items-center gap-1.5">
                    <span>{cat.category_name}</span>
                    {isDeduction && (
                      <Badge variant="muted" className="text-xs px-1.5 py-0">
                        정산 차감
                      </Badge>
                    )}
                  </div>
                </td>
                {actors.map((a) => {
                  const v = cells.get(a.actor_id) ?? 0;
                  return (
                    <td
                      key={a.actor_id ?? "unset"}
                      className={cn(
                        "px-3 py-2 text-right tabular-nums",
                        v < 0 && "text-blue-600",
                      )}
                    >
                      {formatSigned(v)}
                    </td>
                  );
                })}
                <td className="px-3 py-2 text-right font-medium tabular-nums">
                  {formatSigned(rowTotal)}
                </td>
              </tr>
            );
          })}
        </tbody>
        <tfoot>
          <tr className="bg-muted/60 border-t font-medium">
            <td className="px-3 py-2">합계</td>
            {actors.map((a) => {
              const v = actorTotals.get(a.actor_id) ?? 0;
              return (
                <td
                  key={a.actor_id ?? "unset"}
                  className={cn(
                    "px-3 py-2 text-right tabular-nums",
                    v < 0 && "text-blue-600",
                  )}
                >
                  {formatSigned(v)}
                </td>
              );
            })}
            <td className="px-3 py-2 text-right tabular-nums">
              {formatSigned(grandTotal)}
            </td>
          </tr>
        </tfoot>
      </table>
    </div>
  );
}
