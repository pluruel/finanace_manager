import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart } from "lucide-react";
import { ActorDonut } from "./actor-donut";
import { buildActorSlices, collectOrderedActorIds } from "@/lib/donut-data";
import type { SummaryResponse } from "@/lib/schemas";

type Props = {
  data: SummaryResponse | null;
};

function EmptyDonutsCard() {
  return (
    <Card data-testid="dashboard-donuts-empty">
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

export function DashboardDonuts({ data }: Props) {
  const ordered = collectOrderedActorIds(data);
  const knownActorIds = new Set<string | null>(
    data?.actors.map((a) => a.actor_id) ?? [],
  );
  const allDonuts = data ? ordered.map((actorId) => buildActorSlices(data, actorId)) : [];
  // Keep cards for any actor declared in data.actors (even if empty, to preserve
  // the stable 3-column grid). Drop only stray actor_ids that came in via
  // by_actor cells but produced no slices.
  const donuts = allDonuts.filter(
    (d) => knownActorIds.has(d.actorId) || d.slices.length > 0,
  );

  if (donuts.length === 0) return <EmptyDonutsCard />;

  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-4" data-testid="dashboard-donuts">
      {donuts.map((d) => (
        <ActorDonut key={d.actorId ?? "unset"} data={d} />
      ))}
    </div>
  );
}
