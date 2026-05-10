import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart } from "lucide-react";
import { ActorDonut } from "./actor-donut";
import {
  buildActorSlices,
  buildHouseholdSlices,
  buildActorIncomeSlices,
  buildHouseholdIncomeSlices,
} from "@/lib/donut-data";
import type { ActorDonutData } from "@/lib/donut-data";
import type { SummaryResponse, IncomeResponse } from "@/lib/schemas";

type Props = {
  summary: SummaryResponse | null;
  income: IncomeResponse | null;
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

const PERSON_NAMES = ["아기", "엉아"] as const;

const EMPTY_DONUT: ActorDonutData = {
  actorId: null,
  actorName: "",
  total: 0,
  slices: [],
};

export function DashboardDonuts({ summary, income }: Props) {
  if (!summary) return <EmptyDonutsCard />;

  const householdExpense = buildHouseholdSlices(summary);
  const householdIncome = buildHouseholdIncomeSlices(income);

  const personCards = PERSON_NAMES.map((name) => {
    const actor = summary.actors.find((a) => a.actor_name === name);
    const expense = actor ? buildActorSlices(summary, actor.actor_id) : EMPTY_DONUT;
    const incomeData = actor ? buildActorIncomeSlices(income, actor.actor_id) : EMPTY_DONUT;
    return { name, expense, income: incomeData };
  });

  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-4" data-testid="dashboard-donuts">
      <ActorDonut
        actorName="가구 합계"
        expense={householdExpense}
        income={householdIncome}
      />
      {personCards.map((pc) => (
        <ActorDonut
          key={pc.name}
          actorName={pc.name}
          expense={pc.expense}
          income={pc.income}
        />
      ))}
    </div>
  );
}
