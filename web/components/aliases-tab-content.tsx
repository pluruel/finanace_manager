"use client";

import { useState, useTransition, useCallback, useEffect } from "react";
import { useRouter } from "next/navigation";
import { CheckCircle2, Trash2, Merge, Loader2, ChevronDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
  DialogClose,
} from "@/components/ui/dialog";
import {
  ReviewQueueItem,
  PostAliasResponseSchema,
  ConfirmEntityResponseSchema,
} from "@/lib/schemas";

// ── Toast ─────────────────────────────────────────────────────────────────────

type ToastMessage = {
  id: number;
  message: string;
  variant: "success" | "error";
};

let toastCounter = 0;

function useToast() {
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  const show = useCallback((message: string, variant: "success" | "error" = "success") => {
    const id = ++toastCounter;
    setToasts((prev) => [...prev, { id, message, variant }]);
    setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 4000);
  }, []);

  return { toasts, show };
}

function Toaster({ toasts }: { toasts: ToastMessage[] }) {
  if (toasts.length === 0) return null;
  return (
    <div
      className="fixed bottom-4 right-4 z-50 flex flex-col gap-2"
      aria-live="polite"
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`rounded-md border px-4 py-3 text-sm shadow-md max-w-sm ${
            t.variant === "success"
              ? "bg-green-50 border-green-200 text-green-800"
              : "bg-destructive/10 border-destructive/30 text-destructive"
          }`}
        >
          {t.message}
        </div>
      ))}
    </div>
  );
}

// Scope string as used by the backend
type Scope = "category" | "merchant" | "payment_method" | "product";

function proxyUrl(scope: Scope): string | null {
  switch (scope) {
    case "category": return "/api/categories-proxy";
    case "merchant": return "/api/merchants-proxy";
    case "product": return "/api/products-proxy";
    default: return null;
  }
}

type EntityOption = { id: string; name: string };

// ── MergeDialog ───────────────────────────────────────────────────────────────

function MergeDialog({
  open,
  item,
  scope,
  onClose,
  onSuccess,
  onParseWarning,
}: {
  open: boolean;
  item: ReviewQueueItem | null;
  scope: Scope;
  onClose: () => void;
  onSuccess: (remapped: number) => void;
  onParseWarning: () => void;
}) {
  const [selectedTargetId, setSelectedTargetId] = useState<string>("");
  const [isPending, startTransition] = useTransition();
  const [inlineError, setInlineError] = useState<string | null>(null);
  const [allEntities, setAllEntities] = useState<EntityOption[]>([]);
  const [searchQuery, setSearchQuery] = useState("");
  const [loadingEntities, setLoadingEntities] = useState(false);

  useEffect(() => {
    if (!open || !item) return;
    const url = proxyUrl(scope);
    if (!url) return;
    const ctrl = new AbortController();
    setLoadingEntities(true);
    fetch(url, { signal: ctrl.signal })
      .then((r) => r.json())
      .then((data: unknown) => {
        if (!Array.isArray(data)) return;
        setAllEntities(
          (data as { id?: string; name?: string }[])
            .filter((e): e is EntityOption =>
              typeof e?.id === "string" &&
              typeof e?.name === "string" &&
              e.id !== item.id
            )
            .map((e) => ({ id: e.id, name: e.name })),
        );
      })
      .catch((err: unknown) => {
        if (err instanceof Error && err.name === "AbortError") return;
      })
      .finally(() => setLoadingEntities(false));
    return () => ctrl.abort();
  }, [open, scope, item]);

  // Reset state when dialog opens
  const handleOpenChange = (open: boolean) => {
    if (!open) {
      setSelectedTargetId("");
      setInlineError(null);
      setSearchQuery("");
      setAllEntities([]);
      onClose();
    }
  };

  if (!item) return null;

  // Build the merged list: merge_candidates first, then all other items from raw_texts
  // We need a full target list. merge_candidates from the server are the recommended ones.
  // The combobox shows merge_candidates (pre-sorted first).
  const candidates = item.merge_candidates;

  async function handleSubmit() {
    if (!selectedTargetId || !item) return;

    // Use the first alias raw_text for the merge call
    const rawText = item.raw_texts[0]?.raw_text ?? item.name;

    startTransition(async () => {
      setInlineError(null);
      try {
        const res = await fetch("/api/aliases-proxy", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            action: "merge",
            scope,
            raw_text: rawText,
            target_id: selectedTargetId,
            source_id: item.id,
          }),
        });

        if (res.status === 409) {
          const text = await res.text().catch(() => "");
          type Conflict409 = {
            error?: string;
            message?: string;
            source_actor?: string;
            target_actor?: string;
            // legacy fallback fields
            reason?: string;
            detail?: string;
          };
          let parsed409: Conflict409 = {};
          try {
            parsed409 = JSON.parse(text) as Conflict409;
          } catch {
            // raw text fallback
          }

          const errorCode = parsed409.error ?? parsed409.reason ?? "";
          const fallbackMessage = parsed409.message ?? parsed409.detail ?? text;

          if (errorCode === "actor_mismatch" || errorCode.includes("actor_mismatch")) {
            if (parsed409.source_actor && parsed409.target_actor) {
              setInlineError(
                `Cannot merge: source belongs to ${parsed409.source_actor}, target to ${parsed409.target_actor}.`,
              );
            } else {
              setInlineError("Cannot merge: payment methods belong to different actors.");
            }
          } else if (errorCode === "alias_changed" || errorCode.includes("alias_changed") || fallbackMessage.includes("alias_changed")) {
            setInlineError("Merge conflict: another operation changed this alias. Please refresh and try again.");
          } else if (errorCode === "deduction_protected" || fallbackMessage.includes("deduction_protected")) {
            setInlineError("Cannot modify the 차감 category.");
          } else {
            setInlineError(`Conflict: ${fallbackMessage || errorCode || "unknown"}`);
          }
          return;
        }

        if (!res.ok) {
          const text = await res.text().catch(() => "Unknown error");
          setInlineError(`Error: ${text}`);
          return;
        }

        const raw: unknown = await res.json();
        const parsed = PostAliasResponseSchema.safeParse(raw);
        if (!parsed.success) {
          // Soft warning: parse failed on a 200 OK response — surface warning and refresh.
          onParseWarning();
          onClose();
          return;
        }
        const remapped = parsed.data.remapped_transaction_count;
        onSuccess(remapped);
        onClose();
      } catch (err) {
        setInlineError(err instanceof Error ? err.message : "Network error");
      }
    });
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Merge &ldquo;{item.name}&rdquo; into existing entity</DialogTitle>
          <DialogDescription>
            Choose the target entity. All transactions referencing &ldquo;{item.name}&rdquo; will be
            remapped to the target.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* 추천 후보 섹션 */}
          {candidates.length > 0 && (
            <div className="space-y-1.5">
              <label htmlFor="merge-target-select" className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">
                추천 후보
              </label>
              <select
                id="merge-target-select"
                aria-label="Target entity"
                value={selectedTargetId}
                onChange={(e) => setSelectedTargetId(e.target.value)}
                className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
              >
                <option value="">Select target...</option>
                {candidates.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
              </select>
            </div>
          )}

          {/* 직접 검색 섹션 */}
          <div className="space-y-1.5">
            <label className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">
              직접 검색
            </label>
            <Input
              data-testid="merge-search-input"
              placeholder="이름으로 검색 (2자 이상)..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
            />
            {searchQuery.length >= 2 && (
              <div className="border rounded-md overflow-hidden max-h-44 overflow-y-auto">
                {loadingEntities ? (
                  <div className="px-3 py-2 text-sm text-muted-foreground">로딩 중...</div>
                ) : (() => {
                  const candidateIds = new Set(candidates.map((c) => c.id));
                  const filtered = allEntities.filter(
                    (e) =>
                      !candidateIds.has(e.id) &&
                      e.name.toLowerCase().includes(searchQuery.toLowerCase()),
                  );
                  return filtered.length === 0 ? (
                    <div className="px-3 py-2 text-sm text-muted-foreground">검색 결과 없음</div>
                  ) : (
                    filtered.map((e) => (
                      <button
                        key={e.id}
                        type="button"
                        className={`w-full text-left px-3 py-2 text-sm transition-colors hover:bg-muted ${
                          selectedTargetId === e.id ? "bg-muted font-medium" : ""
                        }`}
                        onClick={() => setSelectedTargetId(e.id)}
                      >
                        {e.name}
                      </button>
                    ))
                  );
                })()}
              </div>
            )}
            {candidates.length === 0 && searchQuery.length < 2 && (
              <p className="text-xs text-muted-foreground">
                추천 후보가 없어요. 이름으로 직접 검색하세요.
              </p>
            )}
          </div>

          {inlineError && (
            <Alert variant="destructive">
              <AlertDescription>{inlineError}</AlertDescription>
            </Alert>
          )}
        </div>

        <DialogFooter>
          <DialogClose asChild>
            <Button variant="outline" disabled={isPending}>
              Cancel
            </Button>
          </DialogClose>
          <Button
            onClick={handleSubmit}
            disabled={!selectedTargetId || isPending}
          >
            {isPending ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                Merging...
              </>
            ) : (
              <>
                <Merge className="h-4 w-4" />
                Merge
              </>
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── DeleteDialog ──────────────────────────────────────────────────────────────

function DeleteDialog({
  open,
  aliasId,
  rawText,
  onClose,
  onSuccess,
  onError,
}: {
  open: boolean;
  aliasId: string;
  rawText: string;
  onClose: () => void;
  onSuccess: () => void;
  onError: (message: string) => void;
}) {
  const [isPending, startTransition] = useTransition();

  function handleDelete() {
    startTransition(async () => {
      try {
        const res = await fetch(`/api/aliases-proxy?id=${encodeURIComponent(aliasId)}`, {
          method: "DELETE",
        });
        if (!res.ok) {
          let message = `Failed to delete alias (HTTP ${res.status})`;
          try {
            const body = await res.text();
            if (body) {
              const json = JSON.parse(body) as { message?: string; error?: string };
              message = json.message ?? json.error ?? body;
            }
          } catch {
            // ignore parse error; use default message
          }
          onError(message);
          onClose();
          return;
        }
        onSuccess();
        onClose();
      } catch (err) {
        onError(err instanceof Error ? err.message : "Network error");
        onClose();
      }
    });
  }

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) onClose(); }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete alias</DialogTitle>
          <DialogDescription>
            Are you sure you want to delete the alias &ldquo;{rawText}&rdquo;? This does not affect
            transactions — only the alias mapping is removed.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <DialogClose asChild>
            <Button variant="outline" disabled={isPending}>
              Cancel
            </Button>
          </DialogClose>
          <Button variant="destructive" onClick={handleDelete} disabled={isPending}>
            {isPending ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                Deleting...
              </>
            ) : (
              <>
                <Trash2 className="h-4 w-4" />
                Delete
              </>
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── AliasesTabContent ─────────────────────────────────────────────────────────

export function AliasesTabContent({
  scope,
  initialItems,
}: {
  scope: Scope;
  initialItems: ReviewQueueItem[];
}) {
  const router = useRouter();
  const { toasts, show: showToast } = useToast();

  // Local optimistic state — items managed here after server initial fetch
  const [items, setItems] = useState<ReviewQueueItem[]>(initialItems);
  const [confirming, setConfirming] = useState<Set<string>>(new Set());

  // Merge dialog state
  const [mergeTarget, setMergeTarget] = useState<ReviewQueueItem | null>(null);

  // Delete dialog state
  const [deleteAlias, setDeleteAlias] = useState<{
    entityId: string;
    aliasId: string;
    rawText: string;
  } | null>(null);

  function removeItem(entityId: string) {
    setItems((prev) => prev.filter((it) => it.id !== entityId));
  }

  function removeAlias(entityId: string, aliasId: string) {
    setItems((prev) =>
      prev
        .map((it) => {
          if (it.id !== entityId) return it;
          const remaining = it.raw_texts.filter((a) => a.alias_id !== aliasId);
          // If no aliases remain, remove the item entirely
          if (remaining.length === 0) return null;
          return { ...it, raw_texts: remaining };
        })
        .filter((it): it is ReviewQueueItem => it !== null),
    );
  }

  async function handleConfirm(item: ReviewQueueItem) {
    setConfirming((prev) => new Set(Array.from(prev).concat(item.id)));
    try {
      const res = await fetch(`/api/entities-proxy/${scope}/${item.id}/confirm`, {
        method: "POST",
      });

      if (res.status === 409) {
        showToast("Cannot confirm: this entity is protected (차감).", "error");
        return;
      }

      if (!res.ok) {
        const text = await res.text().catch(() => "Error");
        showToast(`Failed to confirm: ${text}`, "error");
        return;
      }

      const raw: unknown = await res.json();
      const parsed = ConfirmEntityResponseSchema.safeParse(raw);
      // Optimistically remove the confirmed item from pending list regardless of parse result
      removeItem(item.id);
      if (parsed.success) {
        showToast(`"${item.name}" confirmed.`);
      } else {
        // Soft warning: response shape unexpected; still treat as success but warn.
        showToast(
          "Server returned an unexpected response shape. Refresh to verify.",
          "error",
        );
      }
      router.refresh();
    } catch (err) {
      showToast(err instanceof Error ? err.message : "Network error", "error");
    } finally {
      setConfirming((prev) => {
        const next = new Set(Array.from(prev));
        next.delete(item.id);
        return next;
      });
    }
  }

  function handleMergeSuccess(remapped: number) {
    if (mergeTarget) {
      removeItem(mergeTarget.id);
    }
    showToast(
      `Merged successfully. ${remapped} transaction${remapped !== 1 ? "s" : ""} remapped.`,
    );
    router.refresh();
  }

  function handleMergeParseWarning() {
    if (mergeTarget) {
      removeItem(mergeTarget.id);
    }
    showToast(
      "Server returned an unexpected response shape. Refresh to verify.",
      "error",
    );
    router.refresh();
  }

  function handleDeleteSuccess(entityId: string, aliasId: string) {
    removeAlias(entityId, aliasId);
    showToast("Alias deleted.");
    router.refresh();
  }

  async function handleKindChange(itemId: string, nextKind: "income" | "expense") {
    // optimistic update
    const previous = items.find((it) => it.id === itemId)?.kind ?? null;
    setItems((prev) =>
      prev.map((it) => (it.id === itemId ? { ...it, kind: nextKind } : it)),
    );
    try {
      const res = await fetch(`/api/categories-proxy/${itemId}/kind`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ kind: nextKind }),
      });
      if (!res.ok) {
        setItems((prev) =>
          prev.map((it) => (it.id === itemId ? { ...it, kind: previous } : it)),
        );
        const text = await res.text().catch(() => "");
        showToast(`종류 변경 실패: ${text || res.status}`, "error");
      } else {
        showToast(`${nextKind === "income" ? "수입" : "지출"} 으로 변경되었습니다.`);
      }
    } catch (err) {
      setItems((prev) =>
        prev.map((it) => (it.id === itemId ? { ...it, kind: previous } : it)),
      );
      showToast(err instanceof Error ? err.message : "Network error", "error");
    }
  }

  if (items.length === 0) {
    return (
      <>
        <div className="flex flex-col items-center justify-center py-16 text-center">
          <CheckCircle2 className="h-10 w-10 text-green-500 mb-3" />
          <p className="text-sm text-muted-foreground">
            All {scope.replace("_", " ")} entries are confirmed.
          </p>
        </div>
        <Toaster toasts={toasts} />
      </>
    );
  }

  return (
    <>
      <div className="overflow-x-auto rounded-md border">
        <table className="w-full text-sm">
          <thead>
            <tr className="bg-muted/50 border-b">
              <th className="px-4 py-3 text-left font-medium">Entity name</th>
              <th className="px-4 py-3 text-left font-medium">Aliases (raw text / norm key)</th>
              <th className="px-4 py-3 text-left font-medium">State</th>
              {scope === "category" && (
                <th className="px-4 py-3 text-left font-medium">Kind</th>
              )}
              <th className="px-4 py-3 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {items.map((item) => (
              <ItemRow
                key={item.id}
                item={item}
                scope={scope}
                isConfirming={confirming.has(item.id)}
                onConfirm={() => handleConfirm(item)}
                onMerge={() => setMergeTarget(item)}
                onDeleteAlias={(aliasId, rawText) => setDeleteAlias({ entityId: item.id, aliasId, rawText })}
                onKindChange={handleKindChange}
              />
            ))}
          </tbody>
        </table>
      </div>

      <MergeDialog
        open={mergeTarget !== null}
        item={mergeTarget}
        scope={scope}
        onClose={() => setMergeTarget(null)}
        onSuccess={handleMergeSuccess}
        onParseWarning={handleMergeParseWarning}
      />

      {deleteAlias && (
        <DeleteDialog
          open={true}
          aliasId={deleteAlias.aliasId}
          rawText={deleteAlias.rawText}
          onClose={() => setDeleteAlias(null)}
          onSuccess={() => {
            handleDeleteSuccess(deleteAlias.entityId, deleteAlias.aliasId);
            setDeleteAlias(null);
          }}
          onError={(message) => {
            showToast(message, "error");
            setDeleteAlias(null);
          }}
        />
      )}

      <Toaster toasts={toasts} />
    </>
  );
}

// ── ItemRow ───────────────────────────────────────────────────────────────────

function ItemRow({
  item,
  scope,
  isConfirming,
  onConfirm,
  onMerge,
  onDeleteAlias,
  onKindChange,
}: {
  item: ReviewQueueItem;
  scope: Scope;
  isConfirming: boolean;
  onConfirm: () => void;
  onMerge: () => void;
  onDeleteAlias: (aliasId: string, rawText: string) => void;
  onKindChange: (itemId: string, nextKind: "income" | "expense") => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const hasMultipleAliases = item.raw_texts.length > 1;

  return (
    <tr className="border-b last:border-b-0 hover:bg-muted/30 transition-colors">
      {/* Entity name */}
      <td className="px-4 py-3 font-medium align-top">
        {item.name}
        {item.merge_candidates.length > 0 && (
          <div className="text-xs text-muted-foreground mt-0.5">
            {item.merge_candidates.length} merge candidate
            {item.merge_candidates.length !== 1 ? "s" : ""}
          </div>
        )}
      </td>

      {/* Aliases */}
      <td className="px-4 py-3 align-top">
        {item.raw_texts.length === 0 ? (
          <span className="text-xs text-muted-foreground italic">No aliases</span>
        ) : (
          <div className="space-y-1">
            {(expanded ? item.raw_texts : item.raw_texts.slice(0, 1)).map((alias) => (
              <div key={alias.alias_id} className="flex items-center gap-2 group">
                <span className="font-mono text-xs">{alias.raw_text}</span>
                <span className="text-xs text-muted-foreground">→</span>
                <span className="font-mono text-xs text-muted-foreground">{alias.norm_key}</span>
                <button
                  onClick={() => onDeleteAlias(alias.alias_id, alias.raw_text)}
                  className="opacity-0 group-hover:opacity-100 transition-opacity ml-1 text-destructive hover:text-destructive/80"
                  aria-label={`Delete alias ${alias.raw_text}`}
                  title="Delete this alias"
                >
                  <Trash2 className="h-3 w-3" />
                </button>
              </div>
            ))}
            {hasMultipleAliases && (
              <button
                onClick={() => setExpanded((e) => !e)}
                className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
              >
                <ChevronDown
                  className={`h-3 w-3 transition-transform ${expanded ? "rotate-180" : ""}`}
                />
                {expanded
                  ? "Show less"
                  : `+${item.raw_texts.length - 1} more`}
              </button>
            )}
          </div>
        )}
      </td>

      {/* State badge */}
      <td className="px-4 py-3 align-top">
        <Badge
          variant={item.review_state === "pending" ? "secondary" : "default"}
          className={item.review_state === "confirmed" ? "bg-green-100 text-green-800 border-green-200" : ""}
        >
          {item.review_state}
        </Badge>
      </td>

      {/* Kind toggle (category scope only) */}
      {scope === "category" && (
        <td className="px-4 py-3 align-top">
          <div className="flex items-center gap-2">
            <Switch
              aria-label={`${item.name} 종류 토글`}
              checked={item.kind === "income"}
              disabled={item.name === "차감"}
              onCheckedChange={(checked) =>
                onKindChange(item.id, checked ? "income" : "expense")
              }
            />
            <span className="text-xs text-muted-foreground">
              {item.kind === "income" ? "수입" : "지출"}
            </span>
          </div>
        </td>
      )}

      {/* Actions */}
      <td className="px-4 py-3 align-top">
        <div className="flex items-center gap-2 justify-end">
          <Button
            size="sm"
            variant="outline"
            onClick={onConfirm}
            disabled={isConfirming || item.review_state === "confirmed"}
            className="text-xs"
            title="Confirm as new entity"
          >
            {isConfirming ? (
              <Loader2 className="h-3 w-3 animate-spin" />
            ) : (
              <CheckCircle2 className="h-3 w-3" />
            )}
            <span className="ml-1">Confirm</span>
          </Button>

          <Button
            size="sm"
            variant="outline"
            onClick={onMerge}
            disabled={item.raw_texts.length === 0}
            className="text-xs"
            title={
              item.raw_texts.length === 0
                ? "No aliases to merge"
                : "Merge into an existing entity"
            }
          >
            <Merge className="h-3 w-3" />
            <span className="ml-1">Merge</span>
          </Button>
        </div>
      </td>
    </tr>
  );
}
