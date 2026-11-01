import { useCallback, useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { api, COMPUTER, errorText, type Entry, type Listing } from "./api";
import { CONFIRMATIONS, type Confirmation } from "./confirmations";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ContextMenu, type MenuItem } from "./components/ContextMenu";
import { FileGrid } from "./components/FileGrid";
import { NavBar } from "./components/NavBar";
import { TopBar } from "./components/TopBar";
import { useLiveIndexStatus } from "./useLiveIndexStatus";
import styles from "./App.module.css";

interface MenuState {
  entry: Entry;
  x: number;
  y: number;
}

export default function App() {
  const [listing, setListing] = useState<Listing | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [confirming, setConfirming] = useState<{ which: Confirmation; subject: string } | null>(null);
  const latestRequest = useRef(0);
  const [stoppedAt, setStoppedAt] = useState(0);

  useLiveIndexStatus(listing, setListing);

  const load = useCallback(async (path: string) => {
    const request = ++latestRequest.current;
    setLoading(true);
    try {
      const next = await api.listDir(path);
      if (request !== latestRequest.current) return;
      setListing(next);
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

  useEffect(() => {
    const block = (e: MouseEvent) => e.preventDefault();
    window.addEventListener("contextmenu", block);
    return () => window.removeEventListener("contextmenu", block);
  }, []);

  const closeMenu = useCallback(() => setMenu(null), []);

  const changeIndex = useCallback(
    async (action: () => Promise<void>) => {
      try {
        await action();
        if (listing) await load(listing.path);
      } catch (e) {
        setError(`Couldn't update the index. ${errorText(e)}`);
      }
    },
    [listing, load],
  );

  const openEntry = (entry: Entry) => {
    if (entry.kind !== "file" && entry.readable) load(entry.path);
  };

  const menuGroups = (entry: Entry): MenuItem[][] => [
    [
      {
        label: "Add to index",
        disabled: entry.index !== "notIndexed",
        onSelect: () => changeIndex(() => api.addToIndex(entry.path)),
      },
      ...(entry.droppable
        ? [
            {
              label: "Drop from queue",
              onSelect: () => {
                setStoppedAt(Date.now());
                changeIndex(() => api.dropFromQueue(entry.path));
              },
            },
          ]
        : []),
    ],
    [
      {
        label: "Remove from index",
        danger: true,
        disabled: entry.index !== "indexed",
        onSelect: () => changeIndex(() => api.removeFromIndex(entry.path)),
      },
    ],
  ];

  const confirmation = confirming ? CONFIRMATIONS[confirming.which](confirming.subject) : null;

  return (
    <div className={styles.app}>
      <TopBar
        onIndexAll={() => setConfirming({ which: "indexAll", subject: "" })}
        onClearIndex={() => setConfirming({ which: "clearIndex", subject: "" })}
        onCancelIndexing={(subject) => setConfirming({ which: "cancelIndexing", subject })}
        onError={setError}
        stoppedAt={stoppedAt}
      />

      <main className={styles.explorer} aria-busy={loading}>
        <NavBar crumbs={listing?.breadcrumbs ?? []} parent={listing?.parent ?? null} onOpen={load} />

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

        <FileGrid
          key={listing?.path ?? ""}
          entries={listing?.entries ?? []}
          loading={loading}
          onOpen={openEntry}
          onContextMenu={(entry, x, y) => setMenu({ entry, x, y })}
        />
      </main>

      {menu && (
        <ContextMenu
          key={`${menu.entry.path}:${menu.x}:${menu.y}`}
          x={menu.x}
          y={menu.y}
          groups={menuGroups(menu.entry)}
          onClose={closeMenu}
        />
      )}

      <ConfirmDialog
        open={confirmation !== null}
        title={confirmation?.title ?? ""}
        confirmLabel={confirmation?.confirmLabel ?? ""}
        danger={confirmation?.danger}
        onCancel={() => setConfirming(null)}
        onConfirm={() => {
          setConfirming(null);
          if (confirmation?.stopsIndexing) setStoppedAt(Date.now());
          if (confirmation) changeIndex(confirmation.run);
        }}
      >
        {confirmation?.message.map((paragraph) => <p key={paragraph}>{paragraph}</p>)}
      </ConfirmDialog>
    </div>
  );
}
