import { useQuery } from "@tanstack/react-query";
import { PackagePlus, Search } from "lucide-react";
import { useState } from "react";
import { api } from "../lib/api";
import { expiryTone, fmtDate, fmtMoney, fmtNum, TONE_TEXT } from "../lib/format";
import { AddStockForm } from "../components/AddStockForm";
import { EmptyState, ErrorState, Skeleton } from "../components/ui";

/**
 * Receive stock — its own screen, not a dialog behind the sell page.
 *
 * Receiving and selling are opposite operations and deserve opposite surfaces.
 * Left lists recent stock so "top up what I bought last" needs no search;
 * right holds the batch form. Form validates inline on blur, never per keystroke.
 */
export function AddStock() {
  const [tab, setTab] = useState<"existing" | "new">("existing");
  const [query, setQuery] = useState("");

  // Recent batches make the target obvious without searching: most receiving is "top up the
  // thing I bought last", not "find product 7,412 in a 10k catalogue".
  const recent = useQuery({
    queryKey: ["stock", "", "all", "all", "name"],
    queryFn: () => api.stockList({ sort: "name", limit: 1000 }),
  });

  const term = query.trim().toLowerCase();
  const rows = (recent.data?.rows ?? []).filter((r) => {
    if (!term) return true;
    return (
      r.name.toLowerCase().includes(term) ||
      (r.sku ?? "").toLowerCase().includes(term) ||
      (r.category ?? "").toLowerCase().includes(term)
    );
  });

  if (recent.isError) {
    return (
      <ErrorState
        message={(recent.error as Error).message}
        onRetry={() => void recent.refetch()}
      />
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="scroll-y min-h-0 flex-1">
        <div className="mx-auto grid max-w-6xl grid-cols-1 gap-4 p-5 lg:grid-cols-[minmax(0,1fr)_380px]">
          {/* ── Pick the target ────────────────────────────────────────────── */}
          <section className="card flex min-h-0 flex-col overflow-hidden">
            <header className="shrink-0 border-b border-[var(--hairline)] px-4 py-3">
              <h2 className="title-tight text-[12.5px] font-[590] text-ink">
                Which product?
              </h2>
              <p className="mt-0.5 text-[11.5px] text-muted">
                Add a batch to something already in your catalogue, or create a new product on
                the next tab.
              </p>
              <div className="relative mt-3">
                <Search
                  size={14}
                  className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-faint"
                  strokeWidth={1.9}
                />
                <input
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder="Search name, SKU or category…"
                  className="input !pl-8"
                  aria-label="Search products to receive stock for"
                />
              </div>
            </header>

            <div className="scroll-y min-h-0 flex-1">
              {recent.isLoading ? (
                <table className="tbl">
                  <thead>
                    <tr>
                      <th>Product</th>
                      <th>Earliest expiry</th>
                      <th className="num">On hand</th>
                      <th className="num">Batches</th>
                      <th className="num">Avg cost</th>
                    </tr>
                  </thead>
                  <tbody aria-hidden>
                    {Array.from({ length: 8 }).map((_, r) => (
                      <tr key={r}>
                        <td>
                          <div className="flex items-center gap-2">
                            <Skeleton h={9} w={112} />
                            <Skeleton h={9} w={54} />
                          </div>
                        </td>
                        <td>
                          <Skeleton h={9} w="62%" />
                        </td>
                        <td>
                          <div className="flex justify-end">
                            <Skeleton h={9} w={46} />
                          </div>
                        </td>
                        <td>
                          <div className="flex justify-end">
                            <Skeleton h={9} w={24} />
                          </div>
                        </td>
                        <td>
                          <div className="flex justify-end">
                            <Skeleton h={9} w={52} />
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              ) : rows.length === 0 ? (
                <EmptyState
                  icon={<PackagePlus size={19} strokeWidth={1.9} />}
                  title={term ? "Nothing matches" : "Your catalogue is empty"}
                  body={
                    term
                      ? "No product matches that search. Switch to the New product tab to create it."
                      : "Create your first product on the New product tab, then receive a batch against it."
                  }
                />
              ) : (
                <table className="tbl">
                  <colgroup>
                    <col />
                    <col style={{ width: 116 }} />
                    <col style={{ width: 92 }} />
                    <col style={{ width: 104 }} />
                    <col style={{ width: 104 }} />
                  </colgroup>
                  <thead>
                    <tr>
                      <th>Product</th>
                      <th>Earliest expiry</th>
                      <th className="num">On hand</th>
                      <th className="num">Batches</th>
                      <th className="num">Avg cost</th>
                    </tr>
                  </thead>
                  <tbody>
                    {rows.map((r) => {
                      const tone = expiryTone(r.earliestExpiry);
                      return (
                        <tr
                          key={r.id}
                          onClick={() => setTab("existing")}
                          className="cursor-default"
                        >
                          <td className="max-w-0">
                            <div className="flex items-center gap-2">
                              <span className="min-w-0 truncate font-[510] text-ink">
                                {r.name}
                              </span>
                              {r.sku ? (
                                <span className="shrink-0 font-mono text-[10.5px] text-faint">
                                  {r.sku}
                                </span>
                              ) : null}
                            </div>
                          </td>
                          <td className={`tnum text-[12px] ${TONE_TEXT[tone.tone]}`}>
                            <span className="block truncate">
                              {r.earliestExpiry ? fmtDate(r.earliestExpiry) : "No expiry"}
                            </span>
                          </td>
                          <td className="num tnum font-[510] text-ink">
                            {fmtNum(r.totalQuantity)}
                            <span className="ml-1 text-[10.5px] text-faint">{r.unit}</span>
                          </td>
                          <td className="num tnum">{r.batchCount}</td>
                          <td className="num tnum">{fmtMoney(r.avgCost, true)}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              )}
            </div>
          </section>

          {/* ── Enter the batch ────────────────────────────────────────────── */}
          <section className="card card-pad h-fit lg:sticky lg:top-5">
            <AddStockForm
              mode={tab}
              onModeChange={setTab}
              onDone={() => {
                setQuery("");
                void recent.refetch();
              }}
            />
          </section>
        </div>
      </div>
    </div>
  );
}
