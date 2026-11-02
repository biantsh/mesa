import { useState } from "react";
import type { Entry } from "./api";

type Previewable = Pick<Entry, "kind" | "readable" | "category">;

function wantsPreview(entry: Previewable): boolean {
  return (
    entry.kind === "file" &&
    entry.readable &&
    entry.category !== null &&
    entry.category !== "audio"
  );
}

export function usePreview(entry: Previewable) {
  const [state, setState] = useState<"loading" | "loaded" | "failed">("loading");
  const wanted = wantsPreview(entry) && state !== "failed";
  return {
    wanted,
    loaded: wanted && state === "loaded",
    onLoad: () => setState("loaded"),
    onError: () => setState("failed"),
  };
}
