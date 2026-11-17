import type { Ref } from "react";
import type { CodePart, CodeView as Code } from "../api";
import styles from "./CodeView.module.css";

interface Props {
  code: Code | null | undefined;
  onExpand: () => void;
  blockRef?: Ref<HTMLElement>;
}

export function CodeLine({ parts }: { parts: CodePart[] }) {
  return (
    <>
      {parts.map((part, i) => {
        const className = part.kind ? styles[part.kind] : undefined;
        return part.hit ? (
          <mark key={i} className={className}>
            {part.text}
          </mark>
        ) : (
          <span key={i} className={className}>
            {part.text}
          </span>
        );
      })}
    </>
  );
}

export function CodeView({ code, onExpand, blockRef }: Props) {
  if (code === undefined) return <div className={styles.block} data-loading="" aria-busy />;
  if (code === null) return null;
  const shown = code.lines.length;
  const cut = code.start + shown - 1 < code.end;
  return (
    <figure
      ref={blockRef}
      className={styles.block}
      role="button"
      tabIndex={0}
      aria-label="Show the whole file"
      onClick={onExpand}
      onKeyDown={(e) => {
        if (e.key !== "Enter" && e.key !== " ") return;
        e.preventDefault();
        onExpand();
      }}
    >
      <figcaption className={styles.head}>
        <span className={styles.symbol}>{code.symbol || "Outside any function"}</span>
        <span className={styles.where}>
          {code.language}
          {code.project && ` · ${code.project}`}
          {` · ${code.start === code.end ? `line ${code.start}` : `lines ${code.start}–${code.end}`}`}
        </span>
      </figcaption>
      <div className={styles.scroll}>
        <table className={styles.code}>
          <tbody>
            {code.lines.map((parts, i) => (
              <tr key={code.start + i}>
                <td className={styles.lineNumber}>{code.start + i}</td>
                <td className={styles.text}>
                  <CodeLine parts={parts} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {cut && <p className={styles.more}>{code.end - (code.start + shown - 1)} more lines</p>}
      </div>
    </figure>
  );
}
