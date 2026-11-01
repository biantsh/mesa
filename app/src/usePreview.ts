import { useState } from "react";
import type { Entry } from "./api";

function wantsPreview(entry: Entry): boolean {
  return (
    entry.kind === "file" &&
    entry.readable &&
    entry.category !== null &&
    entry.category !== "audio"
  );
}

export function usePreview(entry: Entry) {
  const [state, setState] = useState<"loading" | "loaded" | "failed">("loading");
  const wanted = wantsPreview(entry) && state !== "failed";
  return {
    wanted,
    loaded: wanted && state === "loaded",
    onLoad: () => setState("loaded"),
    onError: () => setState("failed"),
  };
}
