import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart } from "lucide-react";
import { ActorDonut } from "./actor-donut";
import { buildActorSlices } from "@/lib/donut-data";
import type { SummaryResponse } from "@/lib/schemas";

type Props = {
  data: SummaryResponse | null;
};

export function DashboardDonuts({ data }: Props) {
  if (!data || data.categories.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <PieChart className="h-4 w-4" />
            카테고리 분포
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            이 달의 거래 내역이 없습니다.
          </p>
        </CardContent>
      </Card>
    );
  }

  const seen = new Set<string | null>();
  const ordered: Array<string | null> = [];
  for (const a of data.actors) {
    if (!seen.has(a.actor_id)) {
      seen.add(a.actor_id);
      ordered.push(a.actor_id);
    }
  }
  for (const cat of data.categories) {
    for (const cell of cat.by_actor) {
      if (!seen.has(cell.actor_id)) {
        seen.add(cell.actor_id);
        ordered.push(cell.actor_id);
      }
    }
  }

  const donuts = ordered
    .map((actorId) => buildActorSlices(data, actorId))
    .filter((d) => d.slices.length > 0);

  if (donuts.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <PieChart className="h-4 w-4" />
            카테고리 분포
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            이 달의 거래 내역이 없습니다.
          </p>
        </CardContent>
      </Card>
    );
  }

  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-4" data-testid="dashboard-donuts">
      {donuts.map((d) => (
        <ActorDonut key={d.actorId ?? "unset"} data={d} />
      ))}
    </div>
  );
}
