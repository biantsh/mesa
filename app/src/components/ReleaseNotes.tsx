import { useEffect, useRef, useState, type ReactNode } from "react";
import { readNotes, type Inline, type ListItem } from "../releaseNotes";
import { useWheelStopsAtEnds } from "../useWheelStopsAtEnds";
import styles from "./ReleaseNotes.module.css";

export function ReleaseNotes({ notes }: { notes: string }) {
  const scroller = useRef<HTMLDivElement>(null);
  const [more, setMore] = useState(false);
  const [scrolls, setScrolls] = useState(false);
  useWheelStopsAtEnds(scroller, true);
  const measure = () => {
    const box = scroller.current;
    if (!box) return;
    setScrolls(box.scrollHeight > box.clientHeight + 1);
    setMore(box.scrollTop + box.clientHeight < box.scrollHeight - 1);
  };
  useEffect(() => {
    const box = scroller.current;
    if (!box) return;
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    return () => observer.disconnect();
  }, [notes]);
  return (
    <div className={styles.frame} data-more={more || undefined} data-scrolls={scrolls || undefined}>
      <div ref={scroller} className={styles.notes} role="region" tabIndex={scrolls ? 0 : undefined} aria-label="What's new" onScroll={measure}>
        {readNotes(notes).map((block, at) =>
          block.kind === "list" ? (
            <List key={at} items={block.items} />
          ) : block.kind === "heading" ? (
            <h3 key={at} className={styles.heading} data-level={block.level}>
              {inlines(block.inlines)}
            </h3>
          ) : (
            <p key={at} className={styles.paragraph}>
              {inlines(block.inlines)}
            </p>
          ),
        )}
      </div>
    </div>
  );
}

function List({ items }: { items: ListItem[] }) {
  return (
    <ul className={styles.list} role="list">
      {items.map((item, i) => (
        <li key={i}>
          {inlines(item.inlines)}
          {item.items.length > 0 && <List items={item.items} />}
        </li>
      ))}
    </ul>
  );
}

function inlines(pieces: Inline[]): ReactNode[] {
  return pieces.map((piece, at) =>
    piece.kind === "bold" ? (
      <strong key={at}>{piece.text}</strong>
    ) : piece.kind === "italic" ? (
      <em key={at}>{piece.text}</em>
    ) : piece.kind === "code" ? (
      <code key={at} className={styles.code}>
        {piece.text}
      </code>
    ) : (
      piece.text
    ),
  );
}
