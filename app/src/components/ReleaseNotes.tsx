import type { ReactNode } from "react";
import { readNotes, type Inline } from "../releaseNotes";
import styles from "./ReleaseNotes.module.css";

export function ReleaseNotes({ notes }: { notes: string }) {
  return (
    <div className={styles.notes} tabIndex={0} aria-label="What's new">
      {readNotes(notes).map((block, at) =>
        block.kind === "list" ? (
          <ul key={at} className={styles.list}>
            {block.items.map((item, i) => (
              <li key={i}>{inlines(item)}</li>
            ))}
          </ul>
        ) : block.kind === "heading" ? (
          <h3 key={at} className={styles.heading}>
            {inlines(block.inlines)}
          </h3>
        ) : (
          <p key={at} className={styles.paragraph}>
            {inlines(block.inlines)}
          </p>
        ),
      )}
    </div>
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
