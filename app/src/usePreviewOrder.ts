import { useCallback, useEffect, useRef } from "react";
import { api, previewPixels, type Entry, type FolderPreviews, type PreviewBox } from "./api";
import { shapeOf } from "./entriesShape";
import { usePageVisible } from "./usePageVisible";
import { usePixelRatio } from "./usePreview";

let said = 0;

export function usePreviewOrder(entries: readonly Entry[], box: PreviewBox): (start: number, end: number) => void {
  const scale = usePixelRatio();
  const visible = usePageVisible();
  const generation = useRef(0);
  const seen = useRef({ start: 0, end: 0 });
  const sent = useRef<FolderPreviews | null>(null);
  const seenNow = useRef(visible);
  seenNow.current = visible;

  const sendWindow = useCallback(() => {
    if (generation.current === 0) return;
    const { start, end } = seen.current;
    void api.showPreviewWindow(generation.current, start, end, seenNow.current).catch(() => {});
  }, []);

  const shape = shapeOf(entries);
  useEffect(() => {
    if (shape.rust === null) return;
    const { width, height } = previewPixels(box, scale);
    const last = sent.current;
    if (last && last.shape === shape.rust && last.order === shape.order && last.width === width && last.height === height) return;
    sent.current = { shape: shape.rust, order: shape.order, width, height };
    generation.current = ++said;
    void api.showFolderPreviews(generation.current, sent.current).catch(() => {});
    sendWindow();
  }, [shape, box, scale, sendWindow]);

  useEffect(() => sendWindow(), [visible, sendWindow]);

  useEffect(
    () => () => {
      sent.current = null;
      if (generation.current === said) void api.showFolderPreviews(++said, null).catch(() => {});
      generation.current = 0;
    },
    [],
  );

  return useCallback(
    (start: number, end: number) => {
      if (seen.current.start === start && seen.current.end === end) return;
      seen.current = { start, end };
      sendWindow();
    },
    [sendWindow],
  );
}
