import { Fragment, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ComponentType, type CSSProperties, type ReactNode, type RefObject, type SyntheticEvent } from "react";
import { Check, Copy, FolderOpen, Link, ListFilter, ScanSearch, Sparkles, TextSearch, type LucideProps } from "lucide-react";
import { MAX_EXAMPLES } from "../examples";
import { matchReason, type MatchReason } from "../matchReason";
import { api, largePreviewUrl, type AlsoIn, type MatchDetails, type MatchSnippet, type SearchHit } from "../api";
import { CodeThumb } from "./CodeThumb";
import { CodeView } from "./CodeView";
import { CodeViewer } from "./CodeViewer";
import { codeLanguageOf, fileNameOf } from "../examples";
import { detailsAsked, detailsFor, detailsNow, wantsDetails as wantsDetailsOf } from "../matchDetails";
import { cx } from "../cx";
import { CATEGORY_LABELS, formatModified, formatSize } from "../format";
import { prefersReducedMotion } from "../motion";
import { atIsPage, pagingOf, pagingWord, type Paging } from "../paging";
import { middleCuts } from "../pathCut";
import type { PanelLayout } from "../resultWindow";
import { parentOf } from "../paths";
import { fileManager, isWebKitGTK, shortcut } from "../platform";
import { isResizing } from "../resizing";
import type { ResultActions } from "../resultMenu";
import { previewShape, usePixelRatio, usePreview } from "../usePreview";
import { Button } from "./Button";
import { EASE_CURVE, EASE_MS } from "./PanelSplitter";
import { PanelResizer } from "./PanelResizer";
import { PreviewViewer } from "./PreviewViewer";
import { CATEGORY_ICONS } from "./ResultGrid";
import styles from "./ResultPanel.module.css";
import "../registeredZoom";

interface Props {
  hits: SearchHit[];
  shown: boolean;
  query: string;
  withFiles: boolean;
  files: string[];
  isWeak: (path: string) => boolean;
  progress: boolean;
  away: boolean;
  actions: ResultActions;
  onFindSimilar: (paths: string[]) => void;
  onSelectOnly: (path: string) => void;
  beside: boolean;
  resize: {
    track: number;
    taken: number;
    least: number;
    most: number;
    standard: number;
    onResize: (columns: number) => void;
    onAiming: (columns: number | null) => void;
  } | null;
  layout: PanelLayout;
}

const LARGE_STEP_PX = 128;
const LARGEST_PX = 1600;
const LARGE_SETTLED_MS = 300;
const COPIED_FOR_MS = 1500;
const DETAILS_DELAY_MS = 120;
const MOSAIC_MOST = 9;
const NAMED_MOST = 12;

export function ResultPanel({ hits, shown, query, withFiles, files, isWeak, progress, away, actions, onFindSimilar, onSelectOnly, beside, resize, layout }: Props) {
  const kept = useRef(hits);
  if (hits.length > 0) kept.current = hits;
  const showing = kept.current;
  const panel = useRef<HTMLElement>(null);
  useLayoutGlide(panel, layout);
  return (
    <aside
      ref={panel}
      className={cx(styles.panel, shown && styles.shown, progress && styles.progress, away && styles.away, beside && styles.beside, layout === "middle" && styles.middleLayout, layout === "wide" && styles.wideLayout)}
      data-result-panel
      aria-label="Selected result"
      aria-hidden={!shown}
      inert={!shown}
    >
      {resize && <PanelResizer {...resize} />}
      {showing.length === 1 ? (
        <OneFile
          key={showing[0].path}
          hit={showing[0]}
          query={query}
          withFiles={withFiles}
          files={files}
          weak={isWeak(showing[0].path)}
          actions={actions}
          onFindSimilar={onFindSimilar}
          layout={layout}
        />
      ) : showing.length > 1 ? (
        <SeveralFiles hits={showing} actions={actions} onFindSimilar={onFindSimilar} onSelectOnly={onSelectOnly} />
      ) : null}
    </aside>
  );
}

const GLIDE = "panel-layout";
// Where each gliding element was drawn as the current frame began.
const drawnAt = new WeakMap<HTMLElement, DOMRect>();

function glide(element: HTMLElement, from: DOMRect, scaled: boolean) {
  // A glide under way carries on from where the element is drawn, not a stale measurement: as the panel's width eases,
  // the picture's room can change several times during one glide.
  const running = element.getAnimations().filter((animation) => animation.id === GLIDE);
  if (running.length > 0) from = drawnAt.get(element) ?? from;
  for (const animation of running) animation.cancel();
  // WebKitGTK can't keep up with gliding while the panel is being dragged, so there things just move.
  if (isWebKitGTK && isResizing()) return;
  const to = element.getBoundingClientRect();
  if (to.width === 0 || to.height === 0) return;
  const x = from.left - to.left;
  const y = from.top - to.top;
  const width = scaled ? from.width / to.width : 1;
  const height = scaled ? from.height / to.height : 1;
  if (Math.abs(x) < 0.5 && Math.abs(y) < 0.5 && Math.abs(width - 1) < 0.005 && Math.abs(height - 1) < 0.005) return;
  const animation = element.animate(
    [
      { transform: `translate(${x}px, ${y}px) scale(${width}, ${height})`, transformOrigin: "0 0" },
      { transform: "none", transformOrigin: "0 0" },
    ],
    { duration: EASE_MS, easing: EASE_CURVE, id: GLIDE },
  );
  drawnAt.set(element, from);
  const follow = () => {
    if (animation.playState !== "running") return;
    drawnAt.set(element, element.getBoundingClientRect());
    requestAnimationFrame(follow);
  };
  requestAnimationFrame(follow);
}

function useLayoutGlide(panel: RefObject<HTMLElement | null>, layout: PanelLayout) {
  const drawn = useRef(layout);
  const before = useRef<Map<HTMLElement, DOMRect> | null>(null);
  if (drawn.current !== layout && before.current === null && panel.current) {
    before.current = new Map(Array.from(panel.current.querySelectorAll<HTMLElement>("[data-glide]"), (element): [HTMLElement, DOMRect] => [element, element.getBoundingClientRect()]));
  }
  useLayoutEffect(() => {
    if (drawn.current === layout) return;
    drawn.current = layout;
    const was = before.current;
    before.current = null;
    if (!was || prefersReducedMotion() || document.documentElement.hasAttribute("data-resizing")) return;
    for (const element of was.keys()) for (const animation of element.getAnimations()) if (animation.id === GLIDE) animation.cancel();
    for (const [element, from] of was) if (element.isConnected) glide(element, from, element.dataset.glide === "picture");
  }, [layout, panel]);
}

function useSteadyPicture(picture: RefObject<HTMLElement | null>, sizer: RefObject<HTMLElement | null> | null) {
  const sizerNow = useRef(sizer);
  sizerNow.current = sizer;
  useLayoutEffect(() => {
    const element = picture.current;
    const room = element?.parentElement;
    if (!element || !room) return;
    const heightOf = () => sizerNow.current?.current?.offsetHeight ?? null;
    let seen = element.getBoundingClientRect();
    let height = heightOf();
    let watched: Element | null = null;
    const observer = new ResizeObserver(() => {
      const now = heightOf();
      const stepped = now !== null && height !== null && now !== height && sizerNow.current?.current === watched;
      height = now;
      if (stepped && !prefersReducedMotion()) glide(element, seen, true);
      seen = element.getBoundingClientRect();
      watch();
    });
    const watch = () => {
      const sizerElement = sizerNow.current?.current ?? null;
      if (sizerElement === watched) return;
      if (watched) observer.unobserve(watched);
      watched = sizerElement;
      if (watched) observer.observe(watched);
      height = heightOf();
    };
    observer.observe(room);
    observer.observe(element);
    watch();
    return () => observer.disconnect();
  }, [picture]);
}

function CopyButton({ words, tooltip, icon: Icon, copy }: { words: string; tooltip: string; icon: ComponentType<LucideProps>; copy: () => Promise<void> }) {
  const [outcome, setOutcome] = useState<"copied" | "failed" | null>(null);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const press = () => {
    const done = (now: "copied" | "failed") => {
      setOutcome(now);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setOutcome(null), COPIED_FOR_MS);
    };
    copy().then(
      () => done("copied"),
      () => done("failed"),
    );
  };
  return (
    <button type="button" className={styles.quietButton} onClick={press} data-tooltip={tooltip}>
      {outcome === "copied" ? <Check size={14} aria-hidden /> : <Icon size={14} aria-hidden />}
      {outcome === "copied" ? "Copied" : outcome === "failed" ? "Couldn’t copy" : words}
    </button>
  );
}

function extensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toUpperCase() : "";
}

const REASON_ICONS: Record<MatchReason["kind"], ComponentType<LucideProps>> = { words: TextSearch, meaning: Sparkles, filters: ListFilter };

function breakable(text: string): ReactNode {
  const pieces = text.split(/(?<=[_./\\])/);
  if (pieces.length === 1) return text;
  return pieces.map((piece, i) => (
    <Fragment key={i}>
      {piece}
      {i < pieces.length - 1 && <wbr />}
    </Fragment>
  ));
}

interface ShownCode {
  path: string;
  name: string;
  line: number;
  found: { start: number; end: number } | null;
  from: () => DOMRect | null;
}

function AlsoInRow({ path, symbol, detail, onShow }: { path: string; symbol?: string; detail: string; onShow: (from: () => DOMRect | null) => void }) {
  const row = useRef<HTMLButtonElement>(null);
  const folder = parentOf(path).split(/[\\/]/).pop();
  return (
    <button
      ref={row}
      type="button"
      className={cx(styles.snippet, styles.pressable)}
      onClick={() => onShow(() => (row.current?.isConnected ? row.current.getBoundingClientRect() : null))}
    >
      <span className={styles.page}>{detail}</span>
      <span className={styles.snippetText}>
        {fileNameOf(path)}
        {folder && <span className={styles.alsoFolder}> in {folder}</span>}
        {symbol && <span className={styles.alsoSymbol}> · {symbol}</span>}
      </span>
    </button>
  );
}

function Snippet({ snippet, paging, onPage, current, lines }: { snippet: MatchSnippet; paging: Paging | null; onPage?: (page: number) => void; current: boolean; lines?: boolean }) {
  const body = (
    <>
      {snippet.page !== null && (
        <span className={styles.page}>
          {lines ? "line" : paging === "slide" ? "slide" : "p."} {snippet.page}
        </span>
      )}
      <span className={styles.snippetText}>
        {snippet.parts.map((part, i) => (part.hit ? <mark key={i}>{breakable(part.text)}</mark> : <span key={i}>{breakable(part.text)}</span>))}
      </span>
    </>
  );
  if (snippet.page === null || !onPage) return <div className={styles.snippet}>{body}</div>;
  const page = snippet.page;
  return (
    <button type="button" className={cx(styles.snippet, styles.pressable, current && styles.current)} onClick={() => onPage(page)} data-tooltip={lines ? `Show line ${page}` : `Show ${paging ?? "page"} ${page}`}>
      {body}
    </button>
  );
}

const FACTS_ABOVE = 16;
const THUMB_FADE_MS = 80;

function useThumbFade(body: RefObject<HTMLDivElement | null>, room: RefObject<HTMLDivElement | null>) {
  useLayoutEffect(() => {
    const element = body.current;
    const spacer = room.current;
    if (!element || !spacer) return;
    const left = () => {
      const end = spacer.getBoundingClientRect().top - element.getBoundingClientRect().top + element.scrollTop;
      return end + parseFloat(getComputedStyle(element).paddingBottom) - element.clientHeight;
    };
    let alpha = left() > 0.5 ? 1 : 0;
    let fading: "in" | "out" | null = null;
    let frame = 0;
    const show = (value: number) => {
      alpha = value;
      element.style.setProperty("--thumb-alpha", String(Math.round(value * 1000) / 1000));
    };
    const hold = (px: number) => {
      const height = `${Math.max(0, px)}px`;
      if (spacer.style.height !== height) spacer.style.height = height;
    };
    const fade = (to: 0 | 1, then: () => void) => {
      cancelAnimationFrame(frame);
      const from = alpha;
      const start = performance.now();
      const step = (now: number) => {
        const t = Math.min(1, (now - start) / THUMB_FADE_MS);
        show(from + (to - from) * (1 - (1 - t) * (1 - t)));
        if (t < 1) frame = requestAnimationFrame(step);
        else then();
      };
      frame = requestAnimationFrame(step);
    };
    const update = () => {
      const overflow = left();
      if (overflow > 0.5) {
        hold(0);
        if (fading === "out" || (alpha < 1 && fading !== "in")) {
          fading = "in";
          fade(1, () => (fading = null));
        }
        return;
      }
      if (alpha > 0 && fading !== "out") {
        fading = "out";
        fade(0, () => {
          fading = null;
          hold(0);
        });
      }
      if (fading === "out") hold(Math.ceil(-overflow) + 1);
    };
    show(alpha);
    const watching = new ResizeObserver(update);
    watching.observe(element);
    for (const child of Array.from(element.querySelectorAll(":scope > *, :scope > * > *"))) watching.observe(child);
    return () => {
      watching.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [body, room]);
}

const HEIGHT_FOLLOWS_MS = 70;

function useSmoothHeight(content: RefObject<HTMLDivElement | null>) {
  useLayoutEffect(() => {
    const element = content.current;
    if (!element) return;
    let held: number | null = null;
    let own = element.getBoundingClientRect().height;
    let frame = 0;
    let last = 0;
    const ownNow = () => {
      const height = element.style.height;
      element.style.height = "";
      const now = element.getBoundingClientRect().height;
      element.style.height = height;
      return now;
    };
    const hold = (height: number | null) => {
      held = height;
      if (height === null) {
        element.style.removeProperty("height");
        element.style.removeProperty("overflow-y");
      } else {
        element.style.height = `${height}px`;
        element.style.overflowY = "clip";
      }
    };
    const step = (now: number) => {
      const to = ownNow();
      own = to;
      const from = held ?? to;
      const elapsed = Math.min(64, now - last);
      last = now;
      const next = from + (to - from) * (1 - Math.exp(-elapsed / HEIGHT_FOLLOWS_MS));
      if (Math.abs(to - next) < 0.5) {
        hold(null);
        frame = 0;
        return;
      }
      hold(next);
      frame = requestAnimationFrame(step);
    };
    const watching = new ResizeObserver(() => {
      const now = held === null ? element.getBoundingClientRect().height : ownNow();
      const was = own;
      own = now;
      if (held !== null || Math.abs(now - was) < 1 || prefersReducedMotion()) return;
      hold(was);
      last = performance.now();
      frame = requestAnimationFrame(step);
    });
    watching.observe(element);
    return () => {
      watching.disconnect();
      cancelAnimationFrame(frame);
      hold(null);
    };
  }, [content]);
}

function useLargeSize(frame: RefObject<HTMLElement | null>): number | null {
  const scale = usePixelRatio();
  const [size, setSize] = useState<number | null>(null);
  useLayoutEffect(() => {
    const element = frame.current;
    if (!element) return;
    const measure = () => {
      const longest = Math.max(element.offsetWidth, element.offsetHeight) * scale;
      const wanted = Math.min(LARGEST_PX, Math.ceil(longest / LARGE_STEP_PX) * LARGE_STEP_PX);
      if (wanted > 0) setSize((was) => (was !== null && was >= wanted ? was : wanted));
    };
    measure();
    let timer = 0;
    const watching = new ResizeObserver(() => {
      window.clearTimeout(timer);
      timer = window.setTimeout(measure, LARGE_SETTLED_MS);
    });
    watching.observe(element);
    return () => {
      watching.disconnect();
      window.clearTimeout(timer);
    };
  }, [frame, scale]);
  return size;
}

interface OneProps {
  hit: SearchHit;
  layout: PanelLayout;
  query: string;
  withFiles: boolean;
  files: string[];
  weak: boolean;
  actions: ResultActions;
  onFindSimilar: (paths: string[]) => void;
}

function OneFile({ hit, query, withFiles, files, weak, actions, onFindSimilar, layout }: OneProps) {
  const body = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const thumbRoom = useRef<HTMLDivElement>(null);
  useThumbFade(body, thumbRoom);
  useSmoothHeight(content);
  const head = useRef<HTMLDivElement>(null);
  const factsBox = useRef<HTMLElement>(null);
  useLayoutEffect(() => {
    const facts = factsBox.current;
    const element = body.current;
    if (!facts || !element) return;
    const measure = () => element.style.setProperty("--facts-room", `${facts.offsetHeight + FACTS_ABOVE}px`);
    measure();
    const watching = new ResizeObserver(measure);
    watching.observe(facts);
    return () => watching.disconnect();
  }, []);
  const Icon = CATEGORY_ICONS[hit.category];
  const paging = pagingOf(hit.name);
  const paged = paging !== null;
  const wantsDetails = wantsDetailsOf(hit);
  const [details, setDetails] = useState<MatchDetails | null | undefined>(() => (wantsDetails ? detailsNow(hit, query) : undefined));
  useEffect(() => {
    if (!wantsDetails) return;
    const now = detailsNow(hit, query);
    if (now !== undefined) return setDetails(now);
    let current = true;
    const ask = () => void detailsFor(hit, query).then((found) => current && setDetails(found));
    const timer = window.setTimeout(ask, detailsAsked(hit, query) ? 0 : DETAILS_DELAY_MS);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, wantsDetails]);

  const [pressedPage, setPressedPage] = useState<number | null>(null);
  const wordsPage = details?.snippets.find((snippet) => snippet.page !== null)?.page ?? null;
  const closestPage = atIsPage(hit.name) ? hit.at : (details?.closest?.page ?? null);
  const waiting = paged && wantsDetails && (hit.reason === "words" || !atIsPage(hit.name)) && details === undefined;
  const page = paged ? (pressedPage ?? wordsPage ?? closestPage ?? 1) : 1;

  const small = usePreview({ path: hit.path, modified: hit.modified, kind: "file", readable: true, category: hit.category });
  const frame = useRef<HTMLDivElement>(null);
  const largeSize = useLargeSize(frame);
  const largeSrc = small.wanted && !waiting && largeSize !== null ? largePreviewUrl(hit.path, largeSize, page) : null;
  const [largeLoaded, setLargeLoaded] = useState<string | null>(null);
  const [shape, setShape] = useState(() => previewShape(hit) ?? null);
  const loadedSmall = (e: SyntheticEvent<HTMLImageElement>) => {
    small.onLoad(e);
    const { naturalWidth, naturalHeight } = e.currentTarget;
    if (shape === null && naturalWidth > 0 && naturalHeight > 0) setShape(naturalWidth / naturalHeight);
  };

  const viewable = small.wanted && hit.category !== "audio";
  const [viewing, setViewing] = useState<number | null>(null);
  const view = () => setViewing(page);
  const closeViewer = useCallback(() => setViewing(null), []);
  const codeBlock = useRef<HTMLElement>(null);
  const [codeShown, setCodeShown] = useState<ShownCode | null>(null);
  const closeCode = useCallback(() => setCodeShown(null), []);
  const rectOf = (element: HTMLElement | null) => () => (element?.isConnected ? element.getBoundingClientRect() : null);
  const showOwnCode = (line: number) =>
    setCodeShown({
      path: hit.path,
      name: hit.name,
      line,
      found: details?.code ? { start: details.code.start, end: details.code.end } : null,
      from: rectOf(codeBlock.current),
    });
  const [askedAlsoIn, setAskedAlsoIn] = useState<AlsoIn[]>([]);
  const shownStart = details?.code?.start;
  const cameWith = details?.alsoIn;
  useEffect(() => {
    if (hit.category !== "code" || shownStart === undefined || cameWith !== undefined) return;
    let live = true;
    api.alsoIn(hit.path, shownStart).then(
      (found) => live && setAskedAlsoIn(found),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [hit.category, hit.path, shownStart, cameWith]);
  const alsoIn = (cameWith ?? askedAlsoIn).filter((other) => !hit.copies.includes(other.path));

  useSteadyPicture(frame, layout === "middle" ? head : layout === "wide" ? factsBox : null);

  const reason = matchReason({ hit, query, withFiles, files, weak });
  const ReasonIcon = REASON_ICONS[reason.kind];
  const extension = extensionOf(hit.name);

  return (
    <div ref={body} className={styles.body}>
      <div ref={content} className={styles.single}>
      <div className={styles.file}>
        {hit.category === "code" ? (
          <CodeView
            code={details === undefined ? undefined : (details?.code ?? null)}
            blockRef={codeBlock}
            onExpand={() => showOwnCode(details?.code?.start ?? hit.at ?? 1)}
          />
        ) : (
          <div className={cx(styles.stage, hit.category === "document" && styles.pageStage)}>
            <div
              ref={frame}
              data-glide="picture"
              className={cx(styles.frame, shape !== null && styles.fitted, !small.wanted && styles.iconOnly, viewable && styles.zoomable)}
              style={shape !== null ? ({ "--shape": shape } as CSSProperties) : undefined}
              role={viewable ? "button" : undefined}
              tabIndex={viewable ? 0 : undefined}
              aria-label={viewable ? "Show larger" : undefined}
              onClick={viewable ? view : undefined}
              onKeyDown={
                viewable
                  ? (e) => {
                      if (e.key !== "Enter" && e.key !== " ") return;
                      e.preventDefault();
                      view();
                    }
                  : undefined
              }
            >
              <Icon className={styles.icon} size={56} strokeWidth={1} aria-hidden />
              {small.wanted && (
                <img className={cx(styles.image, small.loaded && styles.loaded)} src={small.src} alt="" draggable={false} decoding="async" onLoad={loadedSmall} onError={small.onError} />
              )}
              {largeSrc && (
                <img
                  key={largeSrc}
                  className={cx(styles.image, largeLoaded === largeSrc && styles.loaded)}
                  src={largeSrc}
                  alt=""
                  draggable={false}
                  decoding="async"
                  onLoad={() => setLargeLoaded(largeSrc)}
                />
              )}
              {paging && page > 1 && (
                <span className={styles.pageBadge}>
                  {pagingWord(paging)} {page}
                </span>
              )}
            </div>
          </div>
        )}
        {codeShown !== null && (
          <CodeViewer
            path={codeShown.path}
            name={codeShown.name}
            query={codeShown.path === hit.path ? query : ""}
            line={codeShown.line}
            found={codeShown.found}
            from={codeShown.from}
            onOpen={(line) => actions.openAt({ ...hit, path: codeShown.path, name: codeShown.name }, line)}
            onClosed={closeCode}
          />
        )}
        {viewing !== null && (
          <PreviewViewer
            hit={hit}
            page={viewing}
            from={{
              rect: () => (frame.current?.isConnected ? frame.current.getBoundingClientRect() : null),
              src: largeSrc !== null && largeLoaded === largeSrc ? largeSrc : small.loaded ? small.src : null,
              shape,
            }}
            onOpen={() => actions.open(hit)}
            onClosed={closeViewer}
          />
        )}
        <section ref={factsBox} className={cx(styles.section, styles.factsSection)} data-glide>
          <dl className={styles.facts}>
            <dt>Type</dt>
            <dd>
              {CATEGORY_LABELS[hit.category]}
              {hit.category === "code" ? ` · ${details?.code?.language ?? codeLanguageOf(hit.name) ?? extension}` : extension && ` · ${extension}`}
            </dd>
            <dt>Size</dt>
            <dd>{formatSize(hit.size, true)}</dd>
            <dt>Modified</dt>
            <dd>{formatModified(hit.modified)}</dd>
            <dt>Folder</dt>
            <dd>
              <PathText path={parentOf(hit.path)} />
            </dd>
          </dl>
        </section>
      </div>

      <div className={styles.info}>
      <div ref={head} className={styles.head}>
      <h2 className={styles.name} data-glide>{hit.name}</h2>
      <p className={styles.location} data-tooltip={hit.path} data-glide>
        {parentOf(hit.path)}
      </p>

      <div className={styles.actions}>
        <Button variant="primary" className={styles.actionButton} onClick={() => actions.open(hit)} data-glide>
          Open
        </Button>
        <Button variant="neutral" className={styles.actionButton} onClick={() => onFindSimilar([hit.path])} data-tooltip="Search for files like this one" data-glide>
          <ScanSearch size={16} aria-hidden />
          Find similar
        </Button>
      </div>
      <div className={styles.quiet} data-glide>
        <div className={styles.quietRow}>
          <CopyButton words="Copy file" tooltip={`Copy file (${shortcut("C")})`} icon={Copy} copy={() => actions.copyFiles([hit])} />
          <CopyButton words="Copy path" tooltip={`Copy path (${shortcut("C", true)})`} icon={Link} copy={() => actions.copyPaths([hit])} />
        </div>
        <div className={styles.quietRow}>
          <button type="button" className={styles.quietButton} onClick={() => actions.showInFolder(hit)}>
            <FolderOpen size={14} aria-hidden />
            Show in {fileManager}
          </button>
        </div>
      </div>

      </div>

      <section className={cx(styles.section, styles.whySection)} data-glide>
        <p className={cx(styles.reason, reason.faint && styles.faint)}>
          <ReasonIcon size={14} aria-hidden />
          <span>
            {reason.text}
            {reason.detail && <span className={styles.reasonDetail}>{reason.detail}</span>}
          </span>
        </p>
        {wantsDetails && details === undefined && <p className={styles.loading}>Finding where it matches…</p>}
        {hit.reason === "words" && hit.category !== "code" && details && details.snippets.length > 0 && (
          <div className={styles.snippets}>
            {details.snippets.map((snippet, i) => (
              <Snippet key={i} snippet={snippet} paging={paging} current={paged && snippet.page === page} onPage={paged ? setPressedPage : undefined} />
            ))}
          </div>
        )}
        {hit.category === "code" && (hit.copies.length > 0 || alsoIn.length > 0) && (
          <>
            <h4 className={styles.subheading}>Also in</h4>
            <div className={styles.snippets}>
              {hit.copies.map((path) => (
                <AlsoInRow key={path} path={path} detail="Identical file" onShow={(from) => setCodeShown({ path, name: fileNameOf(path), line: shownStart ?? 1, found: null, from })} />
              ))}
              {alsoIn.map((other) => (
                <AlsoInRow
                  key={other.path}
                  path={other.path}
                  symbol={other.symbol}
                  detail={`Line ${other.line} · ${Math.round(other.similarity * 100)}% alike`}
                  onShow={(from) => setCodeShown({ path: other.path, name: fileNameOf(other.path), line: other.line, found: null, from })}
                />
              ))}
            </div>
          </>
        )}
        {hit.category === "code" && details && details.snippets.length > 0 && (
          <>
            <h4 className={styles.subheading}>Elsewhere in this file</h4>
            <div className={styles.snippets}>
              {details.snippets.map((snippet, i) => (
                <Snippet key={i} snippet={snippet} paging={null} current={false} lines onPage={showOwnCode} />
              ))}
            </div>
          </>
        )}
        {details?.closest && hit.reason !== "words" && (
          <>
            <h4 className={styles.subheading}>Closest passage</h4>
            <div className={styles.snippets}>
              <Snippet
                snippet={details.closest}
                paging={paging}
                current={paged && details.closest.page === page}
                onPage={paged ? setPressedPage : undefined}
              />
            </div>
          </>
        )}
      </section>

      </div>
      </div>
      <div ref={thumbRoom} aria-hidden />
    </div>
  );
}

const PATH_LINES = 2;

function PathText({ path }: { path: string }) {
  const cuts = useMemo(() => middleCuts(path), [path]);
  const [shown, setShown] = useState(0);
  const box = useRef<HTMLSpanElement>(null);
  const probe = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const element = box.current;
    const copy = probe.current;
    if (!element || !copy) return;
    let width = -1;
    const fit = () => {
      if (element.clientWidth === width) return;
      width = element.clientWidth;
      const most = parseFloat(getComputedStyle(element).lineHeight) * PATH_LINES + 1;
      let chosen = cuts.length - 1;
      for (let at = 0; at < cuts.length; at++) {
        copy.replaceChildren(
          ...cuts[at].map((part) => {
            const piece = document.createElement("span");
            piece.className = styles.pathPart;
            piece.textContent = part;
            return piece;
          }),
        );
        if (copy.getBoundingClientRect().height <= most) {
          chosen = at;
          break;
        }
      }
      copy.replaceChildren();
      setShown(chosen);
    };
    fit();
    const watching = new ResizeObserver(fit);
    watching.observe(element);
    return () => watching.disconnect();
  }, [cuts]);
  const parts = cuts[Math.min(shown, cuts.length - 1)];
  return (
    <span ref={box} className={styles.path} data-tooltip={shown > 0 ? path : undefined}>
      <span ref={probe} className={styles.pathProbe} aria-hidden />
      {parts.map((part, i) => (
        <span key={i} className={styles.pathPart}>
          {part}
        </span>
      ))}
    </span>
  );
}

function Thumb({ hit, more }: { hit: SearchHit; more: number }) {
  const Icon = CATEGORY_ICONS[hit.category];
  const { wanted, src, loaded, onLoad, onError } = usePreview({ path: hit.path, modified: hit.modified, kind: "file", readable: true, category: hit.category });
  return (
    <span className={styles.thumb}>
      <Icon className={styles.icon} size={24} strokeWidth={1.25} aria-hidden />
      {wanted && <img className={cx(styles.image, styles.cover, loaded && styles.loaded)} src={src} alt="" draggable={false} decoding="async" onLoad={onLoad} onError={onError} />}
      {hit.category === "code" && <CodeThumb path={hit.path} modified={hit.modified} line={hit.at} />}
      {more > 0 && <span className={styles.more}>+{more}</span>}
    </span>
  );
}

const KIND_NOUNS: Record<SearchHit["category"], [string, string]> = {
  image: ["image", "images"],
  video: ["video", "videos"],
  audio: ["audio file", "audio files"],
  document: ["document", "documents"],
  code: ["code file", "code files"],
};

export function describeKinds(hits: Pick<SearchHit, "category">[]): string {
  const counts = new Map<SearchHit["category"], number>();
  for (const hit of hits) counts.set(hit.category, (counts.get(hit.category) ?? 0) + 1);
  return [...counts].map(([category, count]) => `${count} ${KIND_NOUNS[category][count === 1 ? 0 : 1]}`).join(", ");
}

function SeveralFiles({ hits, actions, onFindSimilar, onSelectOnly }: { hits: SearchHit[]; actions: ResultActions; onFindSimilar: (paths: string[]) => void; onSelectOnly: (path: string) => void }) {
  const body = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const thumbRoom = useRef<HTMLDivElement>(null);
  useThumbFade(body, thumbRoom);
  useSmoothHeight(content);
  const size = hits.reduce((total, hit) => total + hit.size, 0);
  const pictured = hits.slice(0, MOSAIC_MOST);
  return (
    <div ref={body} className={styles.body}>
      <div ref={content}>
      <div className={styles.mosaic}>
        {pictured.map((hit, i) => (
          <Thumb key={hit.path} hit={hit} more={i === pictured.length - 1 ? hits.length - pictured.length : 0} />
        ))}
      </div>
      <h2 className={styles.name}>{hits.length} files selected</h2>
      <p className={styles.location}>
        {describeKinds(hits)} · {formatSize(size, true)}
      </p>

      <div className={styles.actions}>
        <Button
          variant="primary"
          className={styles.actionButton}
          onClick={() => onFindSimilar(hits.map((hit) => hit.path))}
          disabled={hits.length > MAX_EXAMPLES}
          data-tooltip={hits.length > MAX_EXAMPLES ? `Up to ${MAX_EXAMPLES} files: select fewer` : "Search for files like all of these"}
        >
          <ScanSearch size={16} aria-hidden />
          Find similar to these
        </Button>
      </div>
      <div className={styles.quiet}>
        <div className={styles.quietRow}>
          <CopyButton words="Copy files" tooltip={`Copy files (${shortcut("C")})`} icon={Copy} copy={() => actions.copyFiles(hits)} />
          <CopyButton words="Copy paths" tooltip={`Copy paths (${shortcut("C", true)})`} icon={Link} copy={() => actions.copyPaths(hits)} />
        </div>
      </div>

      <section className={styles.section}>
        <h3 className={styles.heading}>Selected</h3>
        <ul className={styles.names}>
          {hits.slice(0, NAMED_MOST).map((hit) => {
            const Icon = CATEGORY_ICONS[hit.category];
            return (
              <li key={hit.path}>
                <button type="button" className={styles.nameButton} onClick={() => onSelectOnly(hit.path)} data-tooltip="Select only this one">
                  <Icon size={14} aria-hidden />
                  <span>{hit.name}</span>
                </button>
              </li>
            );
          })}
        </ul>
        {hits.length > NAMED_MOST && <p className={styles.loading}>and {hits.length - NAMED_MOST} more</p>}
      </section>
      </div>
      <div ref={thumbRoom} aria-hidden />
    </div>
  );
}
