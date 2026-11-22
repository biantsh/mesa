import { memo, useEffect, useId, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { ArrowUpRight, ChevronRight, X } from "lucide-react";
import { api, type NoticeItem, type Notices } from "../api";
import { cx } from "../cx";
import { isMac, isWebKit } from "../platform";
import { IconButton } from "./IconButton";
import { Modal } from "./Modal";
import styles from "./LicensesDialog.module.css";

interface Props {
  open: boolean;
  onClose: () => void;
}

let asked: Promise<Notices> | null = null;
function loadNotices(): Promise<Notices> {
  asked ??= api.notices().catch((e: unknown) => {
    asked = null;
    throw e;
  });
  return asked;
}

export function LicensesDialog({ open, onClose }: Props) {
  const titleId = useId();
  const [notices, setNotices] = useState<Notices | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!open || notices) return;
    let live = true;
    loadNotices().then(
      (loaded) => live && setNotices(loaded),
      (e: unknown) => live && setError(String(e)),
    );
    return () => {
      live = false;
    };
  }, [open, notices]);
  // The list has hundreds of entries, so only the first render with the card and the rest follow a chunk per frame,
  // keeping its entrance smooth. Spacers of the right height stand in for rows not built yet, so the scrollbar holds.
  const [limit, setLimit] = useState(FIRST_ROWS);
  useEffect(() => {
    if (!open || !notices) {
      setLimit(FIRST_ROWS);
      return;
    }
    if (limit >= notices.sections.reduce((n, section) => n + section.items.length, 0)) return;
    const frame = requestAnimationFrame(() => setLimit((n) => n + CHUNK_ROWS));
    return () => cancelAnimationFrame(frame);
  }, [open, notices, limit]);
  const body = useRef<HTMLDivElement>(null);
  useWheelStopsAtEnds(body, open);
  // Measured once rows are on screen: at once if the notices arrive while the card is showing, or after Modal's effect
  // shows it if they were already loaded (child effects run first).
  useLayoutEffect(() => fitUnbuilt(body.current), [open, notices]);
  useEffect(() => fitUnbuilt(body.current), [open, notices]);

  return (
    <Modal open={open} onClose={onClose} className={styles.dialog} labelledBy={titleId} focusCard={isWebKit} modeless>
      {open && (
        <div className={styles.content}>
          <header className={styles.header}>
            <h2 id={titleId} className={styles.title}>
              Licenses
            </h2>
            <IconButton label="Close licenses" onClick={onClose}>
              <X size={18} aria-hidden />
            </IconButton>
          </header>
          <div ref={body} className={styles.body}>
            {error ? (
              <p className={styles.message}>The licenses couldn't be loaded: {error}</p>
            ) : notices?.missing ? (
              <p className={styles.message}>
                The licenses weren't gathered for this build: run <code>npm run build:runtime</code> in app/.
              </p>
            ) : (
              notices && <Sections notices={notices} limit={limit} />
            )}
          </div>
        </div>
      )}
    </Modal>
  );
}

// WebView2 at fractional scales scrolls a pixel past the end and then snaps back, so the wheel is ignored at the ends
// (except on macOS, where the bounce is wanted).
function useWheelStopsAtEnds(view: RefObject<HTMLElement | null>, active: boolean) {
  useEffect(() => {
    const element = view.current;
    if (!active || !element || isMac) return;
    const onWheel = (e: WheelEvent) => {
      if (e.ctrlKey || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
      const atEnd = e.deltaY > 0 ? element.scrollTop >= element.scrollHeight - element.clientHeight - 1 : element.scrollTop <= 0;
      if (atEnd) e.preventDefault();
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, [view, active]);
}

const FIRST_ROWS = 30;
const CHUNK_ROWS = 40;

// Collapsed rows all share a height, divider included. It's averaged over the longest run of them, since at fractional
// scales one row's box is off by a fraction of a pixel.
function fitUnbuilt(body: HTMLElement | null) {
  if (!body || body.style.getPropertyValue("--row")) return;
  let longest = { first: null as Element | null, last: null as Element | null, rows: 0 };
  for (const items of Array.from(body.querySelectorAll(`.${styles.items}`))) {
    let run = { first: null as Element | null, last: null as Element | null, rows: 0 };
    for (const row of Array.from(items.children)) {
      if (!row.querySelector(":scope > [aria-expanded=false]")) run = { first: null, last: null, rows: 0 };
      else run = { first: run.first ?? row, last: row, rows: run.rows + 1 };
      if (run.rows > longest.rows) longest = run;
    }
  }
  if (!longest.first || !longest.last || longest.rows < 2) return;
  const step = (longest.last.getBoundingClientRect().top - longest.first.getBoundingClientRect().top) / (longest.rows - 1);
  if (step > 0) body.style.setProperty("--row", `${step}px`);
}

function Sections({ notices, limit }: { notices: Notices; limit: number }) {
  let left = limit;
  return (
    <>
      {notices.sections.map((section) => {
        const items = section.items.slice(0, Math.max(left, 0));
        left -= items.length;
        const unbuilt = section.items.length - items.length;
        return (
          <section key={section.id} className={styles.group} aria-label={section.title}>
            {!(section.items.length === 1 && section.items[0].name === section.title) && (
              <h3 className={styles.groupTitle}>
                {section.title}
                {section.items.length > 1 && <span className={styles.count}>{section.items.length}</span>}
              </h3>
            )}
            <div className={styles.items}>
              {items.map((item, i) => (
                <Item key={`${item.name} ${item.version ?? ""}`} item={item} texts={notices.texts} startOpen={section.id === "mesa" && i === 0} />
              ))}
              {unbuilt > 0 && <div className={styles.unbuilt} style={{ height: `calc(${unbuilt} * var(--row) - 1px)` }} aria-hidden />}
            </div>
          </section>
        );
      })}
    </>
  );
}

const Item = memo(function Item({ item, texts, startOpen }: { item: NoticeItem; texts: string[]; startOpen: boolean }) {
  const [expanded, setExpanded] = useState(startOpen);
  const panelId = useId();
  const url = item.url;
  return (
    <div className={styles.item}>
      <button
        type="button"
        className={styles.itemHead}
        aria-expanded={expanded}
        aria-controls={panelId}
        onClick={() => setExpanded((was) => !was)}
      >
        <ChevronRight size={14} className={cx(styles.chevron, expanded && styles.chevronOpen)} aria-hidden />
        <span className={styles.name}>{item.name}</span>
        {item.version && <span className={styles.version}>{item.version}</span>}
        <span className={styles.license}>{item.license}</span>
      </button>
      {expanded && (
        <div id={panelId} className={styles.panel}>
          {(item.by || item.note || url) && (
            <p className={styles.about}>
              {item.by && <span>By {item.by}. </span>}
              {item.note && <span>{item.note} </span>}
              {url && (
                <button type="button" className={styles.link} onClick={() => api.openNoticeLink(url).catch(() => {})}>
                  {linkLabel(url)}
                  <ArrowUpRight size={12} aria-hidden />
                </button>
              )}
            </p>
          )}
          {item.texts.map((i) => (
            <pre key={i} className={styles.text}>
              {texts[i]}
            </pre>
          ))}
        </div>
      )}
    </div>
  );
});

function linkLabel(url: string): string {
  const host = new URL(url).hostname;
  if (host === "crates.io") return "crates.io";
  if (host.endsWith("npmjs.com")) return "npm";
  if (host === "huggingface.co") return "Hugging Face";
  if (host === "github.com") return "GitHub";
  return host;
}
