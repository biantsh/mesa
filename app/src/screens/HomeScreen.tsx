import { useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode, type Ref } from "react";
import { ArrowRight, Ban, CircleDashed, Database, Files, Search, Settings, X } from "lucide-react";
import { api, errorText, type Category, type ExampleProgress, type ModelStatus, type Overview, type SearchHit, type SearchPage } from "../api";
import { CATEGORY_LABELS } from "../format";
import { Button } from "../components/Button";
import { ContextMenu } from "../components/ContextMenu";
import { ExampleButton } from "../components/ExampleButton";
import { MonthPicker, TypePicker } from "../components/FilterPickers";
import { IndexStatus } from "../components/IndexStatus";
import { SlidingIcon } from "../components/SlidingIcon";
import { CATEGORY_ICONS, MessageLink, ResultGrid, type ResultMessage } from "../components/ResultGrid";
import { ResultPanel } from "../components/ResultPanel";
import { codeLine, prefetchDetails } from "../matchDetails";
import { cx } from "../cx";
import { fitQuoted } from "../fitText";
import { MAX_EXAMPLES, addExamples, countByKind, categoryOfPath, distinctNames, dropHint, dropIcon, leftOut, limitNotice, shortName } from "../examples";
import logo from "../logo";
import { resultMenuGroups, type ResultActions } from "../resultMenu";
import { DEFAULT_PANEL_COLUMNS, PANEL_COLUMNS, panelBeside, panelLayout, panelRoom, panelWidth, previewHeight, resultColumns } from "../resultWindow";
import { chooseDateEnd, chooseFileType, keepChosen, setCodeProjects, understandChosen, type Chosen, type Month, type Understood } from "../understand";
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
import { useIndexedTypes } from "../useIndexedTypes";
import { useTauriEvent } from "../useTauriEvent";
import styles from "./HomeScreen.module.css";

const FAILURE_MS = 8000;
const TOP_GAP_PX = 20;
const BOTTOM_GAP_PX = 8;
const PROGRESS_BOTTOM_PX = 4;
const ARRIVING_SNAP_MS = 300;
const BOTTOM_GAP_WITH_CORNERS_PX = 36;
const CLEAR_RESULTS_MS = 500;
const PANEL_COLUMNS_KEY = "mesa.resultPanelColumns";

function storedPanelColumns(): number {
  try {
    const columns = Number(localStorage.getItem(PANEL_COLUMNS_KEY));
    return Number.isInteger(columns) && columns > 0 ? columns : DEFAULT_PANEL_COLUMNS;
  } catch {
    return DEFAULT_PANEL_COLUMNS;
  }
}

function keepPanelColumns(columns: number) {
  try {
    localStorage.setItem(PANEL_COLUMNS_KEY, String(columns));
  } catch {
  }
}

const PANEL_GAP = 6;
const SIDE_MARGIN = 24;
const MOST_RESULTS_WIDTH = 968;
const WIDGET_COLUMNS = 6;
const SEARCH_DELAY_MS = 150;

const NO_HITS: SearchHit[] = [];
const NO_BATCHES: SearchHit[][] = [];

interface Found {
  id: number;
  batches: SearchHit[][];
  more: boolean;
  moreClose: boolean;
  stalled?: boolean;
  weakFrom: number | null;
  text: string;
  filtered: boolean;
}

const MAX_QUOTED = 80;
const NOT_INDEXED: Record<Category, string> = {
  image: "Images aren’t indexed",
  video: "Videos aren’t indexed",
  audio: "Audio isn’t indexed",
  document: "Documents aren’t indexed",
  code: "Code isn’t indexed",
};

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

const LIMIT_NOTE_MS = 5000;

const FIELD_ICONS = { search: Search, ...CATEGORY_ICONS, files: Files, refuse: Ban };

const snapToLayout = (px: number) => Math.round(px * 64) / 64;

function placeholder(model: ModelStatus | null, nothingIndexed: boolean, dropping: string | null): string {
  if (dropping) return dropping;
  if (model?.state !== "failed" && nothingIndexed) return "Nothing to search yet";
  switch (model?.state) {
    case "verifying":
    case "finalizing":
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

function codeDetail(understood: Understood): string {
  const parts = [understood.language, understood.project, understood.tests ? "tests" : null];
  return parts.filter(Boolean).join(" · ");
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

const TYPE_SIZER = (
  <span className={styles.typeSizer} aria-hidden>
    <span className={cx(styles.token, styles.ghost)}>Any type</span>
    {(Object.keys(CATEGORY_LABELS) as Category[]).map((type) => {
      const Icon = CATEGORY_ICONS[type];
      return (
        <span key={type} className={styles.token}>
          <Icon size={12} aria-hidden />
          {CATEGORY_LABELS[type]}
        </span>
      );
    })}
  </span>
);

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
  progress?: number | null;
  fieldFocused: boolean;
  onFocusField: () => void;
  onRemove: (path: string) => void;
  onClear: () => void;
}

function ExamplesPill({ examples, progress = null, fieldFocused, onFocusField, onRemove, onClear }: ExamplesPillProps) {
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
        label={`Searching with ${examples.length} ${examples.length === 1 ? "file" : "files"}${progress !== null ? ", still reading them" : ""}. Press to change.`}
        tooltip={tooltip}
        onPress={(pill) => {
          if (menu) return closeMenu();
          const box = pill.getBoundingClientRect();
          fieldHadFocus.current = fieldFocused;
          setMenu({ x: box.left, y: box.bottom + 6 });
        }}
      >
        {progress !== null && <FillingRing value={progress} />}
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

function FillingRing({ value }: { value: number }) {
  const circumference = 2 * Math.PI * 5;
  return (
    <svg className={styles.fillingRing} width={12} height={12} viewBox="0 0 12 12" aria-hidden>
      <circle cx={6} cy={6} r={5} className={styles.fillingTrack} />
      <circle cx={6} cy={6} r={5} className={styles.fillingValue} strokeDasharray={circumference} strokeDashoffset={circumference * (1 - Math.min(1, Math.max(0, value)))} />
    </svg>
  );
}

function Understanding({ understood, indexedTypes, query, examples, examplesProgress, limitNote, fieldFocused, onFocusField, onChooseType, onChooseDate, onRemoveExample, onClearExamples }: { understood: Understood; indexedTypes: Category[] | null; query: string; examples: string[]; examplesProgress: number | null; limitNote: string | null; fieldFocused: boolean; onFocusField: () => void; onChooseType: (value: Category | null) => void; onChooseDate: (end: "from" | "to", month: Month | null) => void; onRemoveExample: (path: string) => void; onClearExamples: () => void }) {
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
        <div className={cx(styles.parsedValue, styles.typeSlot)}>
          {TYPE_SIZER}
          {understood.fileType && TypeIcon ? (
            <Token icon={<TypeIcon size={12} aria-hidden />} open={open?.what === "type"} label="Change the file type" onPress={press("type")}>
              {CATEGORY_LABELS[understood.fileType]}
              {understood.fileType === "code" && codeDetail(understood) && ` · ${codeDetail(understood)}`}
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
          <ExamplesPill examples={examples} progress={examplesProgress} fieldFocused={fieldFocused} onFocusField={onFocusField} onRemove={onRemoveExample} onClear={onClearExamples} />
          {limitNote && (
            <span className={styles.limitNote} role="status">
              {limitNote}
            </span>
          )}
        </div>
      )}

      {open?.what === "type" && <TypePicker anchor={open.pill} value={understood.fileType} indexed={indexedTypes} onChoose={onChooseType} onClose={close} />}
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
  const [failure, setFailure] = useState<string | null>(null);
  const examplesNow = useRef(examples);
  examplesNow.current = examples;
  const fieldNow = useRef({ focused, query, onFocusChange });
  fieldNow.current = { focused, query, onFocusChange };
  const tellOfFiles = useCallback((files: string[]) => {
    const { focused, query, onFocusChange } = fieldNow.current;
    onFocusChange?.(focused && query.trim() === "" && files.length === 0);
  }, []);
  const [limitNote, setLimitNote] = useState<string | null>(null);
  useEffect(() => {
    if (!limitNote) return;
    const timer = setTimeout(() => setLimitNote(null), LIMIT_NOTE_MS);
    return () => clearTimeout(timer);
  }, [limitNote]);
  const addExample = useCallback((paths: string[], replace = false) => {
    const now = replace ? [] : examplesNow.current;
    const next = addExamples(now, paths);
    const over = leftOut(now, paths);
    setExamples(next);
    tellOfFiles(next);
    setExampleNotice(null);
    setLimitNote(over > 0 ? limitNotice(over) : null);
    for (const path of next.filter((path) => !now.includes(path))) {
      api.prepareExample(path).catch((error) => {
        setExamples((now) => now.filter((one) => one !== path));
        tellOfFiles(examplesNow.current.filter((one) => one !== path));
        setExampleNotice(errorText(error));
      });
    }
  }, [tellOfFiles]);
  const [examplesProgress, setExamplesProgress] = useState<number | null>(null);
  const [filledIn, setFilledIn] = useState(0);
  useEffect(() => {
    if (examples.length === 0) {
      setExamplesProgress(null);
      return;
    }
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let wasReading = false;
    const ask = () =>
      api.exampleProgress(examples).then(
        (list: (ExampleProgress | null)[]) => {
          if (!live) return;
          const reading = list.some((one) => one === null || one.reading);
          const made = list.reduce((sum, one) => sum + (one?.made ?? 0), 0);
          const of = list.reduce((sum, one) => sum + (one?.of ?? 0), 0);
          setExamplesProgress(reading ? (of > 0 ? made / of : 0) : null);
          if (wasReading && !reading) setFilledIn((n) => n + 1);
          wasReading = reading;
          if (reading) timer = setTimeout(ask, 300);
        },
        () => {},
      );
    void ask();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [examples]);
  const removeExample = useCallback((path: string) => {
    setExamples((now) => now.filter((one) => one !== path));
    tellOfFiles(examplesNow.current.filter((one) => one !== path));
    setLimitNote(null);
  }, [tellOfFiles]);
  const clearExamples = useCallback(() => {
    setExamples([]);
    tellOfFiles([]);
    setLimitNote(null);
  }, [tellOfFiles]);
  const dragging = useFileDrop({
    enabled: !blocked && !modelFailed,
    isActive: () => !home.current?.closest("[inert]"),
    targets: () => [field.current, focused ? tagsBox.current : null],
    onDrop: addExample,
  });
  const dropping = dragging?.files ?? null;
  const room = MAX_EXAMPLES - examples.length;
  const focusField = useCallback(() => input.current?.focus({ preventScroll: true }), []);
  const [chosen, setChosen] = useState<Chosen>({});
  const [projectsRead, setProjectsRead] = useState(0);
  useEffect(() => {
    if (!focused) return;
    let live = true;
    api.codeProjects().then(
      (names) => {
        if (!live) return;
        setCodeProjects(names);
        setProjectsRead((n) => n + 1);
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [focused]);
  const indexedTypes = useIndexedTypes(focused).types;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const understood = useMemo(() => understandChosen(query, new Date(), chosen), [query, chosen, projectsRead]);
  const chooseType = useCallback((value: Category | null) => setChosen((now) => chooseFileType(now, query, new Date(), value)), [query]);
  const chooseDate = useCallback((end: "from" | "to", month: Month | null) => setChosen((now) => chooseDateEnd(now, query, new Date(), end, month)), [query]);
  const results = query.trim() !== "" || examples.length > 0 || exampleNotice !== null;
  const progress = overview?.progress ?? null;
  const incomplete = progress !== null && !(progress.updating && progress.queued === 0);
  const filled = query !== "" || examples.length > 0 || exampleNotice !== null;
  const dim = focused && !results;
  const engaged = focused || results;
  const [fieldHasBeenFocused, setFieldHasBeenFocused] = useState(false);
  if (focused && !fieldHasBeenFocused) setFieldHasBeenFocused(true);
  const opening = asWidget && !blocked && !fieldHasBeenFocused;
  const refuse = useCallback(() => setAttention((count) => count + 1), []);
  const text = understood.text.trim();
  const fileType = understood.fileType;
  const language = fileType === "code" ? understood.language : null;
  const project = fileType === "code" ? understood.project : null;
  const tests = fileType === "code" ? understood.tests : null;
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
        .search(text, { category: fileType, from: fromMs, to: toMs, language, project, tests }, examples)
        .then(
          (page: SearchPage) =>
            current &&
            setFound({
              id: page.id,
              batches: [page.hits],
              more: page.more,
              moreClose: page.moreClose,
              weakFrom: page.close ? null : 0,
              text,
              filtered: fileType !== null || fromMs !== null || toMs !== null,
            }),
          (error) => current && setFound({ error: errorText(error) }),
        )
        .finally(() => current && setSearching(false));
    }, SEARCH_DELAY_MS);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [results, searchable, text, fileType, fromMs, toMs, language, project, tests, examples, filledIn]);
  const batches = found && "batches" in found ? found.batches : NO_BATCHES;
  const hits = useMemo(() => (batches.length === 0 ? NO_HITS : batches.flat()), [batches]);
  const shortcutTip = useShortcutTip({ onScreen: onScreen && !asWidget, worth: hits.length > 0 && !searching, results: hits.length > 0 });
  const viewMore = useCallback(() => {
    if (!found || !("id" in found) || loadingMore) return;
    const { id } = found;
    setLoadingMore(true);
    api
      .searchMore(id)
      .then((page) =>
        setFound((now) =>
          now && "id" in now && now.id === id
            ? {
                ...now,
                batches: [...now.batches, page.hits],
                more: page.more,
                moreClose: page.moreClose,
                stalled: false,
                weakFrom: now.weakFrom ?? (page.close ? null : now.batches.length),
              }
            : now,
        ),
      )
      .catch(() => setFound((now) => (now && "id" in now && now.id === id ? { ...now, stalled: true } : now)))
      .finally(() => setLoadingMore(false));
  }, [found, loadingMore]);

  const [barAway, setBarAway] = useState(false);
  const stowed = results && barAway;
  useEffect(() => setBarAway(false), [query]);
  useEffect(() => setFailure(null), [query]);
  useEffect(() => {
    if (failure === null) return;
    const timer = window.setTimeout(() => setFailure(null), FAILURE_MS);
    return () => window.clearTimeout(timer);
  }, [failure]);
  useEffect(() => {
    if (focused) setBarAway(false);
  }, [focused]);
  const { selected, active, select, move, selectAll, clear: clearSelection } = useResultSelection(hits);
  const selectedHits = useMemo(() => hits.filter((hit) => selected.has(hit.path)), [hits, selected]);
  const keysFrom = active !== null && selected.has(active) ? active : (selectedHits[0]?.path ?? active);
  const resultActions = useMemo<ResultActions>(
    () => ({
      open: (hit) => {
        (hit.category === "code" ? api.openCode(hit.path, codeLine(hit)) : api.openFile(hit.path)).then(
          () => onDismiss?.(),
          (e) => setFailure(`Couldn't open ${hit.name}. ${errorText(e)}`),
        );
      },
      openAt: (hit, line) => {
        api.openCode(hit.path, line).then(
          () => onDismiss?.(),
          (e) => setFailure(`Couldn't open ${hit.name}. ${errorText(e)}`),
        );
      },
      showInFolder: (hit) => {
        api.showInFolder(hit.path).then(
          () => onDismiss?.(),
          (e) => setFailure(`Couldn't show ${hit.name} in its folder. ${errorText(e)}`),
        );
      },
      copyFiles: (files) => api.copyFiles(files.map((file) => file.path)),
      copyPaths: async (files) => {
        await navigator.clipboard.writeText(files.map((file) => file.path).join("\n"));
      },
    }),
    [onDismiss],
  );
  const panelShown = results && selectedHits.length > 0;
  const [panelColumns, setPanelColumns] = useState(storedPanelColumns);
  const choosePanelColumns = useCallback((columns: number) => {
    setPanelColumns(columns);
    keepPanelColumns(columns);
  }, []);
  const [resultsBox, setResultsBox] = useState<{ column: number; pixelRatio: number; side: number; scrollbar: number } | null>(null);
  const columns = resultsBox ? resultColumns(resultsBox.column, asWidget ? WIDGET_COLUMNS : undefined, panelColumns) : null;
  const beside = columns && resultsBox && !asWidget ? panelBeside(resultsBox.side, columns.track) : null;
  const panelTaken = beside ?? columns?.taken ?? PANEL_COLUMNS;
  const panelSize = beside !== null && columns ? panelWidth(columns.track, beside) : (columns?.panel ?? 0);
  const [aimedColumns, setAimedColumns] = useState<number | null>(null);
  const layout = panelLayout(aimedColumns !== null && columns ? panelWidth(columns.track, aimedColumns) : panelSize);
  const shift =
    panelShown && beside !== null && resultsBox
      ? Math.round(((resultsBox.scrollbar + PANEL_GAP + panelSize) / 2) * resultsBox.pixelRatio) / resultsBox.pixelRatio
      : 0;
  const columnStyle = useMemo(
    () =>
      columns && resultsBox
        ? ({
            "--result-track": `${columns.track}px`,
            "--result-columns": String(columns.columns),
            "--panel-width": `${panelSize}px`,
            "--panel-room": `${beside !== null ? panelRoom(panelSize) : columns.room}px`,
            "--panel-columns": String(columns.besidePanel),
            "--preview-height": `${previewHeight(columns.track, resultsBox.pixelRatio)}px`,
            "--device-px": `${1 / resultsBox.pixelRatio}px`,
            "--results-shift": `${shift}px`,
          } as CSSProperties)
        : undefined,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [columns?.track, columns?.columns, columns?.room, columns?.besidePanel, panelSize, beside !== null, resultsBox?.pixelRatio, shift],
  );
  const weakPaths = useMemo(() => {
    const weakFrom = found && "weakFrom" in found ? found.weakFrom : null;
    return new Set(weakFrom === null ? [] : batches.slice(weakFrom).flat().map((hit) => hit.path));
  }, [found, batches]);
  const isWeak = useCallback((path: string) => weakPaths.has(path), [weakPaths]);
  const prefetch = useCallback((hit: SearchHit, pressed: boolean) => prefetchDetails(hit, text, pressed), [text]);
  const findSimilar = useCallback(
    (paths: string[]) => {
      setQuery("");
      setChosen({});
      addExample(paths, true);
      clearSelection();
    },
    [addExample, clearSelection],
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
      } else if (selectedHits.length > 0 && !window.getSelection()?.toString()) {
        e.preventDefault();
        void (e.shiftKey ? resultActions.copyPaths(selectedHits) : resultActions.copyFiles(selectedHits)).catch(() => {});
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [results, selectedHits, selectAll, clearSelection, resultActions]);
  let message: ResultMessage | null = null;
  if (exampleNotice) message = { text: exampleNotice };
  else if (!searchable) message = { text: model?.state === "failed" ? "Search isn’t available." : "Waiting for the search model to load…" };
  else if (found && "error" in found) message = { text: `The search failed: ${found.error}` };
  else if (found && "batches" in found && batches[0]?.length === 0) {
    if (fileType && indexedTypes && !indexedTypes.includes(fileType)) {
      const openIndex = <MessageLink onClick={() => openIndexFromProgress()}>{fileType === "code" ? "Index" : "Choose what’s indexed"}</MessageLink>;
      message = {
        text: NOT_INDEXED[fileType],
        hint: fileType === "code" ? <>Turn on project code in the advanced view of the {openIndex}.</> : <>{openIndex} to search them.</>,
      };
    } else if (found.more || batches.length > 1) {
      const quoted = found.text.length > MAX_QUOTED ? `${found.text.slice(0, MAX_QUOTED).trimEnd()}…` : found.text;
      message = {
        text: quoted ? `No strong matches for “${quoted}”` : "No strong matches",
        hint:
          batches.length > 1 ? (
            "These are the closest anyway."
          ) : (
            <>
              Try other words, or <MessageLink onClick={viewMore}>look through the closest</MessageLink> anyway.
            </>
          ),
      };
    } else {
      message = { text: "Nothing found", hint: found.filtered ? "Nothing indexed is of that type or from that time." : undefined };
    }
  }
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
  const leave = useCallback((keepFocus = false) => {
    away.current = false;
    setQuery("");
    setExamples([]);
    setExampleNotice(null);
    setChosen({});
    if ((asWidget || keepFocus) && !blocked) {
      input.current?.focus({ preventScroll: true });
      onFocusChange?.(true);
      setFocused(true);
      return;
    }
    onFocusChange?.(false);
    setFocused(false);
    input.current?.blur();
  }, [onFocusChange, asWidget, blocked]);
  // Handled here rather than on click: once the cross disappears, WebKit sends the click on to the field and Chromium doesn't.
  const clear = useCallback(() => leave(true), [leave]);
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
      if (panelShown) {
        e.preventDefault();
        clearSelection();
      } else if (filled) {
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
  }, [filled, leave, asWidget, onDismiss, panelShown, clearSelection]);

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
      const pillBottom = PROGRESS_BOTTOM_PX;
      set("--bottom-gap", bottomGap);
      set("--pill-bottom", pillBottom);
      set("--lift", lift);
      set("--drop", Math.max(0, around.bottom - pillBottom - block.bottom));
      set("--actions-width", snapToLayout(actionsWidth) + gap);
      set("--hole-left", block.left - around.left);
      set("--hole-right", around.right - block.right);
      set("--hole-top", block.top - around.top - lift);
      set("--home-height", around.height);
      const scrollbar = parseFloat(getComputedStyle(root).getPropertyValue("--scrollbar-size")) || 0;
      const column = Math.min(MOST_RESULTS_WIDTH, around.width - 2 * scrollbar);
      const pixelRatio = window.devicePixelRatio || 1;
      const side = Math.floor((around.width - column) / 2 - scrollbar - PANEL_GAP - SIDE_MARGIN);
      if (column > 0) {
        setResultsBox((now) =>
          now?.column === column && now.pixelRatio === pixelRatio && now.side === side && now.scrollbar === scrollbar ? now : { column, pixelRatio, side, scrollbar },
        );
      }
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
      style={columnStyle}
      data-fading={fading ? "" : undefined}
      data-snapping={snapping ? "" : undefined}
      onWheel={(e) => {
        const target = e.target instanceof Element ? e.target : null;
        if (!results || !target || resultsPane.current?.contains(target) || target.closest("[data-result-panel], [role='menu']")) return;
        forwardWheel(e);
      }}
      className={cx(styles.home, engaged && styles.engaged, focused && styles.wide, dim && styles.dim, results && styles.results, stowed && styles.stowed, blocked && styles.blocked, asWidget && styles.asWidget, opening && styles.opening, overview?.progress != null && styles.indexing)}
      onClick={(e) => {
        const target = e.target as HTMLElement;
        if (!target.closest(`button, input, label, [role='menu'], [data-selection-bar], [data-result-panel], .${styles.tagsBox}`)) clearSelection();
      }}
      onPointerDown={
        asWidget && onDismiss
          ? (e) => {
              if (!home.current?.contains(e.target as Node)) return;
              const box = home.current.querySelector(`.${styles.spot}`)?.getBoundingClientRect();
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
        moreClose={found !== null && "moreClose" in found && found.moreClose}
        stalled={found !== null && "stalled" in found && found.stalled === true}
        weakFrom={found !== null && "weakFrom" in found ? found.weakFrom : null}
        loadingMore={loadingMore}
        onMore={viewMore}
        shown={results}
        busy={searching}
        message={message}
        progress={overview?.progress != null}
        selected={selected}
        selection={panelShown ? NO_HITS : selectedHits}
        actions={resultActions}
        onClearSelection={clearSelection}
        onSelect={select}
        active={keysFrom}
        onMove={move}
        onOpen={resultActions.open}
        onContextMenu={openMenu}
        noted={incomplete && dragging === null}
        away={stowed}
        onAway={setBarAway}
        panel={panelShown && beside === null}
        panelColumns={panelTaken}
        onPrefetch={prefetch}
      />
      <ResultPanel
        hits={selectedHits}
        shown={panelShown}
        query={text}
        withFiles={examples.length > 0}
        files={examples}
        isWeak={isWeak}
        progress={overview?.progress != null}
        away={stowed}
        actions={resultActions}
        onFindSimilar={findSimilar}
        onSelectOnly={(path) => select(path, false)}
        beside={beside !== null}
        layout={layout}
        resize={
          columns && beside === null
            ? {
                track: columns.track,
                taken: panelTaken,
                least: columns.least,
                standard: DEFAULT_PANEL_COLUMNS,
                most: columns.most,
                onResize: choosePanelColumns,
                onAiming: setAimedColumns,
              }
            : null
        }
      />

      <img className={styles.logo} src={logo} alt="Mesa" draggable={false} />
      <div ref={search} className={styles.search}>
        <div
          ref={row}
          className={cx(styles.row, dropping && (dropping.supported.length > 0 && room > 0 ? styles.dropping : styles.refusing), dragging?.over && styles.hot)}
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
              <SlidingIcon icons={FIELD_ICONS} current={dropIcon(dropping, room)} />
            </span>
            <input
              ref={input}
              className={styles.input}
              type="text"
              placeholder={placeholder(model, blocked, dropping && dropHint(dropping.supported, dropping.unsupported, dropping.folders, room))}
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
              <button type="button" className={cx(styles.clear, dragging && styles.away)} aria-label="Clear search" data-tooltip="Clear (Esc)" onClick={clear}>
                <X size={18} aria-hidden />
              </button>
            )}
            {!blocked && !modelFailed && (
              <ExampleButton className={styles.attach} onPick={addExample} hidden={dragging !== null} full={room <= 0} calmFor={`${focused}|${results}|${engaged}`} />
            )}
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
            <Understanding understood={understood} indexedTypes={indexedTypes} query={query} examples={examples} examplesProgress={examplesProgress} limitNote={limitNote} fieldFocused={focused} onFocusField={focusField} onChooseType={chooseType} onChooseDate={chooseDate} onRemoveExample={removeExample} onClearExamples={clearExamples} />
          </div>
          <p className={cx(styles.indexNote, incomplete && dragging === null && styles.shown)} aria-hidden={!incomplete || undefined}>
            <CircleDashed size={12} aria-hidden />
            Still indexing, results may be incomplete.
          </p>
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
      <ShortcutTip show={shortcutTip.visible && failure === null} keys={shortcutTip.keys} lifted={overview?.progress != null} />
      {failure && (
        <div className={cx(styles.failure, overview?.progress != null && styles.failureLifted)} role="alert">
          <span>{failure}</span>
          <button type="button" className={styles.failureDismiss} onClick={() => setFailure(null)} aria-label="Dismiss">
            <X size={14} aria-hidden />
          </button>
        </div>
      )}
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
