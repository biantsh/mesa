import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowUpRight, Database, Search, Settings } from "lucide-react";
import { api, type ModelStatus, type Overview } from "../api";
import { describeIndexTotals } from "../format";
import { Button } from "../components/Button";
import { IndexStatus } from "../components/IndexStatus";
import logo from "../logo";
import styles from "./HomeScreen.module.css";

const LICENSE = "MIT License";

interface Props {
  overview: Overview | null;
  model: ModelStatus | null;
  onRetryModel: () => void;
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

export function HomeScreen({ overview, model, onRetryModel, onOpenIndex, onOpenSettings }: Props) {
  const blocked = overview?.empty === true;
  const modelFailed = model?.state === "failed";
  const input = useRef<HTMLInputElement>(null);
  const [attention, setAttention] = useState(0);
  const refuse = useCallback(() => setAttention((count) => count + 1), []);
  useEffect(() => {
    if (blocked) input.current?.blur();
  }, [blocked]);

  return (
    <main className={styles.home}>
      <img className={styles.logo} src={logo} alt="Mesa" draggable={false} />
      <div className={styles.search}>
        <div className={styles.row}>
          <label
            className={styles.field}
            onMouseDown={
              modelFailed
                ? (e) => { e.preventDefault(); onRetryModel(); }
                : blocked ? (e) => { e.preventDefault(); refuse(); } : undefined
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
              onFocus={blocked ? (e) => { e.currentTarget.blur(); refuse(); } : undefined}
              spellCheck={false}
              autoComplete="off"
            />
          </label>
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
        <IndexStatus overview={overview} onOpenIndex={onOpenIndex} attention={attention} />
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
