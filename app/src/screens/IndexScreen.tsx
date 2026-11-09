import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { api, COMPUTER, errorText, type Entry, type Listed, type Listing, type Overview } from "../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { ContextMenu } from "../components/ContextMenu";
import { EmptyFolder } from "../components/EmptyFolder";
import { FileGrid } from "../components/FileGrid";
import { FileList } from "../components/FileList";
import { ItemsPanel } from "../components/ItemsPanel";
import { NavBar } from "../components/NavBar";
import { PanelSplitter } from "../components/PanelSplitter";
import { StatusBar } from "../components/StatusBar";
import { TopBar } from "../components/TopBar";
import { BACKGROUND_PRESS } from "../background";
import { showsEmptyFolder } from "../emptyFolder";
import { MAIN_HIDDEN } from "../events";
import { menuGroups } from "../menuGroups";
import { parentOf } from "../paths";
import { DEFAULT_SORT, nextSort, type Sort, type SortColumn } from "../sort";
import { useDefaultScroll } from "../useDefaultScroll";
import { useFirstArrival } from "../useFirstArrival";
import { useMarquee } from "../useMarquee";
import { useIndexActions } from "../useIndexActions";
import { useIndexKeys } from "../useIndexKeys";
import { useIndexLists } from "../useIndexLists";
import { useLatest } from "../useLatest";
import { useListMenu } from "../useListMenu";
import { useLiveIndexStatus } from "../useLiveIndexStatus";
import { mergeListing, useFolderChanges } from "../useFolderChanges";
import { usePageSelection } from "../usePageSelection";
import { usePanelLayout } from "../usePanelLayout";
import { useSortedEntries } from "../useSortedEntries";
import { useTauriEvent } from "../useTauriEvent";
import { useViewMode } from "../useViewMode";
import styles from "./IndexScreen.module.css";

const NO_ENTRIES: Entry[] = [];
const KEEPS_SELECTION = "button, a, input, select, textarea, label, dialog, [role='menu'], [role='separator']";

const INDEXED_HINT = (
  <>
    Hover a file or folder and click{" "}
    <span className={styles.hintPlus} role="img" aria-label="plus">
      <svg viewBox="0 0 12 12" aria-hidden>
        <circle cx="6" cy="6" r="6" className={styles.hintDisc} />
        <path d="M3.2 6h5.6M6 3.2v5.6" fill="none" stroke="currentColor" strokeWidth={1.7} strokeLinecap="round" />
      </svg>
    </span>{" "}
    to start indexing it.
  </>
);
const EXCLUDED_HINT = (
  <>
    Right-click a file or folder and choose <b>Exclude</b> to keep it out of the index.
  </>
);

interface MenuState {
  entries: Entry[];
  x: number;
  y: number;
}

interface Props {
  onBack: () => void;
  onSimple: () => void;
  overview: Overview | null;
  onRefreshProgress: () => Promise<void>;
  secondsLeft: number | null;
  arrivalDelay: number;
  active: boolean;
}

export function IndexScreen({ onBack, onSimple, overview, onRefreshProgress: refreshProgress, secondsLeft, arrivalDelay, active }: Props) {
  const [listing, setListing] = useState<Listing | null>(null);
  const [loading, setLoading] = useState(true);
  const [openingAnother, setOpeningAnother] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const latestRequest = useRef(0);
  const shownFolder = useRef<string | null>(null);
  const [stoppedAt, setStoppedAt] = useState(0);
  const markStopped = useCallback(() => setStoppedAt(Date.now()), []);
  const [view, setView] = useViewMode();
  const [sort, setSort] = useState<Sort>(DEFAULT_SORT);
  const changeSort = useCallback((column: SortColumn) => setSort((current) => nextSort(current, column)), []);
  const progress = overview?.progress ?? null;

  const { indexed, excluded, loaded, refresh: refreshLists } = useIndexLists(
    `${progress?.name ?? ""}|${progress?.queued ?? 0}|${progress?.paused ? 1 : 0}`,
    active,
  );
  const listItems = { indexed, excluded };

  const entries = listing?.entries ?? NO_ENTRIES;
  const sortedEntries = useSortedEntries(entries, sort);
  const displayed = view === "list" ? sortedEntries : entries;

  const {
    selected,
    active: activeEntry,
    select,
    clear,
    move,
    listSelection,
    clearListSelection,
    selectInList,
    listHandlers,
    selectedIn,
    activeIn,
    marqueeRegions,
  } = usePageSelection({ folder: listing?.path ?? "", displayed, enabled: active, indexed, excluded });
  useEffect(() => {
    if (!active) return;
    window.addEventListener(BACKGROUND_PRESS, clear);
    return () => window.removeEventListener(BACKGROUND_PRESS, clear);
  }, [active, clear]);
  const {
    root: page,
    box: marqueeBox,
    dragging,
    onPointerDown: onMarqueePointerDown,
  } = useMarquee({ regions: marqueeRegions, onBackground: clear });

  const selectedEntries = useMemo(() => entries.filter((entry) => selected.has(entry.path)), [entries, selected]);
  const explorer = useRef<HTMLElement>(null);
  useDefaultScroll(active, () => explorer.current?.querySelector<HTMLElement>("[data-scroller]") ?? null);

  const refreshStatuses = useLiveIndexStatus(listing, setListing, refreshProgress, active && progress !== null);
  const refreshAll = async () => {
    await refreshStatuses();
    await refreshProgress();
  };

  const { open, choose } = usePanelLayout(indexed.length, excluded.length, loaded);
  const expandIndexed = useCallback(() => choose("indexed"), [choose]);
  const expandExcluded = useCallback(() => choose("excluded"), [choose]);
  const [animate, setAnimate] = useState(false);
  useEffect(() => {
    if (!loaded) return;
    const frame = requestAnimationFrame(() => setAnimate(true));
    return () => cancelAnimationFrame(frame);
  }, [loaded]);
  const arrival = useFirstArrival(active, loaded, arrivalDelay);
  useEffect(() => {
    if (listSelection.list !== null && open !== listSelection.list) clearListSelection();
  }, [open, listSelection.list, clearListSelection]);

  const loadsUnderway = useRef(0);
  const load = useCallback(async (path: string) => {
    const request = ++latestRequest.current;
    setOpeningAnother(path !== shownFolder.current);
    setLoading(true);
    loadsUnderway.current++;
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
      loadsUnderway.current--;
      if (request === latestRequest.current) setLoading(false);
    }
  }, []);

  const shownListing = useLatest(listing);
  const refreshFolder = useCallback(async () => {
    const shown = shownListing.current;
    if (!shown || loadsUnderway.current > 0) return;
    const request = ++latestRequest.current;
    try {
      const next = await api.listDir(shown.path);
      if (request !== latestRequest.current) return;
      setListing((current) => (current && current.path === next.path ? mergeListing(current, next) : next));
    } catch {
      if (request !== latestRequest.current) return;
      const above = shown.breadcrumbs.map((crumb) => crumb.path).filter((path) => path !== shown.path).reverse();
      for (const path of above) {
        const next = await api.listDir(path).catch(() => null);
        if (request !== latestRequest.current) return;
        if (next) {
          setListing(next);
          shownFolder.current = next.path;
          return;
        }
      }
    }
  }, [shownListing]);
  useFolderChanges(listing?.path ?? null, active, refreshFolder);

  const hasPrivacyLocked = listing?.entries.some((entry) => entry.privacy) ?? false;
  useEffect(() => {
    if (!active || !hasPrivacyLocked) return;
    const onFocus = () => void refreshFolder();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [active, hasPrivacyLocked, refreshFolder]);

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

  const { actionsFor, addEntry, stopEntry, undoRecentAdd, undoRecentStop, listActions, askAboutIndexAll, confirmation, confirm, dismiss } = useIndexActions({
    folder: listing?.path ?? null,
    entries,
    shownFolder,
    load,
    refreshProgress,
    refreshLists,
    clearSelection: clear,
    clearListSelection,
    setError,
    markStopped,
  });

  const openEntry = useCallback(
    (entry: Entry) => {
      if (!entry.readable) {
        if (entry.privacy) {
          api.openPrivacySettings(entry.privacy).catch((e) => setError(`Couldn't open System Settings. ${errorText(e)}`));
        }
        return;
      }
      if (entry.kind === "file") {
        api.openFile(entry.path).catch((e) => setError(`Couldn't open ${entry.name}. ${errorText(e)}`));
      } else {
        void undoRecentAdd(entry);
        void undoRecentStop(entry);
        clear();
        load(entry.path);
      }
    },
    [clear, load, undoRecentAdd, undoRecentStop],
  );

  const showInFolder = useCallback(
    (entry: Entry) => api.showInFolder(entry.path).catch((e) => setError(`Couldn't show ${entry.name}. ${errorText(e)}`)),
    [],
  );

  const openListed = useCallback(
    (item: Listed) => {
      if (!item.missing) return openEntry(item);
      clear();
      load(parentOf(item.path));
    },
    [clear, load, openEntry],
  );

  const listMenu = useListMenu({
    indexed,
    excluded,
    listSelection,
    selectInList,
    openListed,
    showInFolder,
    listActions,
    onOpening: closeMenu,
  });

  const selectionNow = useLatest({ selected, selectedEntries });
  const closeListMenu = listMenu.close;
  useTauriEvent(MAIN_HIDDEN, () => {
    clear();
    closeMenu();
    closeListMenu();
    dismiss();
  });
  const openMenu = useCallback(
    (entry: Entry, x: number, y: number) => {
      const { selected, selectedEntries } = selectionNow.current;
      const inSelection = selected.has(entry.path);
      if (!inSelection) select(entry.path, false);
      closeListMenu();
      setMenu({ entries: inSelection ? selectedEntries : [entry], x, y });
    },
    [select, closeListMenu, selectionNow],
  );

  const clearOnBackground = (e: React.MouseEvent) => {
    if (e.target instanceof Element && e.target.closest(KEEPS_SELECTION) === null) clear();
  };

  const selectionActions = actionsFor(selectedEntries);
  useIndexKeys({
    enabled: active,
    somethingSelected: selected.size > 0 || listSelection.list !== null,
    dragging,
    clear,
    actions: selectionActions,
  });

  return (
    <div ref={page} className={styles.app} onClick={clearOnBackground} onPointerDown={onMarqueePointerDown}>
      <div ref={marqueeBox} className={styles.marquee} aria-hidden />
      <TopBar
        onBack={onBack}
        progress={progress}
        secondsLeft={secondsLeft}
        onRefreshProgress={refreshAll}
        onIndexAll={askAboutIndexAll}
        everythingIndexed={overview?.everythingIndexed ?? false}
        onError={setError}
        stoppedAt={stoppedAt}
        active={active}
        onSimpleView={onSimple}
        selection={{
          selected: selectedEntries.length,
          add: selectionActions.add,
          onAdd: selectionActions.doAdd,
        }}
      />

      <div className={styles.body}>
        <main ref={explorer} className={styles.explorer} aria-busy={loading}>
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

          {showsEmptyFolder(entries.length, loading, openingAnother) ? (
            <EmptyFolder />
          ) : view === "grid" ? (
            <FileGrid
              key={listing?.path ?? ""}
              entries={entries}
              loading={loading}
              selected={selected}
              active={activeEntry}
              onSelect={select}
              onMove={move}
              onOpen={openEntry}
              onAdd={addEntry}
              onStop={stopEntry}
              onContextMenu={openMenu}
            />
          ) : (
            <FileList
              key={listing?.path ?? ""}
              entries={sortedEntries}
              loading={loading}
              selected={selected}
              active={activeEntry}
              onSelect={select}
              onMove={move}
              sort={sort}
              onSort={changeSort}
              onOpen={openEntry}
              onContextMenu={openMenu}
            />
          )}

          <StatusBar
            entries={listing?.entries ?? null}
            selected={selectedEntries}
            selectedInList={
              listSelection.list === null
                ? null
                : {
                    name: listSelection.list === "indexed" ? "Indexed" : "Excluded",
                    count: listItems[listSelection.list].filter((item) => listSelection.paths.has(item.path)).length,
                  }
            }
          />
        </main>

        <aside
          className={styles.side}
          aria-label="What is indexed and what is excluded"
          style={loaded ? undefined : { visibility: "hidden" }}
        >
          <PanelSplitter />
          <ItemsPanel
            title="Indexed"
            items={indexed}
            selected={selectedIn("indexed")}
            active={activeIn("indexed")}
            {...listHandlers.indexed}
            onOpen={openListed}
            onContextMenu={listMenu.openIndexed}
            onRemove={listActions.unindex}
            verb="Remove"
            danger
            removeLabel="Remove from index"
            empty="Nothing is indexed yet."
            emptyHint={INDEXED_HINT}
            collapsed={open !== "indexed"}
            onExpand={open === "indexed" ? undefined : expandIndexed}
            animate={animate}
            arrival={arrival}
            region="indexed"
          />
          <ItemsPanel
            title="Excluded"
            items={excluded}
            selected={selectedIn("excluded")}
            active={activeIn("excluded")}
            {...listHandlers.excluded}
            onOpen={openListed}
            onContextMenu={listMenu.openExcluded}
            onRemove={listActions.stopExcluding}
            verb="Stop excluding"
            removeLabel="Stop excluding"
            empty="Nothing is excluded."
            emptyHint={EXCLUDED_HINT}
            collapsed={open !== "excluded"}
            onExpand={open === "excluded" ? undefined : expandExcluded}
            animate={animate}
            arrival={arrival}
            region="excluded"
          />
        </aside>
      </div>


      {menu && (
        <ContextMenu
          key={`${menu.entries[0].path}:${menu.x}:${menu.y}`}
          x={menu.x}
          y={menu.y}
          groups={menuGroups(menu.entries, actionsFor(menu.entries), openEntry, showInFolder)}
          onClose={closeMenu}
        />
      )}

      {listMenu.menu && (
        <ContextMenu
          key={`${listMenu.menu.list}:${listMenu.menu.items[0].path}:${listMenu.menu.x}:${listMenu.menu.y}`}
          x={listMenu.menu.x}
          y={listMenu.menu.y}
          groups={listMenu.groups(listMenu.menu)}
          onClose={listMenu.close}
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
        {confirmation?.message.map((paragraph, i) =>
          typeof paragraph === "string" ? (
            <p key={i}>{paragraph}</p>
          ) : (
            <p key={i}>
              <strong>Note:</strong> {paragraph.note}
            </p>
          ),
        )}
      </ConfirmDialog>
    </div>
  );
}
