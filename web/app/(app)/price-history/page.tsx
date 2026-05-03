import { Suspense } from "react";
import { TrendingUp, AlertCircle } from "lucide-react";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  CardDescription,
} from "@/components/ui/card";
import { apiFetch, ApiError } from "@/lib/api";
import {
  ProductListSchema,
  type ProductItem,
  MerchantListSchema,
  type MerchantItem,
  PriceHistoryResponseSchema,
  type PriceHistoryResponse,
  MerchantStatsResponseSchema,
  type MerchantStatsResponse,
} from "@/lib/schemas";
import { formatKRW } from "@/lib/utils";
import { PriceHistoryControls } from "@/components/price-history-controls";
import { PriceHistoryChart } from "@/components/price-history-chart";
import { MerchantStatsChart } from "@/components/merchant-stats-chart";

type View = "products" | "merchants";

interface PageProps {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}

function asStr(v: string | string[] | undefined): string | null {
  return typeof v === "string" && v !== "" ? v : null;
}

async function fetchProducts(): Promise<ProductItem[]> {
  try {
    return await apiFetch<ProductItem[]>("/api/products", {
      schema: ProductListSchema,
    });
  } catch {
    return [];
  }
}

async function fetchMerchants(): Promise<MerchantItem[]> {
  try {
    return await apiFetch<MerchantItem[]>("/api/merchants", {
      schema: MerchantListSchema,
    });
  } catch {
    return [];
  }
}

async function fetchPriceHistory(
  productId: string,
): Promise<PriceHistoryResponse | { error: string; status?: number }> {
  try {
    return await apiFetch<PriceHistoryResponse>(
      `/api/price-history?product_id=${productId}`,
      { schema: PriceHistoryResponseSchema },
    );
  } catch (e) {
    if (e instanceof ApiError) return { error: e.message, status: e.status };
    return { error: "데이터를 불러오지 못했습니다." };
  }
}

async function fetchMerchantStats(
  merchantId: string,
  memoLessOnly: boolean,
): Promise<MerchantStatsResponse | { error: string; status?: number }> {
  const qs = new URLSearchParams({ merchant_id: merchantId });
  if (memoLessOnly) qs.set("memo_less_only", "true");
  try {
    return await apiFetch<MerchantStatsResponse>(
      `/api/merchant-stats?${qs.toString()}`,
      { schema: MerchantStatsResponseSchema },
    );
  } catch (e) {
    if (e instanceof ApiError) return { error: e.message, status: e.status };
    return { error: "데이터를 불러오지 못했습니다." };
  }
}

async function ProductsSection({ productId }: { productId: string | null }) {
  if (!productId) {
    return (
      <p
        data-testid="products-empty"
        className="text-sm text-muted-foreground p-4"
      >
        상단에서 상품을 선택하면 단가 시계열이 표시됩니다.
      </p>
    );
  }
  const result = await fetchPriceHistory(productId);
  if ("error" in result) {
    return (
      <div className="flex items-center gap-2 text-sm text-muted-foreground p-4">
        <AlertCircle className="h-4 w-4" />
        <span>{result.status === 404 ? "상품을 찾을 수 없습니다." : result.error}</span>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-baseline gap-4 px-1">
        <h3 className="text-lg font-semibold">
          {result.merchant_name ? `[${result.merchant_name}] ` : ""}
          {result.product_name}
        </h3>
        <span className="text-sm text-muted-foreground">{result.total}건</span>
      </div>
      <div className="grid grid-cols-3 gap-4 px-1">
        <Stat label="최소" value={result.min_unit_price} />
        <Stat label="평균" value={result.avg_unit_price} />
        <Stat label="최대" value={result.max_unit_price} />
      </div>
      <PriceHistoryChart data={result} />
    </div>
  );
}

async function MerchantsSection({
  merchantId,
  memoLessOnly,
}: {
  merchantId: string | null;
  memoLessOnly: boolean;
}) {
  if (!merchantId) {
    return (
      <p
        data-testid="merchants-empty"
        className="text-sm text-muted-foreground p-4"
      >
        상단에서 구매처를 선택하면 월별 합계가 표시됩니다.
      </p>
    );
  }
  const result = await fetchMerchantStats(merchantId, memoLessOnly);
  if ("error" in result) {
    return (
      <div className="flex items-center gap-2 text-sm text-muted-foreground p-4">
        <AlertCircle className="h-4 w-4" />
        <span>
          {result.status === 404 ? "구매처를 찾을 수 없습니다." : result.error}
        </span>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-baseline gap-4 px-1">
        <h3 className="text-lg font-semibold">{result.merchant_name}</h3>
        <span className="text-sm text-muted-foreground">
          {result.transaction_count}건 · 메모 없음 {result.memo_less_count}건
        </span>
      </div>
      <div className="grid grid-cols-2 gap-4 px-1">
        <Stat label="합계" value={result.grand_total} />
        <Stat label="개월 수" value={String(result.points.length)} raw />
      </div>
      <MerchantStatsChart data={result} />
    </div>
  );
}

function Stat({
  label,
  value,
  raw = false,
}: {
  label: string;
  value: string | null;
  raw?: boolean;
}) {
  return (
    <div className="rounded-md border p-3">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="text-base font-semibold mt-1">
        {value === null ? "-" : raw ? value : formatKRW(value)}
      </div>
    </div>
  );
}

export default async function PriceHistoryPage({ searchParams }: PageProps) {
  const params = await searchParams;
  const viewParam = asStr(params.view);
  const view: View = viewParam === "merchants" ? "merchants" : "products";
  const productId = asStr(params.product_id);
  const merchantId = asStr(params.merchant_id);
  const memoLessOnly = asStr(params.memo_less) === "1";

  const [products, merchants] = await Promise.all([
    fetchProducts(),
    fetchMerchants(),
  ]);

  return (
    <div className="max-w-5xl mx-auto space-y-6">
      <div className="flex items-center gap-3">
        <TrendingUp className="h-6 w-6" />
        <h1 className="text-2xl font-bold">가격 추적</h1>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">필터</CardTitle>
          <CardDescription>
            Products 탭은 단가 시계열, Merchants 탭은 월별 합계를 보여줍니다.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <PriceHistoryControls
            view={view}
            productId={productId}
            merchantId={merchantId}
            memoLessOnly={memoLessOnly}
            products={products}
            merchants={merchants}
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">
            {view === "products" ? "단가 시계열" : "월별 합계"}
          </CardTitle>
        </CardHeader>
        <CardContent>
          {view === "products" ? (
            <Suspense
              key={`products-${productId ?? "none"}`}
              fallback={<p className="text-sm text-muted-foreground p-4">불러오는 중...</p>}
            >
              <ProductsSection productId={productId} />
            </Suspense>
          ) : (
            <Suspense
              key={`merchants-${merchantId ?? "none"}-${memoLessOnly ? 1 : 0}`}
              fallback={<p className="text-sm text-muted-foreground p-4">불러오는 중...</p>}
            >
              <MerchantsSection merchantId={merchantId} memoLessOnly={memoLessOnly} />
            </Suspense>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
