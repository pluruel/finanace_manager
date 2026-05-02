import { Suspense } from "react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { apiFetch, ApiError } from "@/lib/api";
import {
  TransactionsResponseSchema,
  TransactionsResponse,
  SettlementSchema,
  Settlement,
  SummaryResponseSchema,
  SummaryResponse,
} from "@/lib/schemas";
import { formatAmount, formatDate } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { LayoutDashboard, AlertCircle } from "lucide-react";
import { MonthPicker } from "@/components/month-picker";
import { SettlementCard } from "@/components/settlement-card";
import { SummaryPivot } from "@/components/summary-pivot";

// ── Helpers ──────────────────────────────────────────────────────────────────

function parseYM(input: string | undefined): { year: number; month: number } {
  if (input && /^\d{4}-\d{2}$/.test(input)) {
    const [y, m] = input.split("-").map(Number);
    if (m >= 1 && m <= 12) return { year: y, month: m };
  }
  const now = new Date();
  return { year: now.getFullYear(), month: now.getMonth() + 1 };
}

function monthRange(year: number, month: number): { from: string; to: string } {
  const last = new Date(Date.UTC(year, month, 0)).getUTCDate();
  const pad = (n: number) => String(n).padStart(2, "0");
  return {
    from: `${year}-${pad(month)}-01`,
    to: `${year}-${pad(month)}-${pad(last)}`,
  };
}

// ── Data fetchers (server-side) ──────────────────────────────────────────────

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

// ── Sections ─────────────────────────────────────────────────────────────────

async function SettlementSection({ year, month }: { year: number; month: number }) {
  const data = await fetchSettlement(year, month);
  return <SettlementCard year={year} month={month} data={data} />;
}

async function SummarySection({ year, month }: { year: number; month: number }) {
  const data = await fetchSummary(year, month);
  return <SummaryPivot data={data} />;
}

async function RecentTransactions({ year, month }: { year: number; month: number }) {
  const { from, to } = monthRange(year, month);
  const qs = new URLSearchParams({ from, to }).toString();

  let data: TransactionsResponse | null = null;
  let error: string | null = null;

  try {
    data = await apiFetch<TransactionsResponse>(`/api/transactions?${qs}`, {
      schema: TransactionsResponseSchema,
    });
  } catch (err) {
    if (err instanceof ApiError && err.status === 401) {
      error = "인증이 필요합니다.";
    } else {
      error = "데이터를 불러오지 못했습니다.";
    }
  }

  if (error) {
    return (
      <div className="flex items-center gap-2 text-muted-foreground text-sm p-4">
        <AlertCircle className="h-4 w-4" />
        <span>{error}</span>
      </div>
    );
  }

  if (!data || data.items.length === 0) {
    return (
      <p className="text-muted-foreground text-sm p-4">
        이 달의 거래 내역이 없습니다.
      </p>
    );
  }

  const recent = data.items.slice(0, 10);

  return (
    <div className="divide-y">
      {recent.map((item) => {
        const isDeduction = item.category_name === "차감";
        const isMultiLine = item.children.length > 0;

        return (
          <div
            key={item.id}
            className={`flex items-center justify-between py-3 px-1 ${
              isDeduction ? "bg-muted/50" : ""
            }`}
          >
            <div className="flex flex-col gap-0.5">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium">
                  {item.merchant_name ?? "미상"}
                </span>
                {isDeduction && (
                  <Badge variant="muted" className="text-xs">
                    정산 차감
                  </Badge>
                )}
                {isMultiLine && (
                  <Badge variant="outline" className="text-xs">
                    {item.children.length + 1}개 항목
                  </Badge>
                )}
              </div>
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                <span>{formatDate(item.occurred_on)}</span>
                {item.category_name && <span>· {item.category_name}</span>}
                {item.actor_name && <span>· {item.actor_name}</span>}
              </div>
            </div>
            <span
              className={`text-sm font-semibold ${
                item.sign === -1 ? "text-blue-600" : "text-foreground"
              }`}
            >
              {formatAmount(item.amount, item.sign)}
            </span>
          </div>
        );
      })}
    </div>
  );
}

// ── Page ─────────────────────────────────────────────────────────────────────

interface PageProps {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}

export default async function DashboardPage({ searchParams }: PageProps) {
  const params = await searchParams;
  const ymRaw = typeof params.ym === "string" ? params.ym : undefined;
  const { year, month } = parseYM(ymRaw);

  // Per-section keys force <Suspense> to re-fire when the month changes.
  const sectionKey = `${year}-${month}`;

  return (
    <div className="max-w-5xl mx-auto space-y-6">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-3">
          <LayoutDashboard className="h-6 w-6" />
          <h1 className="text-2xl font-bold">대시보드</h1>
        </div>
        <MonthPicker year={year} month={month} />
      </div>

      <Suspense
        key={`settlement-${sectionKey}`}
        fallback={<CardSkeleton title={`${year}년 ${month}월 정산`} />}
      >
        <SettlementSection year={year} month={month} />
      </Suspense>

      <Suspense
        key={`summary-${sectionKey}`}
        fallback={<CardSkeleton title="카테고리 × 액터 집계" />}
      >
        <SummarySection year={year} month={month} />
      </Suspense>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">최근 거래</CardTitle>
          <CardDescription>이 달에 기록된 최근 10건입니다.</CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <Suspense
            key={`recent-${sectionKey}`}
            fallback={<p className="text-sm text-muted-foreground p-4">불러오는 중...</p>}
          >
            <RecentTransactions year={year} month={month} />
          </Suspense>
        </CardContent>
      </Card>
    </div>
  );
}

function CardSkeleton({ title }: { title: string }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{title}</CardTitle>
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
