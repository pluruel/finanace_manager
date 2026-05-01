import { Tag, Clock } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";

export default function AliasesPage() {
  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <div className="flex items-center gap-3">
        <Tag className="h-6 w-6" />
        <h1 className="text-2xl font-bold">정규화 관리</h1>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Clock className="h-5 w-5 text-muted-foreground" />
            M2 마일스톤 예정
          </CardTitle>
          <CardDescription>
            이 페이지는 M2 마일스톤에서 구현됩니다.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <p className="text-sm text-muted-foreground">
            정규화 관리 페이지에서는 다음 기능을 제공할 예정입니다:
          </p>
          <ul className="text-sm text-muted-foreground space-y-1 list-disc pl-5">
            <li>
              <strong>카테고리 탭</strong>: 카테고리 별칭 검토 및 확정
            </li>
            <li>
              <strong>구매처 탭</strong>: 구매처 별칭 검토 (예: "이 마트" → "이마트")
            </li>
            <li>
              <strong>결제수단 탭</strong>: 결제수단 별칭 검토
            </li>
            <li>
              <strong>상품 탭</strong>: 상품 별칭 합치기 및 product_id 재매핑
            </li>
          </ul>
          <p className="text-sm text-muted-foreground">
            임포트 후 미해결 별칭은 /import 페이지의 결과 카드에서 확인할 수 있습니다.
          </p>
        </CardContent>
      </Card>
    </div>
  );
}
