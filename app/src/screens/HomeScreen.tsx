import { useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type Ref } from "react";
import { ArrowRight, ArrowUpRight, Database, Search, Settings, X } from "lucide-react";
import { api, errorText, type Category, type ModelStatus, type Overview, type SearchHit, type SearchPage } from "../api";
import { CATEGORY_LABELS, describeIndexTotals } from "../format";
import { Button } from "../components/Button";
import { ContextMenu } from "../components/ContextMenu";
import { MonthPicker, TypePicker } from "../components/FilterPickers";
import { IndexStatus } from "../components/IndexStatus";
import { CATEGORY_ICONS, ResultGrid } from "../components/ResultGrid";
import { cx } from "../cx";
import logo from "../logo";
import { resultMenuGroups, type ResultActions } from "../resultMenu";
import { chooseDateEnd, chooseFileType, keepChosen, understandChosen, type Chosen, type Month, type Understood } from "../understand";
import { BACKGROUND_PRESS } from "../background";
import { MAIN_HIDDEN } from "../events";
import { isForSomethingElse } from "../useSelection";
import { useForwardedWheel } from "../useForwardedWheel";
import { useResultSelection } from "../useResultSelection";
import { useTauriEvent } from "../useTauriEvent";
import styles from "./HomeScreen.module.css";

const LICENSE = "MIT License";
const TOP_GAP_PX = 20;
const BOTTOM_GAP_PX = 16;
const CLEAR_RESULTS_MS = 500;
const SEARCH_DELAY_MS = 150;

const NO_HITS: SearchHit[] = [];
const NO_BATCHES: SearchHit[][] = [];

interface Found {
  id: number;
  batches: SearchHit[][];
  more: boolean;
}

export interface HomeHandle {
  restoreFocus: () => boolean;
  focusField: () => void;
}

interface Props {
  ref?: Ref<HomeHandle>;
  overview: Overview | null;
  model: ModelStatus | null;
  onRetryModel: () => void;
  onFocusChange?: (focused: boolean) => void;
  onOpenIndex: () => void;
  onOpenSettings: () => void;
  asWidget?: boolean;
  onDismiss?: () => void;
}

const snapToLayout = (px: number) => Math.round(px * 64) / 64;

function placeholder(model: ModelStatus | null, nothingIndexed: boolean): string {
  if (model?.state !== "failed" && nothingIndexed) return "Nothing to search yet";
  switch (model?.state) {
    case "verifying":
    case "starting":
      return "Loading model…";
    case "preparing":
      return "Preparing model…";
    case "failed":
      return "Search isn’t available. Click to try again.";
    default:
      return "Describe what you're looking for…";
  }
}

interface Open {
  what: "type" | "from" | "to";
  pill: HTMLElement;
}

function Token({ icon, ghost, open, label, onPress, children }: { icon?: ReactNode; ghost?: boolean; open: boolean; label: string; onPress: (pill: HTMLElement) => void; children: ReactNode }) {
  return (
    <button
      type="button"
      className={cx(styles.token, ghost && styles.ghost, open && styles.tokenOpen)}
      tabIndex={-1}
      aria-label={label}
      aria-haspopup="true"
      aria-expanded={open}
      onMouseDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
      onClick={(e) => onPress(e.currentTarget)}
    >
      {icon}
      {children}
    </button>
  );
}

function Understanding({ understood, query, onChooseType, onChooseDate }: { understood: Understood; query: string; onChooseType: (value: Category | null) => void; onChooseDate: (end: "from" | "to", month: Month | null) => void }) {
  const [open, setOpen] = useState<Open | null>(null);
  const close = useCallback(() => setOpen(null), []);
  const press = (what: Open["what"]) => (pill: HTMLElement) => setOpen((now) => (now?.pill === pill ? null : { what, pill }));
  useEffect(close, [query, close]);

  const TypeIcon = understood.fileType && CATEGORY_ICONS[understood.fileType];
  const dateEnd = (what: "from" | "to") => {
    const period = understood[what];
    const label = what === "from" ? "Change the start date" : "Change the end date";
    return period ? (
      <Token key={what} open={open?.what === what} label={label} onPress={press(what)}>
        {period.label}
      </Token>
    ) : (
      <Token key={what} ghost open={open?.what === what} label={label} onPress={press(what)}>
        Any time
      </Token>
    );
  };
  return (
    <div className={styles.parsed}>
      <div>
        <div className={styles.parsedLabel}>File type</div>
        <div className={styles.parsedValue}>
          {understood.fileType && TypeIcon ? (
            <Token icon={<TypeIcon size={12} aria-hidden />} open={open?.what === "type"} label="Change the file type" onPress={press("type")}>
              {CATEGORY_LABELS[understood.fileType]}
            </Token>
          ) : (
            <Token ghost open={open?.what === "type"} label="Change the file type" onPress={press("type")}>
              Any type
            </Token>
          )}
        </div>
      </div>
      <div>
        <div className={styles.parsedLabel}>Date range</div>
        <div className={styles.parsedValue}>
          {understood.from || understood.to ? (
            <>
              {dateEnd("from")}
              <ArrowRight className={styles.range} size={12} aria-hidden />
              {dateEnd("to")}
            </>
          ) : (
            dateEnd("from")
          )}
        </div>
      </div>
      <div>
        <div className={styles.parsedLabel}>Query</div>
        <div className={cx(styles.parsedValue, styles.parsedText, !understood.text && styles.parsedNone)}>
          {understood.text ? `“${understood.text}”` : "Anything"}
        </div>
      </div>
      {open?.what === "type" && <TypePicker anchor={open.pill} value={understood.fileType} onChoose={onChooseType} onClose={close} />}
      {(open?.what === "from" || open?.what === "to") && (
        <MonthPicker
          key={open.what}
          anchor={open.pill}
          title={open.what === "from" ? "Start date" : "End date"}
          value={understood[open.what]?.month ?? null}
          now={new Date()}
          onChoose={(month) => onChooseDate(open.what as "from" | "to", month)}
          onClose={close}
        />
      )}
    </div>
  );
}

export function HomeScreen({ ref, overview, model, onRetryModel, onFocusChange, onOpenIndex, onOpenSettings, asWidget, onDismiss }: Props) {
  const blocked = overview?.empty === true;
  const modelFailed = model?.state === "failed";
  const input = useRef<HTMLInputElement>(null);
  const home = useRef<HTMLElement>(null);
  const search = useRef<HTMLDivElement>(null);
  const resultsPane = useRef<HTMLDivElement>(null);
  const forwardWheel = useForwardedWheel(resultsPane);
  const actions = useRef<HTMLDivElement>(null);
  const row = useRef<HTMLDivElement>(null);
  const [attention, setAttention] = useState(0);
  const [focused, setFocused] = useState(false);
  const [query, setQuery] = useState("");
  const [chosen, setChosen] = useState<Chosen>({});
  const understood = useMemo(() => understandChosen(query, new Date(), chosen), [query, chosen]);
  const chooseType = useCallback((value: Category | null) => setChosen((now) => chooseFileType(now, query, new Date(), value)), [query]);
  const chooseDate = useCallback((end: "from" | "to", month: Month | null) => setChosen((now) => chooseDateEnd(now, query, new Date(), end, month)), [query]);
  const results = query.trim() !== "";
  const dim = focused && !results;
  const engaged = focused || results;
  const refuse = useCallback(() => setAttention((count) => count + 1), []);
  const text = understood.text.trim();
  const fileType = understood.fileType;
  const fromMs = understood.from?.start ?? null;
  const toMs = understood.to?.end ?? null;
  const searchable = model?.state === "ready" || model?.state === "preparing";
  const [found, setFound] = useState<Found | { error: string } | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [searching, setSearching] = useState(false);
  useEffect(() => {
    if (!results) {
      setSearching(false);
      const timer = setTimeout(() => setFound(null), CLEAR_RESULTS_MS);
      return () => clearTimeout(timer);
    }
    if (!searchable) return;
    let current = true;
    setSearching(true);
    const timer = setTimeout(() => {
      api
        .search(text, { category: fileType, from: fromMs, to: toMs })
        .then(
          (page: SearchPage) => current && setFound({ id: page.id, batches: [page.hits], more: page.more }),
          (error) => current && setFound({ error: errorText(error) }),
        )
        .finally(() => current && setSearching(false));
    }, SEARCH_DELAY_MS);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [results, searchable, text, fileType, fromMs, toMs]);
  const batches = found && "batches" in found ? found.batches : NO_BATCHES;
  const hits = useMemo(() => (batches.length === 0 ? NO_HITS : batches.flat()), [batches]);
  const viewMore = useCallback(() => {
    if (!found || !("id" in found) || loadingMore) return;
    const { id } = found;
    setLoadingMore(true);
    api
      .searchMore(id)
      .then((page) =>
        setFound((now) =>
          now && "id" in now && now.id === id ? { ...now, batches: [...now.batches, page.hits], more: page.more } : now,
        ),
      )
      .catch(() => {})
      .finally(() => setLoadingMore(false));
  }, [found, loadingMore]);

  const [barAway, setBarAway] = useState(false);
  const stowed = results && barAway;
  useEffect(() => setBarAway(false), [query]);
  useEffect(() => {
    if (focused) setBarAway(false);
  }, [focused]);
  const { selected, select, selectAll, clear: clearSelection } = useResultSelection(hits);
  const selectedHits = useMemo(() => hits.filter((hit) => selected.has(hit.path)), [hits, selected]);
  const resultActions = useMemo<ResultActions>(
    () => ({
      open: (hit) => {
        void api.openFile(hit.path).catch(() => {});
        onDismiss?.();
      },
      showInFolder: (hit) => {
        void api.showInFolder(hit.path).catch(() => {});
        onDismiss?.();
      },
      copyFiles: (files) => api.copyFiles(files.map((file) => file.path)),
      copyPaths: async (files) => {
        await navigator.clipboard.writeText(files.map((file) => file.path).join("\n"));
      },
    }),
    [onDismiss],
  );
  const [menu, setMenu] = useState<{ hits: SearchHit[]; x: number; y: number } | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  const openMenu = (hit: SearchHit, x: number, y: number) => {
    const inSelection = selected.has(hit.path);
    if (!inSelection) select(hit.path, false);
    setMenu({ hits: inSelection ? selectedHits : [hit], x, y });
  };
  useEffect(() => {
    if (!results) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const key = e.key.toLowerCase();
      if (key !== "a" && key !== "c") return;
      if (isForSomethingElse(e.target) || home.current?.closest("[inert]")) return;
      if (key === "a") {
        e.preventDefault();
        if (e.shiftKey) clearSelection();
        else selectAll();
      } else if (selectedHits.length > 0) {
        e.preventDefault();
        void (e.shiftKey ? resultActions.copyPaths(selectedHits) : resultActions.copyFiles(selectedHits)).catch(() => {});
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [results, selectedHits, selectAll, clearSelection, resultActions]);
  let message: string | null = null;
  if (!searchable) message = model?.state === "failed" ? "Search isn’t available." : "Waiting for the search model to load…";
  else if (found && "error" in found) message = `The search failed: ${found.error}`;
  else if (found && !searching && hits.length === 0) message = "Nothing found.";
  const setFocus = useCallback((next: boolean) => {
    setFocused(next);
    onFocusChange?.(next && query.trim() === "");
    if (!next && query.trim() === "") {
      setChosen({});
      onDismiss?.();
    }
  }, [query, onFocusChange, onDismiss]);
  const change = (text: string) => {
    setQuery(text);
    setChosen((now) => keepChosen(now, text));
    onFocusChange?.(focused && text.trim() === "");
  };
  const leave = useCallback(() => {
    away.current = false;
    setQuery("");
    setChosen({});
    if (asWidget && !blocked) {
      onFocusChange?.(true);
      setFocused(true);
      input.current?.focus({ preventScroll: true });
      return;
    }
    onFocusChange?.(false);
    setFocused(false);
    input.current?.blur();
  }, [onFocusChange, asWidget, blocked]);
  useEffect(() => {
    const onPress = () => {
      if (!home.current?.closest("[inert]")) clearSelection();
    };
    window.addEventListener(BACKGROUND_PRESS, onPress);
    return () => window.removeEventListener(BACKGROUND_PRESS, onPress);
  }, [clearSelection]);
  useTauriEvent(MAIN_HIDDEN, () => {
    leave();
    closeMenu();
  });
  const away = useRef(false);
  useImperativeHandle(ref, () => ({
    restoreFocus() {
      if (!away.current) return false;
      away.current = false;
      if (!input.current || input.current.readOnly) return false;
      input.current.focus({ preventScroll: true });
      return true;
    },
    focusField() {
      if (input.current && !input.current.readOnly) input.current.focus({ preventScroll: true });
    },
  }));
  const openIndexFromProgress = useCallback(() => {
    away.current = focused;
    onOpenIndex();
  }, [focused, onOpenIndex]);
  useEffect(() => {
    if (blocked) leave();
  }, [blocked, leave]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      if (home.current?.closest("[inert]") || document.querySelector("dialog[open], [role='menu']")) return;
      if (query !== "") {
        e.preventDefault();
        leave();
      } else if (document.activeElement === input.current) {
        input.current?.blur();
      } else if (asWidget) {
        onDismiss?.();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [query, leave, asWidget, onDismiss]);

  useLayoutEffect(() => {
    const root = home.current;
    if (!root) return;
    const measure = () => {
      if (!search.current || !actions.current || !row.current) return;
      const around = root.getBoundingClientRect();
      const block = search.current.getBoundingClientRect();
      const gap = parseFloat(getComputedStyle(row.current).columnGap) || 0;
      const actionsWidth = parseFloat(getComputedStyle(actions.current).width);
      const lift = Math.max(0, block.top - around.top - TOP_GAP_PX);
      const set = (name: string, px: number) => root.style.setProperty(name, `${px}px`);
      set("--top-gap", TOP_GAP_PX);
      set("--bottom-gap", BOTTOM_GAP_PX);
      set("--lift", lift);
      set("--drop", Math.max(0, around.bottom - BOTTOM_GAP_PX - block.bottom));
      set("--actions-width", snapToLayout(actionsWidth) + gap);
      set("--hole-left", block.left - around.left);
      set("--hole-right", around.right - block.right);
      set("--hole-top", block.top - around.top - lift);
      set("--home-height", around.height);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    observer.observe(actions.current!);
    return () => observer.disconnect();
  }, []);

  return (
    <main
      ref={home}
      className={cx(styles.home, engaged && styles.engaged, focused && styles.wide, dim && styles.dim, results && styles.results, stowed && styles.stowed, blocked && styles.blocked, asWidget && styles.asWidget, overview?.progress != null && styles.indexing)}
      onClick={(e) => {
        const target = e.target as HTMLElement;
        if (!target.closest(`button, input, label, [role='menu'], [data-selection-bar], .${styles.tagsBox}`)) clearSelection();
      }}
      onPointerDown={
        asWidget && onDismiss
          ? (e) => {
              const box = home.current?.querySelector(`.${styles.spot}`)?.getBoundingClientRect();
              if (!results && box && (e.clientX < box.left || e.clientX > box.right || e.clientY < box.top || e.clientY > box.bottom)) onDismiss();
            }
          : undefined
      }
    >
      {asWidget && <div className={styles.backdrop} data-widget-backdrop aria-hidden />}
      <div className={styles.scrim} aria-hidden />
      <ResultGrid
        paneRef={resultsPane}
        batches={batches}
        searchId={found && "id" in found ? found.id : 0}
        more={found !== null && "more" in found && found.more}
        loadingMore={loadingMore}
        onMore={viewMore}
        shown={results}
        busy={searching}
        message={message}
        progress={overview?.progress != null}
        selected={selected}
        selection={selectedHits}
        actions={resultActions}
        onClearSelection={clearSelection}
        onSelect={select}
        onOpen={resultActions.open}
        onContextMenu={openMenu}
        away={stowed}
        onAway={setBarAway}
      />
      <img className={styles.logo} src={logo} alt="Mesa" draggable={false} />
      <div
        ref={search}
        className={styles.search}
        onWheel={(e) => {
          if (results) forwardWheel(e);
        }}
      >
        <div ref={row} className={styles.row}>
          <div className={styles.spot} data-widget-spot aria-hidden />
          <div className={styles.halo} aria-hidden />
          <div className={styles.separator} aria-hidden />
          <label
            className={styles.field}
            onMouseDown={
              modelFailed
                ? (e) => { e.preventDefault(); onRetryModel(); }
                : blocked
                  ? (e) => { e.preventDefault(); refuse(); }
                  : (e) => { if (e.target !== input.current) e.preventDefault(); }
            }
            onClick={blocked || modelFailed ? (e) => e.preventDefault() : undefined}
          >
            <Search className={styles.searchIcon} size={18} aria-hidden />
            <input
              ref={input}
              className={styles.input}
              type="text"
              placeholder={placeholder(model, blocked)}
              title={model?.state === "failed" ? model.reason : undefined}
              aria-label="Search"
              readOnly={blocked}
              aria-disabled={blocked || undefined}
              tabIndex={blocked ? -1 : undefined}
              onFocus={blocked ? (e) => { e.currentTarget.blur(); refuse(); } : () => setFocus(true)}
              onBlur={() => { if (!blocked && document.hasFocus() && !away.current) setFocus(false); }}
              value={query}
              onChange={(e) => change(e.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
            {query !== "" && (
              <button type="button" className={styles.clear} aria-label="Clear search" title="Clear (Esc)" onClick={leave}>
                <X size={16} aria-hidden />
              </button>
            )}
          </label>
          <div
            className={styles.tagsBox}
            aria-hidden={!engaged}
            onMouseDown={(e) => {
              e.preventDefault();
              if (!blocked && !modelFailed) input.current?.focus();
            }}
          >
            <Understanding understood={understood} query={query} onChooseType={chooseType} onChooseDate={chooseDate} />
          </div>
          <div ref={actions} className={styles.actions}>
            <Button variant="primary" className={styles.indexButton} onClick={onOpenIndex}>
              <Database size={16} aria-hidden />
              Index
            </Button>
            <Button
              variant="neutral"
              className={styles.iconOnly}
              onClick={onOpenSettings}
              title="Settings"
              aria-label="Settings"
            >
              <Settings size={18} aria-hidden />
            </Button>
          </div>
        </div>
        <div className={styles.status}>
          <IndexStatus overview={overview} onOpenIndex={openIndexFromProgress} attention={attention} raised={dim} />
        </div>
      </div>
      <footer className={styles.footer}>
        <span className={styles.totals}>
          {overview && describeIndexTotals(overview.files, overview.bytes)}
        </span>
        <span className={styles.legal}>
          <span>{LICENSE}</span>
          <span className={styles.divider} aria-hidden />
          <button type="button" className={styles.project} onClick={() => api.openProjectPage().catch(() => {})}>
            GitHub
            <ArrowUpRight size={12} aria-hidden />
          </button>
        </span>
      </footer>
      {menu && (
        <ContextMenu
          key={`${menu.hits[0].path}:${menu.x}:${menu.y}`}
          x={menu.x}
          y={menu.y}
          groups={resultMenuGroups(menu.hits, resultActions)}
          onClose={closeMenu}
        />
      )}
    </main>
  );
}
