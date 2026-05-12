"use client";

import { useState, useEffect } from "react";
import { PaymentMethodListSchema, type PaymentMethodItem } from "@/lib/schemas";

// ── Actor map helpers ─────────────────────────────────────────────────────────

type ActorEntry = { id: string; name: string };

/** Collect unique 엉아/아기 actors from the payment methods list. */
function collectActors(items: PaymentMethodItem[]): ActorEntry[] {
  const seen = new Map<string, string>();
  for (const pm of items) {
    if (pm.actor_id && pm.actor_name && pm.actor_name !== "공동") {
      seen.set(pm.actor_id, pm.actor_name);
    }
  }
  return Array.from(seen.entries()).map(([id, name]) => ({ id, name }));
}

// ── ActorToggle ───────────────────────────────────────────────────────────────

const BASE =
  "inline-flex items-center justify-center whitespace-nowrap rounded-sm px-3 py-1.5 text-sm font-medium ring-offset-background transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2";
const ACTIVE = "bg-background text-foreground shadow-sm";
const INACTIVE = "text-muted-foreground";

function ActorToggle({
  currentActorId,
  actors,
  onSelect,
}: {
  currentActorId: string | null;
  actors: ActorEntry[];
  onSelect: (actorId: string) => void;
}) {
  if (actors.length === 0) {
    return (
      <span className="text-sm text-muted-foreground italic">
        {currentActorId ? currentActorId : "미지정"}
      </span>
    );
  }

  return (
    <div
      className="inline-flex h-8 items-center justify-center rounded-md bg-muted p-1 text-muted-foreground"
      role="group"
    >
      {actors.map((actor) => (
        <button
          key={actor.id}
          type="button"
          data-testid={`actor-toggle-${actor.name}`}
          className={`${BASE} ${currentActorId === actor.id ? ACTIVE : INACTIVE}`}
          onClick={() => onSelect(actor.id)}
        >
          {actor.name}
        </button>
      ))}
    </div>
  );
}

// ── PaymentMethodTab ──────────────────────────────────────────────────────────

export function PaymentMethodTab() {
  const [items, setItems] = useState<PaymentMethodItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      setLoading(true);
      setError(null);
      try {
        const res = await fetch("/api/payment-methods-proxy");
        if (!res.ok) {
          throw new Error(`Failed to fetch payment methods (${res.status})`);
        }
        const json = await res.json();
        const parsed = PaymentMethodListSchema.safeParse(json);
        if (!parsed.success) {
          throw new Error("Invalid payment method data from server");
        }
        if (!cancelled) setItems(parsed.data);
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : "Unknown error");
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    load();
    return () => { cancelled = true; };
  }, []);

  async function handleActorToggle(pmId: string, actorId: string) {
    // Optimistic update
    const prev = items;
    const actorName = actors.find((a) => a.id === actorId)?.name ?? null;
    setItems((current) =>
      current.map((pm) =>
        pm.id === pmId ? { ...pm, actor_id: actorId, actor_name: actorName } : pm,
      ),
    );

    try {
      const res = await fetch(`/api/payment-methods-proxy/${encodeURIComponent(pmId)}/actor`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ actor_id: actorId }),
      });
      if (!res.ok) {
        throw new Error(`PATCH failed (${res.status})`);
      }
    } catch (err) {
      console.error("[PaymentMethodTab] actor toggle error:", err);
      // Rollback
      setItems(prev);
    }
  }

  const actors = collectActors(items);

  if (loading) {
    return (
      <div className="flex items-center gap-2 py-8 text-muted-foreground text-sm">
        <span>결제수단 목록 로딩 중...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className="py-4 text-sm text-destructive" role="alert">
        {error}
      </div>
    );
  }

  if (items.length === 0) {
    return (
      <div className="py-4 text-sm text-muted-foreground">
        결제수단이 없습니다.
      </div>
    );
  }

  return (
    <div data-testid="payment-method-list" className="space-y-2">
      <div className="grid grid-cols-[1fr_auto] gap-4 px-3 py-2 text-xs font-medium text-muted-foreground uppercase tracking-wide border-b">
        <span>결제수단</span>
        <span>담당자</span>
      </div>
      {items.map((pm) => (
        <div
          key={pm.id}
          data-testid={`payment-method-row-${pm.id}`}
          className="grid grid-cols-[1fr_auto] gap-4 px-3 py-2 rounded-md hover:bg-muted/50 items-center"
        >
          <span className="text-sm font-medium">{pm.name}</span>
          <ActorToggle
            currentActorId={pm.actor_id ?? null}
            actors={actors}
            onSelect={(actorId) => handleActorToggle(pm.id, actorId)}
          />
        </div>
      ))}
    </div>
  );
}
