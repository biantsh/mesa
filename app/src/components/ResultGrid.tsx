import { Fragment, useEffect, useRef, useState, type ComponentType, type CSSProperties, type ReactNode, type RefObject, type UIEvent, type WheelEvent } from "react";
import { Check, FileText, Film, Image as ImageIcon, Music, X, type LucideProps } from "lucide-react";
import { previewUrl, type Category, type SearchHit } from "../api";
import { cx } from "../cx";
import { formatMonth, formatSize } from "../format";
import type { ResultActions } from "../resultMenu";
import { useForwardedWheel } from "../useForwardedWheel";
import { usePreview } from "../usePreview";
import { pressProps } from "./entryProps";
import styles from "./ResultGrid.module.css";
import { fileManager, shortcut } from "../platform";

export const CATEGORY_ICONS: Record<Category, ComponentType<LucideProps>> = {
  image: ImageIcon,
  video: Film,
  audio: Music,
  document: FileText,
};

interface Props {
  paneRef: RefObject<HTMLDivElement | null>;
  batches: SearchHit[][];
  searchId: number;
  more: boolean;
  loadingMore: boolean;
  onMore: () => void;
  shown: boolean;
  busy: boolean;
  message: string | null;
  progress: boolean;
  selected: ReadonlySet<string>;
  selection: SearchHit[];
  actions: ResultActions;
  onClearSelection: () => void;
  onSelect: (path: string, toggle: boolean) => void;
  onOpen: (hit: SearchHit) => void;
  onContextMenu: (hit: SearchHit, x: number, y: number) => void;
  away: boolean;
  onAway: (away: boolean) => void;
}

const ALWAYS_SHOWN_ABOVE_PX = 48;
const SCROLL_DOWN_PX = 24;
const SCROLL_UP_PX = 10;

export function ResultGrid({ paneRef, batches, searchId, more, loadingMore, onMore, shown, busy, message, progress, selected, selection, actions, onClearSelection, onSelect, onOpen, onContextMenu, away, onAway }: Props) {
  const last = useRef(0);
  const run = useRef(0);
  const scrolled = (e: UIEvent<HTMLDivElement>) => {
    const top = e.currentTarget.scrollTop;
    const moved = top - last.current;
    last.current = top;
    if (top < ALWAYS_SHOWN_ABOVE_PX) {
      run.current = 0;
      onAway(false);
      return;
    }
    run.current = Math.sign(moved) === Math.sign(run.current) ? run.current + moved : moved;
    if (run.current > SCROLL_DOWN_PX) onAway(true);
    else if (run.current < -SCROLL_UP_PX) onAway(false);
  };
  useEffect(() => {
    paneRef.current?.scrollTo({ top: 0 });
  }, [paneRef, searchId]);
  const forwardWheel = useForwardedWheel(paneRef);
  const counting = shown && selection.length > 0;
  return (
    <>
      <div
        ref={paneRef}
        className={cx(
          styles.pane,
          shown && styles.shown,
          busy && styles.busy,
          progress && styles.progress,
          counting && styles.counting,
          away && styles.away,
        )}
        inert={!shown}
        onScroll={scrolled}
      >
        <div className={styles.grid}>
          {batches.map((batch, number) => (
            <Fragment key={number}>
              {number > 0 && <div className={styles.divider} role="separator" />}
              {batch.map((hit, index) => (
                <Result
                  key={hit.path}
                  hit={hit}
                  index={index}
                  selected={selected.has(hit.path)}
                  onSelect={onSelect}
                  onOpen={onOpen}
                  onContextMenu={onContextMenu}
                />
              ))}
            </Fragment>
          ))}
        </div>
        {more && (
          <div className={styles.more}>
            <button type="button" className={styles.moreButton} onClick={onMore} disabled={loadingMore}>
              View more
            </button>
          </div>
        )}
        {message && batches.length === 0 && <p className={styles.message}>{message}</p>}
      </div>
      <SelectionBar
        hits={selection}
        visible={counting}
        progress={progress}
        actions={actions}
        onClear={onClearSelection}
        onWheel={forwardWheel}
      />
    </>
  );
}

interface BarProps {
  hits: SearchHit[];
  visible: boolean;
  progress: boolean;
  actions: ResultActions;
  onClear: () => void;
  onWheel: (e: WheelEvent) => void;
}

const COPIED_FOR_MS = 1500;

function Foldable({ shown, inline, children }: { shown: boolean; inline?: boolean; children: ReactNode }) {
  return (
    <div className={cx(styles.fold, inline && styles.inline, !shown && styles.folded)} inert={!shown}>
      <div className={styles.foldInner}>{children}</div>
    </div>
  );
}

function SelectionBar({ hits, visible, progress, actions, onClear, onWheel }: BarProps) {
  const kept = useRef(hits);
  if (hits.length > 0) kept.current = hits;
  const shown = kept.current;
  const one = shown.length === 1;
  const size = shown.reduce((total, hit) => total + hit.size, 0);

  const [copy, setCopy] = useState<{ what: "files" | "paths"; ok: boolean } | null>(null);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const copying = (what: "files" | "paths") => {
    const done = (ok: boolean) => {
      setCopy({ what, ok });
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopy(null), COPIED_FOR_MS);
    };
    (what === "files" ? actions.copyFiles(shown) : actions.copyPaths(shown)).then(
      () => done(true),
      () => done(false),
    );
  };
  const copyButton = (what: "files" | "paths", noun: string, shortcut: string) => {
    const result = copy?.what === what ? copy : null;
    return (
      <button type="button" className={styles.barButton} title={`Copy ${noun}${one ? "" : "s"} (${shortcut})`} onClick={() => copying(what)}>
        <span className={styles.swap}>
          <span className={cx(result && styles.hidden)}>
            Copy {noun}
            <Foldable inline shown={!one}>
              s
            </Foldable>
          </span>
          <span className={cx(styles.result, !result && styles.hidden)} aria-live="polite">
            {result?.ok === false ? (
              "Couldn’t copy"
            ) : (
              <>
                <Check size={12} strokeWidth={2.5} aria-hidden />
                Copied
              </>
            )}
          </span>
        </span>
      </button>
    );
  };

  return (
    <div className={cx(styles.counter, progress && styles.progress, visible && styles.counting)} aria-hidden={!visible}>
      <div className={styles.bar} data-selection-bar inert={!visible} onWheel={onWheel}>
        <span className={styles.count} role="status">
          {shown.length} selected <span className={styles.size}>· {formatSize(size)}</span>
        </span>
        <span className={styles.rule} aria-hidden />
        <Foldable shown={one}>
          <button type="button" className={styles.barButton} title="Open" onClick={() => actions.open(shown[0])}>
            Open
          </button>
        </Foldable>
        <Foldable shown={one}>
          <button
            type="button"
            className={styles.barButton}
            title={`Show in ${fileManager}`}
            onClick={() => actions.showInFolder(shown[0])}
          >
            Show in {fileManager}
          </button>
        </Foldable>
        {copyButton("files", "file", shortcut("C"))}
        {copyButton("paths", "path", shortcut("C", true))}
        <span className={styles.rule} aria-hidden />
        <button type="button" className={cx(styles.barButton, styles.clear)} onClick={onClear} aria-label="Clear selection" title={`Clear selection (${shortcut("A", true)})`}>
          <X size={14} aria-hidden />
        </button>
      </div>
    </div>
  );
}

interface ResultProps {
  hit: SearchHit;
  index: number;
  selected: boolean;
  onSelect: (path: string, toggle: boolean) => void;
  onOpen: (hit: SearchHit) => void;
  onContextMenu: (hit: SearchHit, x: number, y: number) => void;
}

function Result({ hit, index, selected, onSelect, onOpen, onContextMenu }: ResultProps) {
  const Icon = CATEGORY_ICONS[hit.category];
  const { wanted, loaded, onLoad, onError } = usePreview({ kind: "file", readable: true, category: hit.category });
  return (
    <button
      type="button"
      className={cx(styles.tile, selected && styles.selected)}
      style={{ "--i": index } as CSSProperties}
      {...pressProps(hit, true, hit.path, { selected, onSelect, onOpen, onContextMenu })}
    >
      <span className={styles.preview}>
        <Icon size={28} strokeWidth={1.4} aria-hidden />
        {wanted && (
          <img
            className={cx(styles.thumb, loaded && styles.loaded, hit.category === "document" && styles.contain)}
            src={previewUrl(hit.path)}
            alt=""
            draggable={false}
            loading="lazy"
            decoding="async"
            onLoad={onLoad}
            onError={onError}
          />
        )}
      </span>
      <span className={styles.name}>{hit.name}</span>
      <span className={styles.meta}>
        <span className={styles.folder}>{hit.folder}</span>
        <span className={styles.date} aria-hidden>·</span>
        <span className={styles.date}>{formatMonth(hit.modified)}</span>
      </span>
    </button>
  );
}
