import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { api, COMPUTER, errorText, type Entry, type IndexView, type Listed, type Listing, type Overview } from "../api";
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
import { entriesWith, sameShape, shapeOf } from "../entriesShape";
import type { IndexLink } from "../useIndexState";
import { withStatusChanges } from "../listingColumns";
import { useFolderChanges } from "../useFolderChanges";
import { usePageSelection } from "../usePageSelection";
import { usePanelLayout } from "../usePanelLayout";
import { useSortedEntries } from "../useSortedEntries";
import { useTauriEvent } from "../useTauriEvent";
import { useTypeChoice } from "../useTypeChoice";
import { useViewMode } from "../useViewMode";
import styles from "./IndexScreen.module.css";

const NO_ENTRIES: Entry[] = [];
const isUnfinished = (entry: Entry) => entry.index === "indexing" || entry.index === "paused" || entry.queued;
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
const INDEXED_LIST_HINT = (
  <>
    Right-click a file or folder and choose <b>Add to index</b> to start indexing it.
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
  index: IndexLink;
  secondsLeft: number | null;
  arrivalDelay: number;
  active: boolean;
}

export function IndexScreen({ onBack, onSimple, overview, index, secondsLeft, arrivalDelay, active }: Props) {
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
  const { indexed, excluded, loaded, ask: askLists, take: takeLists } = useIndexLists(active, progress !== null);
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
  const {
    root: page,
    box: marqueeBox,
    dragging,
    onPointerDown: onMarqueePointerDown,
  } = useMarquee({ regions: marqueeRegions, onBackground: clear });
  useEffect(() => {
    if (!active) return;
    const onPress = () => {
      if (!page.current?.closest("[inert]")) clear();
    };
    window.addEventListener(BACKGROUND_PRESS, onPress);
    return () => window.removeEventListener(BACKGROUND_PRESS, onPress);
  }, [active, clear, page]);

  const selectedEntries = useMemo(() => entriesWith(entries, selected), [entries, selected]);
  const explorer = useRef<HTMLElement>(null);
  useDefaultScroll(active, () => explorer.current?.querySelector<HTMLElement>("[data-scroller]") ?? null);

  const readAgainLater = useRef<() => void>(() => {});

  const { open, choose } = usePanelLayout(indexed.length, excluded.length, loaded);
  const expandIndexed = useCallback(() => choose("indexed"), [choose]);
  const expandExcluded = useCallback(() => choose("excluded"), [choose]);
  const [animate, setAnimate] = useState(false);
  useEffect(() => {
    if (!loaded) return;
    const frame = requestAnimationFrame(() => setAnimate(true));
    return () => cancelAnimationFrame(frame);
  }, [loaded]);

  const typeChoice = useTypeChoice({
    active,
    nothingIndexed: overview?.empty ?? true,
    onChanged: index.refresh,
    onError: setError,
  });
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
      let base = shown;
      for (let attempt = 0; ; attempt++) {
        const { listing: next, whole } = await api.refreshFolder(base);
        if (request !== latestRequest.current) return;
        const now = shownListing.current;
        if (whole || now === base) return setListing(next);
        if (!now || now.path !== base.path) return;
        if (attempt > 0) {
          const read = await api.listDir(base.path);
          if (request === latestRequest.current) setListing(read);
          return;
        }
        base = now;
      }
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
  readAgainLater.current = () => void refreshFolder();
  useFolderChanges(listing?.path ?? null, active, refreshFolder);

  const indexing = progress !== null;
  const unfinished = useMemo(() => !indexing && entries.some(isUnfinished), [entries, indexing]);
  const watcher = useLatest({ listing, askLists, takeLists, wanted: indexing || unfinished });
  useEffect(() => {
    if (!active) return;
    const unwatch = index.watch({
      listing: (now) => (now || watcher.current.wanted ? (watcher.current.listing?.generation ?? null) : null),
      lists: (now) => watcher.current.askLists(now),
      take: (view: IndexView) => {
        if (view.lists) watcher.current.takeLists(view.lists);
        const told = view.folder;
        if (!told) return;
        if (told.stale) return readAgainLater.current();
        setListing((current) => {
          if (!current || current.generation !== told.base) return current;
          const changed = withStatusChanges(current.entries, told.changes);
          if (!changed) return current.generation === told.generation ? current : { ...current, generation: told.generation };
          return { ...current, entries: sameShape(changed, current.entries), generation: told.generation };
        });
      },
    });
    void index.refresh();
    return unwatch;
  }, [active, index, watcher]);

  const listedShape = listing ? shapeOf(listing.entries) : null;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const hasPrivacyLocked = useMemo(() => listing?.entries.some((entry) => entry.privacy) ?? false, [listedShape]);
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

  const { actionsFor, addEntry, stopEntry, undoRecentAdd, undoRecentStop, listActions, askAboutIndexAll, confirmation, confirm, chooseAlternative, dismiss } = useIndexActions({
    folder: listing?.path ?? null,
    entries,
    refresh: index.refresh,
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
        onRefreshProgress={index.refresh}
        onIndexAll={askAboutIndexAll}
        everythingIndexed={overview?.everythingIndexed ?? false}
        onError={setError}
        stoppedAt={stoppedAt}
        active={active}
        onSimpleView={onSimple}
        types={typeChoice.types}
        onToggleType={(type) => void typeChoice.toggle(type)}
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
            emptyHint={view === "list" ? INDEXED_LIST_HINT : INDEXED_HINT}
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
        alternative={confirmation?.alternative ? { label: confirmation.alternative.label, onChoose: chooseAlternative } : undefined}
        cancel={!confirmation?.noCancel}
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

      <ConfirmDialog
        open={typeChoice.question?.open ?? false}
        title={typeChoice.question?.title ?? ""}
        confirmLabel={typeChoice.question?.confirmLabel ?? ""}
        danger={typeChoice.question?.danger}
        onCancel={() => typeChoice.answer(false)}
        onConfirm={() => typeChoice.answer(true)}
      >
        {typeChoice.question?.paragraphs.map((paragraph, i) => <p key={i}>{paragraph}</p>)}
      </ConfirmDialog>
    </div>
  );
}
