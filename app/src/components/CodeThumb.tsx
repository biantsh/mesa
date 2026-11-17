import { useEffect, useState } from "react";
import { api, type CodePart } from "../api";
import { cx } from "../cx";
import { CodeLine } from "./CodeView";
import styles from "./CodeThumb.module.css";

const kept = new Map<string, CodePart[][] | null>();
const MOST = 400;

interface Props {
  path: string;
  modified: number;
  line: number | null;
}

export function CodeThumb({ path, modified, line }: Props) {
  const key = `${path}\n${modified}\n${line ?? ""}`;
  const [lines, setLines] = useState<CodePart[][] | null | undefined>(() => kept.get(key));
  useEffect(() => {
    if (kept.has(key)) {
      setLines(kept.get(key));
      return;
    }
    let live = true;
    api.codePeek(path, line).then(
      (found) => {
        if (kept.size >= MOST) kept.delete(kept.keys().next().value!);
        kept.set(key, found);
        if (live) setLines(found);
      },
      () => {
        kept.set(key, null);
        if (live) setLines(null);
      },
    );
    return () => {
      live = false;
    };
  }, [key, path, line]);
  if (!lines) return null;
  return (
    <span className={cx(styles.thumb)} aria-hidden>
      {lines.map((parts, i) => (
        <span key={i} className={styles.line}>
          <CodeLine parts={parts} />
          {"\n"}
        </span>
      ))}
    </span>
  );
}
