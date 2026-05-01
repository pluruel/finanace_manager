import { Suspense } from "react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { apiFetch, ApiError } from "@/lib/api";
import { TransactionsResponseSchema, TransactionsResponse } from "@/lib/schemas";
import { formatAmount, formatDate } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { LayoutDashboard, TrendingUp, AlertCircle } from "lucide-react";

async function RecentTransactions() {
  let data: TransactionsResponse | null = null;
  let error: string | null = null;

  try {
    data = await apiFetch<TransactionsResponse>("/api/transactions", {
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
        아직 거래 내역이 없습니다. 엑셀 파일을 임포트해주세요.
      </p>
    );
  }

  // 최근 5개 아이템만 표시
  const recentItems = data.items.slice(0, 5);

  return (
    <div className="divide-y">
      {recentItems.map((item) => {
        const isDeduction = item.category_name === "차감";
        const isMultiLine = item.children.length > 0;
        const signed = formatAmount(item.amount, item.sign);

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
                {item.category_name && (
                  <span className="text-muted-foreground">
                    · {item.category_name}
                  </span>
                )}
                {item.actor_name && (
                  <span className="text-muted-foreground">
                    · {item.actor_name}
                  </span>
                )}
              </div>
            </div>
            <span
              className={`text-sm font-semibold ${
                item.sign === -1 ? "text-blue-600" : "text-foreground"
              }`}
            >
              {signed}
            </span>
          </div>
        );
      })}
    </div>
  );
}

export default function DashboardPage() {
  const now = new Date();
  const year = now.getFullYear();
  const month = now.getMonth() + 1;

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <div className="flex items-center gap-3">
        <LayoutDashboard className="h-6 w-6" />
        <h1 className="text-2xl font-bold">대시보드</h1>
      </div>

      {/* 정산 카드 — M2에서 활성화 */}
      <Card className="border-dashed opacity-60">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <TrendingUp className="h-4 w-4" />
            {year}년 {month}월 정산 카드
          </CardTitle>
          <CardDescription>
            M2 마일스톤에서 활성화됩니다. (경비인정 − 차감 = 입금액)
          </CardDescription>
        </CardHeader>
        <CardContent>
          <p className="text-muted-foreground text-sm">
            정산 데이터는 /api/settlement/:year/:month 엔드포인트가 준비되면 표시됩니다.
          </p>
        </CardContent>
      </Card>

      {/* 최근 거래 */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">최근 거래</CardTitle>
          <CardDescription>가장 최근에 기록된 거래 내역입니다.</CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <Suspense
            fallback={
              <p className="text-sm text-muted-foreground p-4">
                불러오는 중...
              </p>
            }
          >
            <RecentTransactions />
          </Suspense>
        </CardContent>
      </Card>
    </div>
  );
}
