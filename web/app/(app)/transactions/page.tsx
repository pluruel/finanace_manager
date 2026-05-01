import { Suspense } from "react";
import { ListOrdered, AlertCircle } from "lucide-react";
import { apiFetch, ApiError } from "@/lib/api";
import { TransactionsResponseSchema, TransactionsResponse } from "@/lib/schemas";
import { TransactionsTable } from "@/components/transactions-table";
import { Alert, AlertDescription } from "@/components/ui/alert";

interface PageProps {
  searchParams: Promise<Record<string, string>>;
}

async function TransactionsList({
  searchParams,
}: {
  searchParams: Record<string, string>;
}) {
  const params = new URLSearchParams();

  const filterKeys = ["from", "to", "category", "actor", "merchant", "payment", "product", "group"];
  for (const key of filterKeys) {
    if (searchParams[key]) {
      params.set(key, searchParams[key]);
    }
  }

  const queryString = params.toString();
  const path = `/api/transactions${queryString ? `?${queryString}` : ""}`;

  try {
    const data = await apiFetch<TransactionsResponse>(path, {
      schema: TransactionsResponseSchema,
    });

    return (
      <TransactionsTable
        items={data.items}
        total={data.total}
        searchParams={searchParams}
      />
    );
  } catch (err) {
    if (err instanceof ApiError && err.status === 401) {
      return (
        <Alert variant="destructive">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription>인증이 만료되었습니다. 다시 로그인해주세요.</AlertDescription>
        </Alert>
      );
    }
    return (
      <Alert variant="destructive">
        <AlertCircle className="h-4 w-4" />
        <AlertDescription>
          {err instanceof Error ? err.message : "데이터를 불러오지 못했습니다."}
        </AlertDescription>
      </Alert>
    );
  }
}

export default async function TransactionsPage({ searchParams }: PageProps) {
  const resolvedSearchParams = await searchParams;

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <ListOrdered className="h-6 w-6" />
        <h1 className="text-2xl font-bold">거래 내역</h1>
      </div>

      <Suspense
        fallback={
          <p className="text-sm text-muted-foreground">불러오는 중...</p>
        }
      >
        <TransactionsList searchParams={resolvedSearchParams} />
      </Suspense>
    </div>
  );
}
