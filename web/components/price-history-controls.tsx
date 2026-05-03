"use client";

import { useRouter, useSearchParams, usePathname } from "next/navigation";
import { useTransition } from "react";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { ProductItem, MerchantItem } from "@/lib/schemas";

type View = "products" | "merchants";

export function PriceHistoryControls({
  view,
  productId,
  merchantId,
  memoLessOnly,
  products,
  merchants,
}: {
  view: View;
  productId: string | null;
  merchantId: string | null;
  memoLessOnly: boolean;
  products: ProductItem[];
  merchants: MerchantItem[];
}) {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const [, startTransition] = useTransition();

  function pushParams(updates: Record<string, string | null>) {
    const sp = new URLSearchParams(searchParams.toString());
    for (const [k, v] of Object.entries(updates)) {
      if (v === null || v === "") sp.delete(k);
      else sp.set(k, v);
    }
    const qs = sp.toString();
    startTransition(() => {
      router.push(qs ? `${pathname}?${qs}` : pathname);
    });
  }

  function onTabChange(next: string) {
    // Reset selection when switching tabs to avoid mixed state.
    if (next === "products") {
      pushParams({ view: "products", merchant_id: null, memo_less: null });
    } else {
      pushParams({ view: "merchants", product_id: null });
    }
  }

  return (
    <div className="space-y-4">
      <Tabs value={view} onValueChange={onTabChange}>
        <TabsList>
          <TabsTrigger value="products" data-testid="tab-products">
            Products
          </TabsTrigger>
          <TabsTrigger value="merchants" data-testid="tab-merchants">
            Merchants
          </TabsTrigger>
        </TabsList>
      </Tabs>

      {view === "products" ? (
        <div className="flex items-center gap-2">
          <label htmlFor="product-picker" className="text-sm font-medium">
            상품
          </label>
          <select
            id="product-picker"
            data-testid="product-picker"
            className="flex h-10 rounded-md border border-input bg-background px-3 py-2 text-sm min-w-[16rem]"
            value={productId ?? ""}
            onChange={(e) => pushParams({ product_id: e.target.value || null })}
          >
            <option value="">상품을 선택하세요</option>
            {products.map((p) => (
              <option key={p.id} value={p.id}>
                {p.merchant_name ? `[${p.merchant_name}] ` : ""}
                {p.name}
                {p.transaction_count > 0 ? ` (${p.transaction_count})` : ""}
              </option>
            ))}
          </select>
        </div>
      ) : (
        <div className="flex flex-wrap items-center gap-4">
          <div className="flex items-center gap-2">
            <label htmlFor="merchant-picker" className="text-sm font-medium">
              구매처
            </label>
            <select
              id="merchant-picker"
              data-testid="merchant-picker"
              className="flex h-10 rounded-md border border-input bg-background px-3 py-2 text-sm min-w-[16rem]"
              value={merchantId ?? ""}
              onChange={(e) => pushParams({ merchant_id: e.target.value || null })}
            >
              <option value="">구매처를 선택하세요</option>
              {merchants.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
          </div>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              data-testid="memo-less-toggle"
              className="h-4 w-4"
              checked={memoLessOnly}
              onChange={(e) =>
                pushParams({ memo_less: e.target.checked ? "1" : null })
              }
            />
            메모 없는 거래만
          </label>
        </div>
      )}
    </div>
  );
}
