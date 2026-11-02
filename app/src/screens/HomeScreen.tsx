import { useCallback, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type ReactNode, type Ref } from "react";
import { ArrowRight, ArrowUpRight, Database, Image as ImageIcon, Search, Settings } from "lucide-react";
import { api, type ModelStatus, type Overview } from "../api";
import { describeIndexTotals } from "../format";
import { Button } from "../components/Button";
import { IndexStatus } from "../components/IndexStatus";
import { cx } from "../cx";
import logo from "../logo";
import styles from "./HomeScreen.module.css";

const LICENSE = "MIT License";
const TOP_GAP_PX = 20;
const BOTTOM_GAP_PX = 16;

export interface HomeHandle {
  restoreFocus: () => boolean;
}

interface Props {
  ref?: Ref<HomeHandle>;
  overview: Overview | null;
  model: ModelStatus | null;
  onRetryModel: () => void;
  onFocusChange: (focused: boolean) => void;
  onOpenIndex: () => void;
  onOpenSettings: () => void;
}

function placeholder(model: ModelStatus | null): string {
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

interface Understood {
  fileType: string | null;
  from: string | null;
  to: string | null;
  text: string;
}

function understand(query: string): Understood {
  if (query.trim() === "") return { fileType: null, from: null, to: null, text: "" };
  return { fileType: "Image", from: "Apr 2023", to: "Jul 2023", text: query };
}

function Token({ icon, ghost, children }: { icon?: ReactNode; ghost?: boolean; children: ReactNode }) {
  return (
    <span className={cx(styles.token, ghost && styles.ghost)}>
      {icon}
      {children}
    </span>
  );
}

export function HomeScreen({ ref, overview, model, onRetryModel, onFocusChange, onOpenIndex, onOpenSettings }: Props) {
  const blocked = overview?.empty === true;
  const modelFailed = model?.state === "failed";
  const input = useRef<HTMLInputElement>(null);
  const home = useRef<HTMLElement>(null);
  const search = useRef<HTMLDivElement>(null);
  const actions = useRef<HTMLDivElement>(null);
  const row = useRef<HTMLDivElement>(null);
  const [attention, setAttention] = useState(0);
  const [focused, setFocused] = useState(false);
  const [query, setQuery] = useState("");
  const understood = understand(query);
  const refuse = useCallback(() => setAttention((count) => count + 1), []);
  const setFocus = useCallback((next: boolean) => {
    setFocused(next);
    onFocusChange(next);
  }, [onFocusChange]);
  const away = useRef(false);
  useImperativeHandle(ref, () => ({
    restoreFocus() {
      if (!away.current) return false;
      away.current = false;
      if (!input.current || input.current.readOnly) return false;
      input.current.focus({ preventScroll: true });
      return true;
    },
  }));
  const openIndexFromProgress = useCallback(() => {
    away.current = focused;
    onOpenIndex();
  }, [focused, onOpenIndex]);
  useEffect(() => {
    if (!blocked) return;
    away.current = false;
    input.current?.blur();
    setFocus(false);
  }, [blocked, setFocus]);

  useLayoutEffect(() => {
    const root = home.current;
    if (!root) return;
    const measure = () => {
      if (!search.current || !actions.current || !row.current) return;
      const around = root.getBoundingClientRect();
      const block = search.current.getBoundingClientRect();
      const gap = parseFloat(getComputedStyle(row.current).columnGap) || 0;
      root.style.setProperty("--lift", `${Math.max(0, block.top - around.top - TOP_GAP_PX)}px`);
      root.style.setProperty("--drop", `${Math.max(0, around.bottom - BOTTOM_GAP_PX - block.bottom)}px`);
      root.style.setProperty("--actions-width", `${actions.current.offsetWidth + gap}px`);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    observer.observe(actions.current!);
    return () => observer.disconnect();
  }, []);

  return (
    <main ref={home} className={cx(styles.home, focused && styles.focused)}>
      <div className={styles.scrim} aria-hidden />
      <img className={styles.logo} src={logo} alt="Mesa" draggable={false} />
      <div ref={search} className={styles.search}>
        <div ref={row} className={styles.row}>
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
              placeholder={placeholder(model)}
              title={model?.state === "failed" ? model.reason : undefined}
              aria-label="Search"
              readOnly={blocked}
              aria-disabled={blocked || undefined}
              tabIndex={blocked ? -1 : undefined}
              onFocus={blocked ? (e) => { e.currentTarget.blur(); refuse(); } : () => setFocus(true)}
              onBlur={() => { if (document.hasFocus() && !away.current) setFocus(false); }}
              onKeyDown={(e) => { if (e.key === "Escape") e.currentTarget.blur(); }}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
            <div className={styles.extra} aria-hidden={!focused}>
              <div className={styles.tags}>
                <div className={styles.parsed}>
                  <div>
                    <div className={styles.parsedLabel}>File type</div>
                    <div className={styles.parsedValue}>
                      {understood.fileType ? (
                        <Token icon={<ImageIcon size={12} aria-hidden />}>{understood.fileType}</Token>
                      ) : (
                        <Token ghost>Any type</Token>
                      )}
                    </div>
                  </div>
                  <div>
                    <div className={styles.parsedLabel}>Date range</div>
                    <div className={styles.parsedValue}>
                      {understood.from && understood.to ? (
                        <>
                          <Token>{understood.from}</Token>
                          <ArrowRight className={styles.range} size={12} aria-hidden />
                          <Token>{understood.to}</Token>
                        </>
                      ) : (
                        <Token ghost>Any time</Token>
                      )}
                    </div>
                  </div>
                  <div>
                    <div className={styles.parsedLabel}>Query</div>
                    <div className={cx(styles.parsedValue, styles.parsedText, !understood.text && styles.parsedNone)}>
                      {understood.text || "Anything"}
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </label>
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
          <IndexStatus overview={overview} onOpenIndex={openIndexFromProgress} attention={attention} raised={focused} />
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
    </main>
  );
}
