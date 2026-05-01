import { TrendingUp, Clock } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";

export default function PriceHistoryPage() {
  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <div className="flex items-center gap-3">
        <TrendingUp className="h-6 w-6" />
        <h1 className="text-2xl font-bold">가격 추적</h1>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Clock className="h-5 w-5 text-muted-foreground" />
            M3 마일스톤 예정
          </CardTitle>
          <CardDescription>
            이 페이지는 M3 마일스톤에서 구현됩니다.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <p className="text-sm text-muted-foreground">
            가격 추적 페이지에서는 다음 기능을 제공할 예정입니다:
          </p>
          <ul className="text-sm text-muted-foreground space-y-1 list-disc pl-5">
            <li>
              <strong>Products 모드</strong>: 메모가 있는 상품의 단가 시계열 라인 차트
            </li>
            <li>
              <strong>Merchants 모드</strong>: 메모 없는 거래의 구매처별 월합계 추이
            </li>
            <li>다중 월 비교 차트</li>
          </ul>
          <p className="text-sm text-muted-foreground">
            예: 고덕방 아이스아메리카노 단가 추이, 이마트 월별 지출액 등.
          </p>
        </CardContent>
      </Card>
    </div>
  );
}
