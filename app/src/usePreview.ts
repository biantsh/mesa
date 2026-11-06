import { useState } from "react";
import type { Entry } from "./api";

type Previewable = Pick<Entry, "path" | "modified" | "kind" | "readable" | "category">;

function wantsPreview(entry: Previewable): boolean {
  return (
    entry.kind === "file" &&
    entry.readable &&
    entry.category !== null &&
    entry.category !== "audio"
  );
}

export function usePreview(entry: Previewable) {
  const key = `${entry.modified ?? ""}:${entry.path}`;
  const [state, setState] = useState<State>(() => outcomes.get(key) ?? "loading");
  const settle = (outcome: Outcome) => {
    if (outcomes.size >= REMEMBERED) outcomes.clear();
    outcomes.set(key, outcome);
    setState(outcome);
  };
  const wanted = wantsPreview(entry) && state !== "failed";
  return {
    wanted,
    loaded: wanted && state === "loaded",
    onLoad: () => settle("loaded"),
    onError: () => settle("failed"),
  };
}

type Outcome = "loaded" | "failed";
type State = "loading" | Outcome;
const outcomes = new Map<string, Outcome>();
const REMEMBERED = 50_000;
