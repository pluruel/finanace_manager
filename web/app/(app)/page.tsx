import { Suspense } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { apiFetch } from "@/lib/api";
import {
  SettlementSchema,
  Settlement,
  SummaryResponseSchema,
  SummaryResponse,
} from "@/lib/schemas";
import { LayoutDashboard, Download } from "lucide-react";
import { MonthPicker } from "@/components/month-picker";
import { SettlementCard } from "@/components/settlement-card";
import { DashboardDonuts } from "@/components/dashboard-donuts";

function parseYM(input: string | undefined): { year: number; month: number } {
  if (input && /^\d{4}-\d{2}$/.test(input)) {
    const [y, m] = input.split("-").map(Number);
    if (m >= 1 && m <= 12) return { year: y, month: m };
  }
  const now = new Date();
  return { year: now.getFullYear(), month: now.getMonth() + 1 };
}

async function fetchSettlement(year: number, month: number): Promise<Settlement | null> {
  try {
    return await apiFetch<Settlement>(`/api/settlement/${year}/${month}`, {
      schema: SettlementSchema,
    });
  } catch {
    return null;
  }
}

async function fetchSummary(year: number, month: number): Promise<SummaryResponse | null> {
  try {
    return await apiFetch<SummaryResponse>(`/api/summary/${year}/${month}`, {
      schema: SummaryResponseSchema,
    });
  } catch {
    return null;
  }
}

async function SettlementSection({ year, month }: { year: number; month: number }) {
  const data = await fetchSettlement(year, month);
  return <SettlementCard year={year} month={month} data={data} compact />;
}

async function DashboardDonutsSection({ year, month }: { year: number; month: number }) {
  const data = await fetchSummary(year, month);
  return <DashboardDonuts data={data} />;
}

interface PageProps {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}

export default async function DashboardPage({ searchParams }: PageProps) {
  const params = await searchParams;
  const ymRaw = typeof params.ym === "string" ? params.ym : undefined;
  const { year, month } = parseYM(ymRaw);

  const sectionKey = `${year}-${month}`;

  return (
    <div className="max-w-5xl mx-auto space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-3">
          <LayoutDashboard className="h-6 w-6" />
          <h1 className="text-2xl font-bold">대시보드</h1>
        </div>
        <div className="flex items-center gap-2">
          <a
            href={`/api/export-proxy/${year}/${month}`}
            download
            className="inline-flex items-center gap-1.5 h-9 px-3 rounded-md border border-input bg-background text-sm font-medium hover:bg-accent hover:text-accent-foreground transition-colors"
            data-testid="export-download-link"
          >
            <Download className="h-4 w-4" />
            Excel 다운로드
          </a>
          <MonthPicker year={year} month={month} />
        </div>
      </div>

      <Suspense
        key={`settlement-${sectionKey}`}
        fallback={<StripSkeleton />}
      >
        <SettlementSection year={year} month={month} />
      </Suspense>

      <Suspense
        key={`donuts-${sectionKey}`}
        fallback={<DonutsSkeleton />}
      >
        <DashboardDonutsSection year={year} month={month} />
      </Suspense>
    </div>
  );
}

function StripSkeleton() {
  return (
    <div className="rounded-md border bg-card px-4 py-3 animate-pulse">
      <div className="h-4 bg-muted rounded w-1/3" />
    </div>
  );
}

function DonutsSkeleton() {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">카테고리 분포</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="animate-pulse space-y-2">
          <div className="h-4 bg-muted rounded w-3/4" />
          <div className="h-4 bg-muted rounded w-1/2" />
        </div>
      </CardContent>
    </Card>
  );
}
