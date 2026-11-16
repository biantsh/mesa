import { useState, useSyncExternalStore, type SyntheticEvent } from "react";
import { flushSync } from "react-dom";
import { previewUrl, type Entry, type PreviewBox } from "./api";

type Previewable = Pick<Entry, "path" | "modified" | "kind" | "readable" | "category">;

export function wantsPreview(entry: Previewable): boolean {
  return (
    entry.kind === "file" &&
    entry.readable &&
    entry.category !== null &&
    entry.category !== "audio"
  );
}

export function usePreview(entry: Previewable, box?: PreviewBox) {
  const scale = usePixelRatio();
  const shape = box ? `${box.width}x${box.height}:${box.fit}@${scale}` : "whole";
  const key = `${shape}:${entry.modified ?? ""}:${entry.path}`;
  const [state, setState] = useState<State>(() => outcomes.get(key) ?? "loading");
  const settle = (outcome: Outcome) => {
    if (outcomes.size >= REMEMBERED) outcomes.clear();
    outcomes.set(key, outcome);
    told.push(() => setState(outcome));
    if (!telling) telling = requestAnimationFrame(tell);
  };
  const wanted = wantsPreview(entry) && state !== "failed";
  return {
    wanted,
    src: previewUrl(entry.path, box, scale),
    loaded: wanted && state === "loaded",
    onLoad: (e?: SyntheticEvent<HTMLImageElement>) => {
      if (!box && e) rememberShape(entry, e.currentTarget);
      settle("loaded");
    },
    onError: () => settle("failed"),
  };
}

const told: (() => void)[] = [];
let telling = 0;

function tell() {
  telling = 0;
  const saying = told.splice(0);
  flushSync(() => {
    for (const say of saying) say();
  });
}

const shapes = new Map<string, number>();

function rememberShape(entry: Previewable, image: HTMLImageElement) {
  if (image.naturalWidth === 0 || image.naturalHeight === 0) return;
  if (shapes.size >= REMEMBERED) shapes.clear();
  shapes.set(`${entry.modified ?? ""}:${entry.path}`, image.naturalWidth / image.naturalHeight);
}

export function previewShape(entry: Pick<Entry, "path" | "modified">): number | undefined {
  return shapes.get(`${entry.modified ?? ""}:${entry.path}`);
}

type Outcome = "loaded" | "failed";
type State = "loading" | Outcome;
const outcomes = new Map<string, Outcome>();
const REMEMBERED = 50_000;

const ratio = (() => {
  const listeners = new Set<() => void>();
  let query: MediaQueryList | null = null;
  const changed = () => {
    watch();
    for (const listener of listeners) listener();
  };
  function watch() {
    query?.removeEventListener("change", changed);
    query = typeof matchMedia === "undefined" ? null : matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
    query?.addEventListener("change", changed);
  }
  return {
    subscribe(listener: () => void) {
      if (listeners.size === 0) watch();
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
        if (listeners.size === 0) {
          query?.removeEventListener("change", changed);
          query = null;
        }
      };
    },
    get: () => (typeof window === "undefined" ? 1 : window.devicePixelRatio || 1),
  };
})();

export function usePixelRatio(): number {
  return useSyncExternalStore(ratio.subscribe, ratio.get);
}
