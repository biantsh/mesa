import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { api, COMPUTER, errorText, type Entry, type Listing } from "../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { ContextMenu } from "../components/ContextMenu";
import { FileGrid } from "../components/FileGrid";
import { FileList } from "../components/FileList";
import { NavBar } from "../components/NavBar";
import { StatusBar } from "../components/StatusBar";
import { TopBar } from "../components/TopBar";
import { menuGroups } from "../menuGroups";
import { DEFAULT_SORT, nextSort, type Sort, type SortColumn } from "../sort";
import { useIndexActions } from "../useIndexActions";
import { useIndexProgress } from "../useIndexProgress";
import { useLiveIndexStatus } from "../useLiveIndexStatus";
import { useSelection } from "../useSelection";
import { useViewMode } from "../useViewMode";
import styles from "./IndexScreen.module.css";

const NO_ENTRIES: Entry[] = [];

interface MenuState {
  entries: Entry[];
  x: number;
  y: number;
}

interface Props {
  onBack: () => void;
  active: boolean;
}

export function IndexScreen({ onBack, active }: Props) {
  const [listing, setListing] = useState<Listing | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const latestRequest = useRef(0);
  const shownFolder = useRef<string | null>(null);
  const [stoppedAt, setStoppedAt] = useState(0);
  const markStopped = useCallback(() => setStoppedAt(Date.now()), []);
  const [view, setView] = useViewMode();
  const [sort, setSort] = useState<Sort>(DEFAULT_SORT);
  const changeSort = useCallback((column: SortColumn) => setSort((current) => nextSort(current, column)), []);
  const entries = listing?.entries ?? NO_ENTRIES;
  const { selected, select, clear } = useSelection(listing?.path ?? "", entries, active);
  const selectedEntries = useMemo(() => entries.filter((entry) => selected.has(entry.path)), [entries, selected]);

  const { progress, refresh: refreshProgress } = useIndexProgress();
  useLiveIndexStatus(listing, setListing, refreshProgress);

  const load = useCallback(async (path: string) => {
    const request = ++latestRequest.current;
    setLoading(true);
    try {
      const next = await api.listDir(path);
      if (request !== latestRequest.current) return;
      setListing(next);
      shownFolder.current = next.path;
      setError(null);
    } catch (e) {
      if (request !== latestRequest.current) return;
      setError(`Couldn't open this folder. ${errorText(e)}`);
    } finally {
      if (request === latestRequest.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    api
      .homeDir()
      .then((home) => {
        if (!cancelled) load(home || COMPUTER);
      })
      .catch((e) => {
        if (!cancelled) setError(errorText(e));
      });
    return () => {
      cancelled = true;
    };
  }, [load]);

  const closeMenu = useCallback(() => setMenu(null), []);

  const { actionsFor, askAboutExclusions, askToCancel, confirmation, confirm, dismiss } = useIndexActions({
    folder: listing?.path ?? null,
    entries,
    shownFolder,
    load,
    refreshProgress,
    clearSelection: clear,
    setError,
    markStopped,
  });

  const openEntry = useCallback(
    (entry: Entry) => {
      if (!entry.readable) return;
      if (entry.kind === "file") {
        api.openFile(entry.path).catch((e) => setError(`Couldn't open ${entry.name}. ${errorText(e)}`));
      } else {
        clear();
        load(entry.path);
      }
    },
    [clear, load],
  );

  const showInFolder = (entry: Entry) =>
    api.showInFolder(entry.path).catch((e) => setError(`Couldn't show ${entry.name}. ${errorText(e)}`));

  const selectionNow = useRef({ selected, selectedEntries });
  useLayoutEffect(() => {
    selectionNow.current = { selected, selectedEntries };
  });
  const openMenu = useCallback(
    (entry: Entry, x: number, y: number) => {
      const { selected, selectedEntries } = selectionNow.current;
      const inSelection = selected.has(entry.path);
      if (!inSelection) select(entry.path, false);
      setMenu({ entries: inSelection ? selectedEntries : [entry], x, y });
    },
    [select],
  );

  const selectionActions = actionsFor(selectedEntries);

  return (
    <div className={styles.app}>
      <TopBar
        onBack={onBack}
        progress={progress}
        onRefreshProgress={refreshProgress}
        onIndexAll={() => askAboutExclusions("indexAll")}
        onClearIndex={() => askAboutExclusions("clearIndex")}
        onCancelIndexing={askToCancel}
        onError={setError}
        stoppedAt={stoppedAt}
        selection={{
          selected: selectedEntries.length,
          add: selectionActions.add,
          remove: selectionActions.remove,
          onAdd: selectionActions.doAdd,
          onRemove: selectionActions.doRemove,
        }}
      />

      <main className={styles.explorer} aria-busy={loading}>
        <NavBar
          crumbs={listing?.breadcrumbs ?? []}
          parent={listing?.parent ?? null}
          onOpen={load}
          view={view}
          onViewChange={setView}
        />

        {error && (
          <div className={styles.error} role="alert">
            <span>{error}</span>
            <button
              type="button"
              className={styles.dismiss}
              onClick={() => setError(null)}
              aria-label="Dismiss"
            >
              <X size={16} aria-hidden />
            </button>
          </div>
        )}

        {view === "grid" ? (
          <FileGrid
            key={listing?.path ?? ""}
            entries={entries}
            loading={loading}
            selected={selected}
            onSelect={select}
            onClear={clear}
            onOpen={openEntry}
            onContextMenu={openMenu}
          />
        ) : (
          <FileList
            key={listing?.path ?? ""}
            entries={entries}
            loading={loading}
            selected={selected}
            onSelect={select}
            onClear={clear}
            sort={sort}
            onSort={changeSort}
            onOpen={openEntry}
            onContextMenu={openMenu}
          />
        )}
      </main>

      <StatusBar entries={listing?.entries ?? null} selected={selectedEntries} />

      {menu && (
        <ContextMenu
          key={`${menu.entries[0].path}:${menu.x}:${menu.y}`}
          x={menu.x}
          y={menu.y}
          groups={menuGroups(menu.entries, actionsFor(menu.entries), openEntry, showInFolder)}
          onClose={closeMenu}
        />
      )}

      <ConfirmDialog
        open={confirmation !== null}
        title={confirmation?.title ?? ""}
        confirmLabel={confirmation?.confirmLabel ?? ""}
        danger={confirmation?.danger}
        onCancel={dismiss}
        onConfirm={confirm}
      >
        {confirmation?.message.map((paragraph) =>
          typeof paragraph === "string" ? (
            <p key={paragraph}>{paragraph}</p>
          ) : (
            <p key={paragraph.note}>
              <strong>Note:</strong> {paragraph.note}
            </p>
          ),
        )}
      </ConfirmDialog>
    </div>
  );
}
