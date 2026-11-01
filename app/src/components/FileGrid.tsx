import { memo, useRef, type Ref } from "react";
import { previewUrl, type Entry } from "../api";
import { cx } from "../cx";
import { usePreview } from "../usePreview";
import { useRevealOnMount } from "../useRevealOnMount";
import { EmptyFolder } from "./EmptyFolder";
import { EntryIcon } from "./EntryIcon";
import { entryProps } from "./entryProps";
import { StatusBadge } from "./StatusBadge";
import styles from "./FileGrid.module.css";

interface Props {
  entries: Entry[];
  loading: boolean;
  selected: ReadonlySet<string>;
  onSelect: (path: string, toggle: boolean) => void;
  onClear: () => void;
  onOpen: (entry: Entry) => void;
  onContextMenu: (entry: Entry, x: number, y: number) => void;
}

export const FileGrid = memo(function FileGrid({
  entries,
  loading,
  selected,
  onSelect,
  onClear,
  onOpen,
  onContextMenu,
}: Props) {
  const firstSelected = entries.find((entry) => selected.has(entry.path))?.path;
  const selectedTile = useRef<HTMLButtonElement>(null);
  useRevealOnMount(selectedTile);

  if (entries.length === 0 && !loading) return <EmptyFolder />;

  return (
    <div
      className={cx(styles.grid, loading && styles.loading)}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClear();
      }}
    >
      {entries.map((entry) => (
        <Tile
          key={entry.path}
          entry={entry}
          selected={selected.has(entry.path)}
          buttonRef={entry.path === firstSelected ? selectedTile : undefined}
          onSelect={onSelect}
          onOpen={onOpen}
          onContextMenu={onContextMenu}
        />
      ))}
    </div>
  );
});

interface TileProps {
  entry: Entry;
  selected: boolean;
  buttonRef?: Ref<HTMLButtonElement>;
  onSelect: (path: string, toggle: boolean) => void;
  onOpen: (entry: Entry) => void;
  onContextMenu: (entry: Entry, x: number, y: number) => void;
}

const Tile = memo(function Tile({ entry, selected, buttonRef, onSelect, onOpen, onContextMenu }: TileProps) {
  const usable = entry.readable;

  const { wanted: hasImage, loaded: showingImage, onLoad, onError } = usePreview(entry);

  return (
    <button
      ref={buttonRef}
      type="button"
      className={cx(styles.tile, selected && styles.selected, !usable && styles.unreadable)}
      {...entryProps(entry, { selected, onSelect, onOpen, onContextMenu })}
    >
      <span className={styles.preview}>
        <span className={styles.iconBox}>
          <EntryIcon entry={entry} size={60} />
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
            onLoad={onLoad}
            onError={onError}
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
});
