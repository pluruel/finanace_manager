/**
 * Aliases page — vitest + @testing-library/react
 *
 * Covers:
 * 1. Merge dialog interaction — mocked fetch, user picks candidate, submits,
 *    sees success toast with remapped count.
 * 2. 409 actor_mismatch rendering on a payment-method row.
 * 3. Tab switching state isolation — data for one scope doesn't bleed into
 *    another scope rendered in the same tree.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AliasesTabContent } from "../components/aliases-tab-content";
import type { ReviewQueueItem } from "../lib/schemas";

// ── Mock next/navigation ──────────────────────────────────────────────────────

const mockRefresh = vi.fn();

vi.mock("next/navigation", () => ({
  useRouter: () => ({
    refresh: mockRefresh,
    push: vi.fn(),
    replace: vi.fn(),
  }),
}));

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeMerchantItem(overrides: Partial<ReviewQueueItem> = {}): ReviewQueueItem {
  return {
    scope: "merchant",
    id: "aaaa0000-0000-0000-0000-000000000001",
    name: "이 마트",
    review_state: "pending",
    raw_texts: [
      {
        alias_id: "bbbb0000-0000-0000-0000-000000000001",
        raw_text: "이 마트",
        norm_key: "이마트",
      },
    ],
    merge_candidates: [
      {
        id: "cccc0000-0000-0000-0000-000000000001",
        name: "이마트",
      },
    ],
    ...overrides,
  };
}

function makePaymentItem(overrides: Partial<ReviewQueueItem> = {}): ReviewQueueItem {
  return {
    scope: "payment_method",
    id: "dddd0000-0000-0000-0000-000000000001",
    name: "신한아기",
    review_state: "pending",
    raw_texts: [
      {
        alias_id: "eeee0000-0000-0000-0000-000000000001",
        raw_text: "신한아기",
        norm_key: "신한아기",
      },
    ],
    merge_candidates: [
      {
        id: "ffff0000-0000-0000-0000-000000000001",
        name: "신한",
      },
    ],
    ...overrides,
  };
}

function makeCategoryItem(overrides: Partial<ReviewQueueItem> = {}): ReviewQueueItem {
  return {
    scope: "category",
    id: "1111aaaa-0000-0000-0000-000000000001",
    name: "식비",
    review_state: "pending",
    raw_texts: [
      {
        alias_id: "2222bbbb-0000-0000-0000-000000000001",
        raw_text: "식비",
        norm_key: "식비",
      },
    ],
    merge_candidates: [],
    ...overrides,
  };
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/** Assert text exists somewhere in the rendered tree */
function assertTextPresent(text: string | RegExp) {
  const els = screen.queryAllByText(text);
  expect(els.length).toBeGreaterThan(0);
}

/** Assert text does NOT exist in the rendered tree */
function assertTextAbsent(text: string | RegExp) {
  const els = screen.queryAllByText(text);
  expect(els.length).toBe(0);
}

// ── Test 1: Merge dialog interaction ─────────────────────────────────────────

describe("Merge dialog interaction", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mockRefresh.mockClear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("opens merge dialog, selects candidate, submits, and shows success toast with remapped count", async () => {
    const user = userEvent.setup();

    global.fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      text: async () =>
        JSON.stringify({
          created: false,
          remapped_transaction_count: 5,
          orphan_deleted: true,
        }),
      json: async () => ({
        created: false,
        remapped_transaction_count: 5,
        orphan_deleted: true,
      }),
    });

    const item = makeMerchantItem();
    render(<AliasesTabContent scope="merchant" initialItems={[item]} />);

    // Entity name column renders "이 마트"
    const entityNameCells = screen.queryAllByText("이 마트");
    expect(entityNameCells.length).toBeGreaterThan(0);

    // Click "Merge" action button in the row
    const mergeButton = screen.getByTitle("Merge into an existing entity");
    await user.click(mergeButton);

    // Dialog opens — the dialog element should be present
    const dialogs = screen.queryAllByRole("dialog");
    expect(dialogs.length).toBe(1);

    // Find the native <select> inside the dialog and pick the candidate
    const select = screen.getByRole("combobox", { name: /target entity/i });
    expect(select).toBeTruthy();
    await user.selectOptions(select, "cccc0000-0000-0000-0000-000000000001");
    expect((select as HTMLSelectElement).value).toBe("cccc0000-0000-0000-0000-000000000001");

    // Find the submit Merge button inside the dialog (not the row button)
    const allMergeButtons = screen.getAllByRole("button", { name: /Merge/ });
    const dialogEl = screen.getByRole("dialog");
    const submitButton = allMergeButtons.find((btn) => dialogEl.contains(btn))!;
    expect(submitButton).toBeTruthy();

    await user.click(submitButton);

    // Success toast with remapped count should appear
    await waitFor(() => {
      const toastEls = screen.queryAllByText(/5 transactions remapped/);
      expect(toastEls.length).toBeGreaterThan(0);
    });

    // After merge, the item disappears (optimistic update) → empty state shows
    await waitFor(() => {
      const emptyEls = screen.queryAllByText(/All merchant entries are confirmed/);
      expect(emptyEls.length).toBeGreaterThan(0);
    });

    // router.refresh() was called
    expect(mockRefresh).toHaveBeenCalled();
  });

  it("shows empty state when all items are confirmed", () => {
    render(<AliasesTabContent scope="merchant" initialItems={[]} />);
    assertTextPresent(/All merchant entries are confirmed/);
  });

  it("shows singular 'transaction' in toast when remapped count is 1", async () => {
    const user = userEvent.setup();

    global.fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      text: async () =>
        JSON.stringify({
          created: false,
          remapped_transaction_count: 1,
          orphan_deleted: false,
        }),
      json: async () => ({
        created: false,
        remapped_transaction_count: 1,
        orphan_deleted: false,
      }),
    });

    const item = makeMerchantItem();
    render(<AliasesTabContent scope="merchant" initialItems={[item]} />);

    await user.click(screen.getByTitle("Merge into an existing entity"));

    const select = screen.getByRole("combobox", { name: /target entity/i });
    await user.selectOptions(select, "cccc0000-0000-0000-0000-000000000001");

    const allMergeButtons = screen.getAllByRole("button", { name: /Merge/ });
    const dialogEl = screen.getByRole("dialog");
    const submitButton = allMergeButtons.find((btn) => dialogEl.contains(btn))!;
    await user.click(submitButton);

    await waitFor(() => {
      // singular: "1 transaction remapped" — not "transactions"
      const els = screen.queryAllByText(/1 transaction remapped/);
      expect(els.length).toBeGreaterThan(0);
    });
  });
});

// ── Test 2: 409 actor_mismatch on payment method ─────────────────────────────

describe("409 actor_mismatch on payment method merge", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mockRefresh.mockClear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("shows inline error when backend returns 409 with structured actor_mismatch", async () => {
    const user = userEvent.setup();

    global.fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 409,
      text: async () =>
        JSON.stringify({
          error: "actor_mismatch",
          message: "Cannot merge payment methods across actors",
          source_actor: "아기",
          target_actor: "엉아",
        }),
    });

    const item = makePaymentItem();
    render(<AliasesTabContent scope="payment_method" initialItems={[item]} />);

    await user.click(screen.getByTitle("Merge into an existing entity"));

    const dialogs = screen.queryAllByRole("dialog");
    expect(dialogs.length).toBe(1);

    const select = screen.getByRole("combobox", { name: /target entity/i });
    await user.selectOptions(select, "ffff0000-0000-0000-0000-000000000001");

    const allMergeButtons = screen.getAllByRole("button", { name: /Merge/ });
    const dialogEl = screen.getByRole("dialog");
    const submitButton = allMergeButtons.find((btn) => dialogEl.contains(btn))!;
    await user.click(submitButton);

    // Inline error shows actor names from structured backend fields
    await waitFor(() => {
      const dialogNow = screen.getByRole("dialog");
      expect(dialogNow.textContent).toMatch(/Cannot merge: source belongs to 아기, target to 엉아/);
    });

    // Dialog stays open (user can cancel)
    expect(screen.queryAllByRole("dialog").length).toBe(1);

    // No router refresh on failure
    expect(mockRefresh).not.toHaveBeenCalled();
  });

  it("shows generic actor mismatch message when actor fields are absent", async () => {
    const user = userEvent.setup();

    global.fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 409,
      text: async () =>
        JSON.stringify({
          error: "actor_mismatch",
          message: "Payment methods belong to different actors",
        }),
    });

    const item = makePaymentItem();
    render(<AliasesTabContent scope="payment_method" initialItems={[item]} />);

    await user.click(screen.getByTitle("Merge into an existing entity"));

    const select = screen.getByRole("combobox", { name: /target entity/i });
    await user.selectOptions(select, "ffff0000-0000-0000-0000-000000000001");

    const allMergeButtons = screen.getAllByRole("button", { name: /Merge/ });
    const dialogEl = screen.getByRole("dialog");
    const submitButton = allMergeButtons.find((btn) => dialogEl.contains(btn))!;
    await user.click(submitButton);

    await waitFor(() => {
      const dialogNow = screen.getByRole("dialog");
      // Falls back to generic actor mismatch message
      expect(dialogNow.textContent).toMatch(
        /Cannot merge: payment methods belong to different actors/,
      );
    });
  });

  it("shows alias_changed error message for concurrent merge 409", async () => {
    const user = userEvent.setup();

    global.fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 409,
      text: async () =>
        JSON.stringify({
          error: "alias_changed",
          message: "A concurrent merge already remapped this alias",
        }),
    });

    const item = makePaymentItem();
    render(<AliasesTabContent scope="payment_method" initialItems={[item]} />);

    await user.click(screen.getByTitle("Merge into an existing entity"));

    const select = screen.getByRole("combobox", { name: /target entity/i });
    await user.selectOptions(select, "ffff0000-0000-0000-0000-000000000001");

    const allMergeButtons = screen.getAllByRole("button", { name: /Merge/ });
    const dialogEl = screen.getByRole("dialog");
    const submitButton = allMergeButtons.find((btn) => dialogEl.contains(btn))!;
    await user.click(submitButton);

    await waitFor(() => {
      const dialogNow = screen.getByRole("dialog");
      // Component shows "Merge conflict: another operation changed this alias."
      expect(dialogNow.textContent).toMatch(/Merge conflict/i);
    });
  });
});

// ── Test 3: DELETE alias error path ──────────────────────────────────────────

describe("DELETE alias error path", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mockRefresh.mockClear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("shows error toast and does not remove alias when DELETE returns 500", async () => {
    const user = userEvent.setup();

    global.fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 500,
      text: async () =>
        JSON.stringify({
          error: "internal_server_error",
          message: "Unexpected database error",
        }),
    });

    const item = makeMerchantItem();
    render(<AliasesTabContent scope="merchant" initialItems={[item]} />);

    // Hover to reveal delete button and click it
    const deleteButton = screen.getByRole("button", { name: /Delete alias 이 마트/i });
    await user.click(deleteButton);

    // Confirm delete dialog opens
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toBeTruthy();

    // Click the Delete button in the dialog
    const deleteDialogButton = screen.getByRole("button", { name: /^Delete$/i });
    await user.click(deleteDialogButton);

    // Error toast should appear
    await waitFor(() => {
      const toastEl = screen.queryAllByText(/Unexpected database error/);
      expect(toastEl.length).toBeGreaterThan(0);
    });

    // The alias should still be present (no optimistic removal on failure)
    expect(screen.queryAllByText("이 마트").length).toBeGreaterThan(0);

    // router.refresh was NOT called on failure
    expect(mockRefresh).not.toHaveBeenCalled();
  });
});

// ── Test 4: Tab switching state isolation ─────────────────────────────────────

describe("Tab switching state isolation", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mockRefresh.mockClear();
  });

  it("renders correct items for each scope independently via remount (key)", () => {
    const merchantItem = makeMerchantItem();

    // Wrap in a parent that can change key to force remount
    function Wrapper({ scope, items }: { scope: "merchant" | "category"; items: ReviewQueueItem[] }) {
      return <AliasesTabContent key={scope} scope={scope} initialItems={items} />;
    }

    const { rerender } = render(
      <Wrapper scope="merchant" items={[merchantItem]} />,
    );

    // Merchant scope: entity name "이 마트" is present
    expect(screen.queryAllByText("이 마트").length).toBeGreaterThan(0);
    // Category item not present
    expect(screen.queryAllByText("식비").length).toBe(0);

    // Remount with category scope (key changes → full remount)
    const categoryItem = makeCategoryItem();
    rerender(<Wrapper scope="category" items={[categoryItem]} />);

    // Category scope: "식비" present, "이 마트" gone
    expect(screen.queryAllByText("식비").length).toBeGreaterThan(0);
    expect(screen.queryAllByText("이 마트").length).toBe(0);
  });

  it("two scopes rendered simultaneously do not bleed data into each other", () => {
    const merchantItem = makeMerchantItem();
    const categoryItem = makeCategoryItem();

    const { container } = render(
      <div>
        <div data-testid="merchant-tab">
          <AliasesTabContent scope="merchant" initialItems={[merchantItem]} />
        </div>
        <div data-testid="category-tab">
          <AliasesTabContent scope="category" initialItems={[categoryItem]} />
        </div>
      </div>,
    );

    const merchantTab = container.querySelector("[data-testid='merchant-tab']")!;
    const categoryTab = container.querySelector("[data-testid='category-tab']")!;

    expect(merchantTab.textContent).toContain("이 마트");
    expect(merchantTab.textContent).not.toContain("식비");

    expect(categoryTab.textContent).toContain("식비");
    expect(categoryTab.textContent).not.toContain("이 마트");
  });

  it("empty state per scope renders scope-specific message", () => {
    type Scope = "merchant" | "product" | "category" | "payment_method";
    function Wrapper({ scope }: { scope: Scope }) {
      return <AliasesTabContent key={scope} scope={scope} initialItems={[]} />;
    }

    const { rerender } = render(<Wrapper scope="merchant" />);
    assertTextPresent(/All merchant entries are confirmed/);

    rerender(<Wrapper scope="product" />);
    assertTextPresent(/All product entries are confirmed/);

    rerender(<Wrapper scope="category" />);
    assertTextPresent(/All category entries are confirmed/);

    rerender(<Wrapper scope="payment_method" />);
    // scope.replace("_", " ") → "payment method"
    assertTextPresent(/All payment method entries are confirmed/);
  });

  it("optimistic confirm on one scope does not affect another scope's items", async () => {
    const user = userEvent.setup();

    global.fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      text: async () =>
        JSON.stringify({ id: "1111aaaa-0000-0000-0000-000000000001", review_state: "confirmed" }),
      json: async () => ({
        id: "1111aaaa-0000-0000-0000-000000000001",
        review_state: "confirmed",
      }),
    });

    const categoryItem = makeCategoryItem();
    const { container } = render(
      <div>
        <div data-testid="category-tab">
          <AliasesTabContent scope="category" initialItems={[categoryItem]} />
        </div>
        <div data-testid="product-tab">
          <AliasesTabContent scope="product" initialItems={[]} />
        </div>
      </div>,
    );

    // Product tab already shows empty state
    const productTab = container.querySelector("[data-testid='product-tab']")!;
    expect(productTab.textContent).toContain("All product entries are confirmed");

    // Confirm the category item
    const confirmButton = screen.getByTitle("Confirm as new entity");
    await user.click(confirmButton);

    await waitFor(() => {
      const categoryTab = container.querySelector("[data-testid='category-tab']")!;
      expect(categoryTab.textContent).toContain("All category entries are confirmed");
    });

    // Product tab unchanged
    expect(productTab.textContent).toContain("All product entries are confirmed");
  });
});
