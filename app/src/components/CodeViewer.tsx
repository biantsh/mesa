import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type MouseEvent } from "react";
import { X } from "lucide-react";
import { api, type CodeFile, type CodePart } from "../api";
import { cx } from "../cx";
import { prefersReducedMotion } from "../motion";
import { Button } from "./Button";
import { CodeLine } from "./CodeView";
import { IconButton } from "./IconButton";
import { Modal } from "./Modal";
import { EASE_CURVE, EASE_MS } from "./PanelSplitter";
import viewer from "./PreviewViewer.module.css";
import styles from "./CodeViewer.module.css";

const LEAVING_MS = 180;
const SMALL = 0.92;
const SLOW_MS = 250;
const LINE = 20;
const ROOM = 10;
const AROUND = 80;
const TAB = 4;

function columnsOf(parts: CodePart[]): number {
  let column = 0;
  for (const part of parts) {
    for (const c of part.text) column = c === "\t" ? column + TAB - (column % TAB) : column + 1;
  }
  return column;
}

interface Props {
  path: string;
  name: string;
  query: string;
  line: number;
  found: { start: number; end: number } | null;
  from: () => DOMRect | null;
  onOpen: (line: number) => void;
  onClosed: () => void;
}

export function CodeViewer({ path, name, query, line, found, from, onOpen, onClosed }: Props) {
  const frame = useRef<HTMLDivElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const [file, setFile] = useState<CodeFile | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    api.codeFile(path, query).then(
      (got) => live && setFile(got),
      () => live && setFile(null),
    );
    return () => {
      live = false;
    };
  }, [path, query]);

  const [first, setFrom] = useState(0);
  const [shownRows, setShownRows] = useState(60);
  const follow = () => {
    const room = scroller.current;
    if (!room) return;
    const step = AROUND / 2;
    setFrom(Math.max(0, Math.floor((room.scrollTop - ROOM) / LINE / step) * step - AROUND));
    setShownRows(Math.ceil(room.clientHeight / LINE) + 2 * AROUND + step);
  };
  useEffect(() => {
    const room = scroller.current;
    if (!room) return;
    const observer = new ResizeObserver(follow);
    observer.observe(room);
    return () => observer.disconnect();
  }, []);
  const size = useMemo(() => {
    if (!file) return null;
    const digits = String(file.lines.length).length;
    const columns = file.lines.reduce((most, parts) => Math.max(most, columnsOf(parts)), 0);
    return { digits, columns };
  }, [file]);

  useLayoutEffect(() => {
    const room = scroller.current;
    if (!file || !room) return;
    room.scrollTop = Math.max(0, ROOM + (line - 1) * LINE - room.clientHeight / 5);
    follow();
  }, [file, line]);

  const [slow, setSlow] = useState(false);
  useEffect(() => {
    const timer = window.setTimeout(() => setSlow(true), SLOW_MS);
    return () => window.clearTimeout(timer);
  }, []);
  const ready = file !== undefined || slow;
  const appeared = useRef(false);
  useLayoutEffect(() => {
    const box = frame.current;
    if (!ready || appeared.current || !box) return;
    appeared.current = true;
    box.style.visibility = "visible";
    if (prefersReducedMotion()) return;
    aim(box, from());
    box.animate([{ transform: `scale(${SMALL})`, opacity: 0 }, { transform: "none", opacity: 1 }], { duration: EASE_MS, easing: EASE_CURVE });
  }, [ready, from]);
  const [leaving, setLeaving] = useState(false);
  const [shut, setShut] = useState(false);
  const leave = () => {
    if (leaving) return;
    setLeaving(true);
    const box = frame.current;
    if (!box || !appeared.current || prefersReducedMotion()) return setShut(true);
    aim(box, from());
    const going = box.animate([{ transform: "none", opacity: 1 }, { transform: `scale(${SMALL})`, opacity: 0 }], {
      duration: LEAVING_MS,
      easing: "ease-in",
      fill: "forwards",
    });
    going.onfinish = () => setShut(true);
    going.oncancel = () => setShut(true);
  };
  useEffect(() => {
    if (shut) onClosed();
  }, [shut, onClosed]);

  const onClick = (e: MouseEvent) => {
    if (!(e.target as Element).closest("[data-viewer-keep]")) leave();
  };
  const shaded = (n: number) => found !== null && n >= found.start && n <= found.end;
  return (
    <Modal open={!shut} onClose={leave} className={cx(viewer.viewer, leaving && viewer.leaving)} label={name} focusCard>
      <div className={viewer.content} onClick={onClick}>
        <div className={viewer.stage}>
          <div ref={frame} className={styles.frame} data-viewer-keep>
            <div ref={scroller} className={styles.scroll} onScroll={follow}>
              {file === undefined && <p className={styles.message}>Reading the file…</p>}
              {file === null && <p className={styles.message}>This file can’t be shown.</p>}
              {file && size && (
                <div
                  className={styles.code}
                  style={{ "--digits": size.digits, "--columns": size.columns, height: ROOM * 2 + file.lines.length * LINE } as CSSProperties}
                >
                  <div className={styles.rows} style={{ top: ROOM + first * LINE }}>
                    {file.lines.slice(first, first + shownRows).map((parts, i) => {
                      const n = first + i + 1;
                      return (
                        <div key={n} className={cx(styles.row, shaded(n) && styles.found)}>
                          <span className={styles.lineNumber}>{n}</span>
                          <span className={styles.text}>
                            <CodeLine parts={parts} />
                          </span>
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}
              {file && file.total > file.lines.length && (
                <p className={styles.message}>{(file.total - file.lines.length).toLocaleString()} more lines (open it to see them)</p>
              )}
            </div>
          </div>
        </div>
        <div className={viewer.bar} data-viewer-keep>
          <span className={viewer.name} data-tooltip={name}>
            {name}
          </span>
          {file && <span className={styles.language}>{file.language}</span>}
          <Button variant="primary" onClick={() => onOpen(line)}>
            Open
          </Button>
          <IconButton label="Close" onClick={leave}>
            <X size={18} aria-hidden />
          </IconButton>
        </div>
      </div>
    </Modal>
  );
}

function aim(box: HTMLElement, toward: DOMRect | null) {
  const at = box.getBoundingClientRect();
  if (!toward || at.width === 0) {
    box.style.transformOrigin = "50% 50%";
    return;
  }
  const x = Math.min(Math.max(toward.left + toward.width / 2, at.left), at.right) - at.left;
  const y = Math.min(Math.max(toward.top + toward.height / 2, at.top), at.bottom) - at.top;
  box.style.transformOrigin = `${x}px ${y}px`;
}
