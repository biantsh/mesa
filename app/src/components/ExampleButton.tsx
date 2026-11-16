import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type Category } from "../api";
import { cx } from "../cx";
import { EXAMPLE_KINDS, MAX_EXAMPLES, categoryOfPath } from "../examples";
import { CATEGORY_ICONS } from "./ResultGrid";
import { SlidingIcon } from "./SlidingIcon";
import styles from "./ExampleButton.module.css";

const CYCLE_MS = 2800;

const FILTERS = [
  { name: "All supported files", extensions: EXAMPLE_KINDS.flatMap((kind) => kind.extensions) },
  ...EXAMPLE_KINDS.map((kind) => ({ name: kind.label, extensions: kind.extensions })),
];

function useCycle(still: boolean, restart: string): Category {
  const [index, setIndex] = useState(0);
  useEffect(() => {
    if (still) return;
    const timer = setInterval(() => setIndex((now) => (now + 1) % EXAMPLE_KINDS.length), CYCLE_MS);
    return () => clearInterval(timer);
  }, [still, restart]);
  return EXAMPLE_KINDS[index].category;
}

interface Props {
  onPick: (paths: string[]) => void;
  className?: string;
  hidden?: boolean;
  calmFor?: string;
  full?: boolean;
}

export function ExampleButton({ onPick, className, hidden = false, calmFor = "", full = false }: Props) {
  const [picking, setPicking] = useState(false);
  const kind = useCycle(picking || hidden, calmFor);

  const choose = async () => {
    if (picking || full) return;
    setPicking(true);
    await api.setPicking(true).catch(() => {});
    try {
      const picked = await open({
        multiple: true,
        directory: false,
        title: "Choose files to search with",
        filters: FILTERS,
      });
      const usable = (picked ?? []).filter((path) => categoryOfPath(path) !== null);
      if (usable.length > 0) onPick(usable);
    } catch {
    } finally {
      await api.setPicking(false).catch(() => {});
      setPicking(false);
    }
  };

  return (
    <button
      type="button"
      className={cx(styles.button, picking && styles.open, hidden && styles.hidden, full && styles.full, className)}
      aria-label={full ? `Search with a file (up to ${MAX_EXAMPLES} files: there are as many as can be)` : "Search with a file"}
      aria-disabled={full}
      data-tooltip={full ? `Up to ${MAX_EXAMPLES} files` : "Search with a file"}
      onMouseDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
      onClick={() => void choose()}
    >
      <SlidingIcon icons={CATEGORY_ICONS} current={kind} />
    </button>
  );
}
