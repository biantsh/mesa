import { useState } from "react";
import { File, FileMusic, HardDrive } from "lucide-react";
import { previewUrl, type Entry } from "../api";
import { cx } from "../cx";
import { FolderIcon } from "./FolderIcon";
import { StatusBadge, statusText } from "./StatusBadge";
import styles from "./FileGrid.module.css";

interface Props {
  entries: Entry[];
  loading: boolean;
  onOpen: (entry: Entry) => void;
  onContextMenu: (entry: Entry, x: number, y: number) => void;
}

export function FileGrid({ entries, loading, onOpen, onContextMenu }: Props) {
  const [selected, setSelected] = useState<string | null>(null);

  if (entries.length === 0 && !loading) {
    return (
      <div className={styles.empty}>
        <p className={styles.emptyTitle}>Nothing to show here</p>
        <p>Only folders and supported files (images, video, audio and documents) are listed.</p>
      </div>
    );
  }

  return (
    <div
      className={cx(styles.grid, loading && styles.loading)}
      onClick={(e) => {
        if (e.target === e.currentTarget) setSelected(null);
      }}
    >
      {entries.map((entry) => (
        <Tile
          key={entry.path}
          entry={entry}
          selected={entry.path === selected}
          onSelect={setSelected}
          onOpen={onOpen}
          onContextMenu={(entry, x, y) => {
            setSelected(entry.path);
            onContextMenu(entry, x, y);
          }}
        />
      ))}
    </div>
  );
}

function wantsPreview(entry: Entry): boolean {
  return (
    entry.kind === "file" &&
    entry.readable &&
    entry.category !== null &&
    entry.category !== "audio"
  );
}

interface TileProps {
  entry: Entry;
  selected: boolean;
  onSelect: (path: string) => void;
  onOpen: (entry: Entry) => void;
  onContextMenu: (entry: Entry, x: number, y: number) => void;
}

function Tile({ entry, selected, onSelect, onOpen, onContextMenu }: TileProps) {
  const usable = entry.readable;
  const status = statusText(entry);
  const [preview, setPreview] = useState<"loading" | "loaded" | "failed">("loading");
  const hasImage = wantsPreview(entry) && preview !== "failed";
  const showingImage = hasImage && preview === "loaded";

  return (
    <button
      type="button"
      className={cx(styles.tile, selected && styles.selected, !usable && styles.unreadable)}
      title={status ? `${entry.name} — ${status}` : entry.name}
      aria-pressed={usable ? selected : undefined}
      aria-disabled={usable ? undefined : true}
      onClick={() => usable && onSelect(entry.path)}
      onDoubleClick={() => usable && onOpen(entry)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && usable) onOpen(entry);
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        if (!usable) return;
        const rect = e.currentTarget.getBoundingClientRect();
        const fromKeyboard = e.clientX === 0 && e.clientY === 0;
        onContextMenu(
          entry,
          fromKeyboard ? rect.left + rect.width / 2 : e.clientX,
          fromKeyboard ? rect.top + rect.height / 2 : e.clientY,
        );
      }}
    >
      <span className={styles.preview}>
        <span className={styles.iconBox}>
          <EntryIcon entry={entry} />
          {!showingImage && (
            <span className={styles.badgeOnIcon}>
              <StatusBadge entry={entry} />
            </span>
          )}
        </span>
        {hasImage && (
          <img
            className={cx(
              styles.thumb,
              showingImage && styles.loaded,
              entry.category === "document" && styles.contain,
            )}
            src={previewUrl(entry.path)}
            alt=""
            draggable={false}
            loading="lazy"
            decoding="async"
            onLoad={() => setPreview("loaded")}
            onError={() => setPreview("failed")}
          />
        )}
        {showingImage && (
          <span className={styles.badgeOnPreview}>
            <StatusBadge entry={entry} />
          </span>
        )}
      </span>
      <span className={styles.name}>{entry.name}</span>
    </button>
  );
}

function EntryIcon({ entry }: { entry: Entry }) {
  const props = { size: 60, strokeWidth: 1.4, "aria-hidden": true } as const;
  if (entry.kind === "drive") return <HardDrive {...props} className={styles.drive} />;
  if (entry.kind === "folder") {
    return <FolderIcon full={entry.hasContents} className={styles.folder} />;
  }
  if (entry.category === "audio") return <FileMusic {...props} className={styles.file} />;
  return <File {...props} className={styles.file} />;
}
