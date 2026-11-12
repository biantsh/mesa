import { useCallback, useRef, useState } from "react";
import { api, sameProgress, type IndexView, type Overview } from "./api";
import { usePoll } from "./usePoll";

export interface IndexWatcher {
  listing: (now: boolean) => number | null;
  lists: (now: boolean) => { version: number; states: boolean } | null;
  take: (view: IndexView) => void;
}

export interface IndexLink {
  refresh: () => Promise<void>;
  watch: (watcher: IndexWatcher) => () => void;
}

export function useIndexState(): { overview: Overview | null } & IndexLink {
  const [overview, setOverview] = useState<Overview | null>(null);
  const watcher = useRef<IndexWatcher | null>(null);
  const last = useRef<Promise<void>>(Promise.resolve());

  const read = useCallback(async (now: boolean) => {
    const page = watcher.current;
    const lists = page?.lists(now) ?? null;
    const view = await api.indexView(page?.listing(now) ?? null, lists?.version ?? null, lists?.states ?? false);
    setOverview((current) => (current && sameOverview(current, view.overview) ? current : view.overview));
    if (page && watcher.current === page) page.take(view);
  }, []);

  const ask = useCallback(
    (now: boolean) => {
      const next = last.current.then(() => read(now));
      last.current = next.catch(() => {});
      return last.current;
    },
    [read],
  );

  const refresh = useCallback(() => ask(true), [ask]);
  const watch = useCallback((page: IndexWatcher) => {
    watcher.current = page;
    return () => {
      if (watcher.current === page) watcher.current = null;
    };
  }, []);
  usePoll(() => ask(false), { immediately: true });
  return { overview, refresh, watch };
}

function sameOverview(a: Overview, b: Overview): boolean {
  return (
    a.empty === b.empty &&
    a.everythingIndexed === b.everythingIndexed &&
    a.files === b.files &&
    a.bytes === b.bytes &&
    sameProgress(a.progress, b.progress)
  );
}
