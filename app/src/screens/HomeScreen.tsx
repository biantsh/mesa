import { useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type Ref } from "react";
import { ArrowRight, Ban, Database, Files, Search, Settings, X } from "lucide-react";
import { api, errorText, type Category, type ModelStatus, type Overview, type SearchHit, type SearchPage } from "../api";
import { CATEGORY_LABELS } from "../format";
import { Button } from "../components/Button";
import { ContextMenu } from "../components/ContextMenu";
import { ExampleButton } from "../components/ExampleButton";
import { MonthPicker, TypePicker } from "../components/FilterPickers";
import { IndexStatus } from "../components/IndexStatus";
import { SlidingIcon } from "../components/SlidingIcon";
import { CATEGORY_ICONS, ResultGrid } from "../components/ResultGrid";
import { cx } from "../cx";
import { fitQuoted } from "../fitText";
import { addExamples, countByKind, categoryOfPath, distinctNames, dropHint, dropIcon, shortName } from "../examples";
import logo from "../logo";
import { resultMenuGroups, type ResultActions } from "../resultMenu";
import { chooseDateEnd, chooseFileType, keepChosen, understandChosen, type Chosen, type Month, type Understood } from "../understand";
import { BACKGROUND_PRESS } from "../background";
import { MAIN_HIDDEN } from "../events";
import { scrollByKey } from "../scrollKeys";
import { ShortcutTip } from "../components/ShortcutTip";
import { useDefaultScroll } from "../useDefaultScroll";
import { useFileDrop } from "../useFileDrop";
import { isForSomethingElse } from "../useSelection";
import { useForwardedWheel } from "../useForwardedWheel";
import { useResultSelection } from "../useResultSelection";
import { useShortcutTip } from "../useShortcutTip";
import { useTauriEvent } from "../useTauriEvent";
import styles from "./HomeScreen.module.css";

const TOP_GAP_PX = 20;
const BOTTOM_GAP_PX = 16;
const CORNER_TEXT_RISE_PX = -2;
const ARRIVING_SNAP_MS = 300;
const BOTTOM_GAP_WITH_CORNERS_PX = 36;
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
  hasText: () => boolean;
  focusFieldAtOnce: () => void;
  focusField: () => void;
}

interface Props {
  ref?: Ref<HomeHandle>;
  overview: Overview | null;
  model: ModelStatus | null;
  onRetryModel: () => void;
  onFocusChange?: (focused: boolean) => void;
  onBottomPill?: (width: number | null) => void;
  onOpenIndex: () => void;
  onOpenSettings: () => void;
  onScreen?: boolean;
  indexShown?: boolean;
  asWidget?: boolean;
  onDismiss?: () => void;
}

const FIELD_ICONS = { search: Search, ...CATEGORY_ICONS, files: Files, refuse: Ban };

const snapToLayout = (px: number) => Math.round(px * 64) / 64;

function placeholder(model: ModelStatus | null, nothingIndexed: boolean, dropping: string | null): string {
  if (dropping) return dropping;
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

function Token({ icon, ghost, open, label, className, tooltip, onPress, children }: { icon?: ReactNode; ghost?: boolean; open: boolean; label: string; className?: string; tooltip?: string; onPress: (pill: HTMLElement) => void; children: ReactNode }) {
  return (
    <button
      type="button"
      className={cx(styles.token, ghost && styles.ghost, open && styles.tokenOpen, className)}
      data-tooltip={tooltip}
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

let measuring: CanvasRenderingContext2D | null | undefined;
const measuringContext = () => (measuring === undefined ? (measuring = document.createElement("canvas").getContext("2d")) : measuring);

function QuotedText({ text }: { text: string }) {
  const box = useRef<HTMLSpanElement>(null);
  const [shown, setShown] = useState(`“${text}”`);
  useLayoutEffect(() => {
    const node = box.current;
    if (!node) return;
    const canvas = measuringContext();
    const fit = () => {
      if (!canvas) return setShown(`“${text}”`);
      canvas.font = getComputedStyle(node).font;
      setShown(fitQuoted(text, node.clientWidth - 1, (piece) => canvas.measureText(piece).width));
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(node);
    return () => observer.disconnect();
  }, [text]);
  return (
    <span ref={box} className={styles.quoted}>
      {shown}
    </span>
  );
}

const MAX_NAMES_IN_TIP = 8;

function examplesTooltip(names: string[]): string {
  if (names.length === 1) return `Searching with ${names[0]}`;
  const lines = names.slice(0, MAX_NAMES_IN_TIP).map((name) => `• ${name}`);
  if (names.length > MAX_NAMES_IN_TIP) lines.push(`and ${names.length - MAX_NAMES_IN_TIP} more`);
  return ["Searching with:", ...lines].join("\n");
}

interface ExamplesPillProps {
  examples: string[];
  fieldFocused: boolean;
  onFocusField: () => void;
  onRemove: (path: string) => void;
  onClear: () => void;
}

function ExamplesPill({ examples, fieldFocused, onFocusField, onRemove, onClear }: ExamplesPillProps) {
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const fieldHadFocus = useRef(false);
  const closeMenu = () => {
    setMenu(null);
    if (fieldHadFocus.current) onFocusField();
  };
  const names = useMemo(() => distinctNames(examples), [examples]);
  const tooltip = useMemo(() => examplesTooltip(names), [names]);
  const kinds = useMemo(() => countByKind(examples), [examples]);
  return (
    <>
      <Token
        className={styles.examples}
        open={menu !== null}
        label={`Searching with ${examples.length} ${examples.length === 1 ? "file" : "files"}. Press to change.`}
        tooltip={tooltip}
        onPress={(pill) => {
          if (menu) return closeMenu();
          const box = pill.getBoundingClientRect();
          fieldHadFocus.current = fieldFocused;
          setMenu({ x: box.left, y: box.bottom + 6 });
        }}
      >
        {kinds.map(({ category, count }) => {
          const Icon = CATEGORY_ICONS[category];
          return (
            <span key={category} className={styles.exampleKind}>
              {count}
              <Icon size={12} aria-hidden />
            </span>
          );
        })}
      </Token>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={closeMenu}
          groups={[
            names.map((name, i) => {
              const category = categoryOfPath(examples[i]);
              const Icon = category && CATEGORY_ICONS[category];
              return {
                label: shortName(name, 32),
                ariaLabel: `Remove ${name}`,
                icon: Icon ? <Icon size={16} aria-hidden /> : undefined,
                trailing: <X size={14} aria-hidden />,
                onSelect: () => onRemove(examples[i]),
              };
            }),
            ...(examples.length > 1 ? [[{ label: "Remove all", onSelect: onClear }]] : []),
          ]}
        />
      )}
    </>
  );
}

function Understanding({ understood, query, examples, fieldFocused, onFocusField, onChooseType, onChooseDate, onRemoveExample, onClearExamples }: { understood: Understood; query: string; examples: string[]; fieldFocused: boolean; onFocusField: () => void; onChooseType: (value: Category | null) => void; onChooseDate: (end: "from" | "to", month: Month | null) => void; onRemoveExample: (path: string) => void; onClearExamples: () => void }) {
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
    <div className={cx(styles.parsed, examples.length > 0 && styles.parsedWithFiles)}>
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
          {understood.text ? <QuotedText text={understood.text} /> : "Anything"}
        </div>
      </div>
      {examples.length > 0 && (
        <div>
          <ExamplesPill examples={examples} fieldFocused={fieldFocused} onFocusField={onFocusField} onRemove={onRemoveExample} onClear={onClearExamples} />
        </div>
      )}
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

export function HomeScreen({ ref, overview, model, onRetryModel, onFocusChange, onBottomPill, onOpenIndex, onOpenSettings, onScreen = false, indexShown = false, asWidget, onDismiss }: Props) {
  const blocked = overview?.empty === true;
  const modelFailed = model?.state === "failed";
  const input = useRef<HTMLInputElement>(null);
  const field = useRef<HTMLLabelElement>(null);
  const tagsBox = useRef<HTMLDivElement>(null);
  const home = useRef<HTMLElement>(null);
  const search = useRef<HTMLDivElement>(null);
  const resultsPane = useRef<HTMLDivElement>(null);
  const forwardWheel = useForwardedWheel(resultsPane);
  useDefaultScroll(true, () => (!results || home.current?.closest("[inert]") ? null : resultsPane.current));
  const scrollResultsByKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.ctrlKey || e.altKey || e.metaKey || e.shiftKey || e.nativeEvent.isComposing) return;
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown" && e.key !== "PageUp" && e.key !== "PageDown") return;
    const pane = resultsPane.current;
    if (results && pane && scrollByKey(pane, e.key, e.repeat)) e.preventDefault();
  };
  const actions = useRef<HTMLDivElement>(null);
  const row = useRef<HTMLDivElement>(null);
  const [attention, setAttention] = useState(0);
  const [focused, setFocused] = useState(false);
  const [query, setQuery] = useState("");
  const [examples, setExamples] = useState<string[]>([]);
  const [exampleNotice, setExampleNotice] = useState<string | null>(null);
  const addExample = useCallback((paths: string[]) => {
    setExamples((now) => addExamples(now, paths));
    setExampleNotice(null);
    for (const path of paths) {
      api.prepareExample(path).catch((error) => {
        setExamples((now) => now.filter((one) => one !== path));
        setExampleNotice(errorText(error));
      });
    }
  }, []);
  const removeExample = useCallback((path: string) => setExamples((now) => now.filter((one) => one !== path)), []);
  const clearExamples = useCallback(() => setExamples([]), []);
  const dragging = useFileDrop({
    enabled: !blocked && !modelFailed,
    isActive: () => !home.current?.closest("[inert]"),
    targets: () => [field.current, focused ? tagsBox.current : null],
    onDrop: addExample,
  });
  const dropping = dragging?.files ?? null;
  const focusField = useCallback(() => input.current?.focus({ preventScroll: true }), []);
  const [chosen, setChosen] = useState<Chosen>({});
  const understood = useMemo(() => understandChosen(query, new Date(), chosen), [query, chosen]);
  const chooseType = useCallback((value: Category | null) => setChosen((now) => chooseFileType(now, query, new Date(), value)), [query]);
  const chooseDate = useCallback((end: "from" | "to", month: Month | null) => setChosen((now) => chooseDateEnd(now, query, new Date(), end, month)), [query]);
  const results = query.trim() !== "" || examples.length > 0 || exampleNotice !== null;
  const filled = query !== "" || examples.length > 0 || exampleNotice !== null;
  const dim = focused && !results;
  const engaged = focused || results;
  const [fieldHasBeenFocused, setFieldHasBeenFocused] = useState(false);
  if (focused && !fieldHasBeenFocused) setFieldHasBeenFocused(true);
  const opening = asWidget && !blocked && !fieldHasBeenFocused;
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
        .search(text, { category: fileType, from: fromMs, to: toMs }, examples)
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
  }, [results, searchable, text, fileType, fromMs, toMs, examples]);
  const batches = found && "batches" in found ? found.batches : NO_BATCHES;
  const hits = useMemo(() => (batches.length === 0 ? NO_HITS : batches.flat()), [batches]);
  const shortcutTip = useShortcutTip({ onScreen: onScreen && !asWidget, worth: hits.length > 0 && !searching });
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
  if (exampleNotice) message = exampleNotice;
  else if (!searchable) message = model?.state === "failed" ? "Search isn’t available." : "Waiting for the search model to load…";
  else if (found && "error" in found) message = `The search failed: ${found.error}`;
  else if (found && !searching && hits.length === 0) message = "Nothing found.";
  const setFocus = useCallback((next: boolean) => {
    setFocused(next);
    const empty = query.trim() === "" && examples.length === 0;
    onFocusChange?.(next && empty);
    if (!next && empty) {
      setChosen({});
      onDismiss?.();
    }
  }, [query, examples, onFocusChange, onDismiss]);
  const change = (text: string) => {
    setQuery(text);
    setExampleNotice(null);
    setChosen((now) => keepChosen(now, text));
    onFocusChange?.(focused && text.trim() === "" && examples.length === 0);
  };
  const leave = useCallback(() => {
    away.current = false;
    setQuery("");
    setExamples([]);
    setExampleNotice(null);
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
  const [snapping, setSnapping] = useState(false);
  const snapTimer = useRef(0);
  useEffect(() => () => window.clearTimeout(snapTimer.current), []);
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
    focusFieldAtOnce() {
      setSnapping(true);
      window.clearTimeout(snapTimer.current);
      snapTimer.current = window.setTimeout(() => setSnapping(false), ARRIVING_SNAP_MS);
      if (input.current && !input.current.readOnly) input.current.focus({ preventScroll: true });
    },
    hasText() {
      return query.trim() !== "" || examples.length > 0;
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
      if (filled) {
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
  }, [filled, leave, asWidget, onDismiss]);

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
      const bottomGap = asWidget ? BOTTOM_GAP_PX : BOTTOM_GAP_WITH_CORNERS_PX;
      const cornerBottom = parseFloat(getComputedStyle(root).getPropertyValue("--corner-bottom")) || 10;
      const pillBottom = asWidget ? BOTTOM_GAP_PX : cornerBottom + CORNER_TEXT_RISE_PX;
      set("--bottom-gap", bottomGap);
      set("--pill-bottom", pillBottom);
      set("--lift", lift);
      set("--drop", Math.max(0, around.bottom - pillBottom - block.bottom));
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
  }, [asWidget]);

  const pillAtBottom = engaged && !asWidget && overview?.progress != null;
  const wasAtBottom = useRef(false);
  const fading = indexShown && wasAtBottom.current;
  useLayoutEffect(() => {
    if (!indexShown) wasAtBottom.current = pillAtBottom;
  });
  useEffect(() => {
    const pill = pillAtBottom ? home.current?.querySelector<HTMLElement>("[data-pill]") : null;
    if (!pill) {
      onBottomPill?.(null);
      return;
    }
    const report = () => onBottomPill?.(pill.offsetWidth);
    report();
    const observer = new ResizeObserver(report);
    observer.observe(pill);
    return () => observer.disconnect();
  }, [pillAtBottom, onBottomPill]);

  return (
    <main
      ref={home}
      data-fading={fading ? "" : undefined}
      data-snapping={snapping ? "" : undefined}
      className={cx(styles.home, engaged && styles.engaged, focused && styles.wide, dim && styles.dim, results && styles.results, stowed && styles.stowed, blocked && styles.blocked, asWidget && styles.asWidget, opening && styles.opening, overview?.progress != null && styles.indexing)}
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
        <div
          ref={row}
          className={cx(styles.row, dropping && (dropping.supported.length > 0 ? styles.dropping : styles.refusing), dragging?.over && styles.hot)}
        >
          <div className={styles.spot} data-widget-spot aria-hidden />
          <div className={styles.halo} aria-hidden />
          <div className={styles.separator} aria-hidden />
          <label
            ref={field}
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
            <span className={styles.searchIcon}>
              <SlidingIcon icons={FIELD_ICONS} current={dropIcon(dropping)} />
            </span>
            <input
              ref={input}
              className={styles.input}
              type="text"
              placeholder={placeholder(model, blocked, dropping && dropHint(dropping.supported, dropping.unsupported, dropping.folders))}
              data-tooltip={model?.state === "failed" ? model.reason : undefined}
              aria-label="Search"
              readOnly={blocked}
              aria-disabled={blocked || undefined}
              tabIndex={blocked ? -1 : undefined}
              onFocus={blocked ? (e) => { e.currentTarget.blur(); refuse(); } : () => { away.current = false; setFocus(true); }}
              onBlur={() => { if (!blocked && document.hasFocus() && !away.current) setFocus(false); }}
              value={query}
              onChange={(e) => change(e.target.value)}
              onKeyDown={scrollResultsByKey}
              spellCheck={false}
              autoComplete="off"
            />
            {filled && (
              <button type="button" className={cx(styles.clear, dragging && styles.away)} aria-label="Clear search" data-tooltip="Clear (Esc)" onClick={leave}>
                <X size={18} aria-hidden />
              </button>
            )}
            {!blocked && !modelFailed && <ExampleButton className={styles.attach} onPick={addExample} hidden={dragging !== null} calmFor={`${focused}|${results}|${engaged}`} />}
          </label>
          <div
            ref={tagsBox}
            className={styles.tagsBox}
            aria-hidden={!engaged}
            onMouseDown={(e) => {
              e.preventDefault();
              if (!blocked && !modelFailed) input.current?.focus();
            }}
          >
            <Understanding understood={understood} query={query} examples={examples} fieldFocused={focused} onFocusField={focusField} onChooseType={chooseType} onChooseDate={chooseDate} onRemoveExample={removeExample} onClearExamples={clearExamples} />
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
              data-tooltip="Settings"
              aria-label="Settings"
            >
              <Settings size={18} aria-hidden />
            </Button>
          </div>
        </div>
        <div className={styles.status}>
          <IndexStatus overview={overview} onOpenIndex={openIndexFromProgress} attention={attention} raised={dim} keepsFocus={focused} />
        </div>
      </div>
      <ShortcutTip show={shortcutTip.visible} keys={shortcutTip.keys} lifted={overview?.progress != null} />
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
