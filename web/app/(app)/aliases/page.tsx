import { Suspense } from "react";
import { Tag, AlertCircle, Loader2 } from "lucide-react";
import { apiFetch, ApiError } from "@/lib/api";
import { ReviewQueueResponseSchema, ReviewQueueItem } from "@/lib/schemas";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { AliasesTabContent } from "@/components/aliases-tab-content";

// The 4 scopes exposed in the UI. Actor is intentionally omitted — only 3 fixed
// values and no review_state column.
const TABS = [
  { value: "category", label: "Category" },
  { value: "merchant", label: "Merchant" },
  { value: "payment_method", label: "Payment" },
  { value: "product", label: "Product" },
] as const;

type TabScope = (typeof TABS)[number]["value"];

// ── Per-tab server fetch ───────────────────────────────────────────────────────

async function fetchReviewQueue(scope: TabScope): Promise<ReviewQueueItem[]> {
  const data = await apiFetch(`/api/review-queue?scope=${scope}`, {
    schema: ReviewQueueResponseSchema,
  });
  return data;
}

// ── Tab panel (server component — fetches its own data) ───────────────────────

async function TabPanel({ scope }: { scope: TabScope }) {
  let items: ReviewQueueItem[];

  try {
    items = await fetchReviewQueue(scope);
  } catch (err) {
    const message =
      err instanceof ApiError
        ? `Error ${err.status}: ${err.message}`
        : err instanceof Error
          ? err.message
          : "Failed to load data.";

    return (
      <Alert variant="destructive">
        <AlertCircle className="h-4 w-4" />
        <AlertDescription>{message}</AlertDescription>
      </Alert>
    );
  }

  return <AliasesTabContent scope={scope} initialItems={items} />;
}

// ── Page ───────────────────────────────────────────────────────────────────────

export default function AliasesPage() {
  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <Tag className="h-6 w-6" />
        <h1 className="text-2xl font-bold">Normalization Review</h1>
      </div>

      <p className="text-sm text-muted-foreground">
        Review pending entities and either confirm them as new or merge them into existing ones.
        After a merge, all affected transactions are automatically remapped.
      </p>

      <Tabs defaultValue="category">
        <TabsList>
          {TABS.map((tab) => (
            <TabsTrigger key={tab.value} value={tab.value}>
              {tab.label}
            </TabsTrigger>
          ))}
        </TabsList>

        {TABS.map((tab) => (
          <TabsContent key={tab.value} value={tab.value} className="mt-4">
            <Suspense
              fallback={
                <div className="flex items-center gap-2 py-8 text-muted-foreground text-sm">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  Loading {tab.label.toLowerCase()} queue...
                </div>
              }
            >
              <TabPanel scope={tab.value} />
            </Suspense>
          </TabsContent>
        ))}
      </Tabs>
    </div>
  );
}
