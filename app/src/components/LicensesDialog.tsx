import { useEffect, useId, useRef, useState, type RefObject } from "react";
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
  const body = useRef<HTMLDivElement>(null);
  useWheelStopsAtEnds(body, open);

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
              notices && <Sections notices={notices} />
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

function Sections({ notices }: { notices: Notices }) {
  return (
    <>
      {notices.sections.map((section) => (
        <section key={section.id} className={styles.group} aria-label={section.title}>
          {!(section.items.length === 1 && section.items[0].name === section.title) && (
            <h3 className={styles.groupTitle}>
              {section.title}
              {section.items.length > 1 && <span className={styles.count}>{section.items.length}</span>}
            </h3>
          )}
          <div className={styles.items}>
            {section.items.map((item, i) => (
              <Item key={`${item.name} ${item.version ?? ""}`} item={item} texts={notices.texts} startOpen={section.id === "mesa" && i === 0} />
            ))}
          </div>
        </section>
      ))}
    </>
  );
}

function Item({ item, texts, startOpen }: { item: NoticeItem; texts: string[]; startOpen: boolean }) {
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
}

function linkLabel(url: string): string {
  const host = new URL(url).hostname;
  if (host === "crates.io") return "crates.io";
  if (host.endsWith("npmjs.com")) return "npm";
  if (host === "huggingface.co") return "Hugging Face";
  if (host === "github.com") return "GitHub";
  return host;
}
