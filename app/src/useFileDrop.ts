import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api } from "./api";
import { categoryOfPath } from "./examples";
import { isWebKit } from "./platform";
import { useLatest } from "./useLatest";

interface Options {
  enabled: boolean;
  isActive?: () => boolean;
  targets: () => (HTMLElement | null)[];
  onDrop: (paths: string[]) => void;
}

export interface Dragged {
  supported: string[];
  unsupported: string[];
  folders: string[];
}

export interface Dragging {
  files: Dragged;
  over: boolean;
}

export function useFileDrop({ enabled, isActive, targets, onDrop }: Options): Dragging | null {
  const [state, setState] = useState<Dragging | null>(null);
  const latest = useLatest({ isActive, targets, onDrop });

  useEffect(() => {
    if (!enabled) {
      setState(null);
      return;
    }
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    let armed: Dragging | null = null;
    let hot: Dragging | null = null;
    let current: Dragged | null = null;
    let last = { x: 0, y: 0 };

    const inside = (position: { x: number; y: number }) => {
      const scale = isWebKit ? 1 : window.devicePixelRatio;
      const x = position.x / scale;
      const y = position.y / scale;
      return latest.current.targets().some((target) => {
        const box = target?.getBoundingClientRect();
        return !!box && x >= box.left && x <= box.right && y >= box.top && y <= box.bottom;
      });
    };
    const hover = (position: { x: number; y: number }) => {
      last = position;
      const next = !armed || !hot || !(latest.current.isActive?.() ?? true) ? null : inside(position) ? hot : armed;
      setState((now) => (now === next ? now : next));
    };
    const end = () => {
      armed = null;
      hot = null;
      current = null;
      setState(null);
    };
    const take = (files: Dragged) => {
      const any = files.supported.length + files.unsupported.length + files.folders.length > 0;
      current = files;
      armed = any ? { files, over: false } : null;
      hot = any ? { files, over: true } : null;
    };

    getCurrentWebview()
      .onDragDropEvent((event) => {
        const drag = event.payload;
        switch (drag.type) {
          case "enter": {
            const files: Dragged = {
              supported: drag.paths.filter((path) => categoryOfPath(path) !== null),
              unsupported: drag.paths.filter((path) => categoryOfPath(path) === null),
              folders: [],
            };
            take(files);
            hover(drag.position);
            if (files.unsupported.length > 0) {
              api
                .foldersAmong(files.unsupported)
                .then((isFolder) => {
                  if (cancelled || current !== files) return;
                  take({
                    supported: files.supported,
                    unsupported: files.unsupported.filter((_, i) => !isFolder[i]),
                    folders: files.unsupported.filter((_, i) => isFolder[i]),
                  });
                  hover(last);
                })
                .catch(() => {});
            }
            break;
          }
          case "over":
            hover(drag.position);
            break;
          case "drop": {
            const files = drag.paths.filter((path) => categoryOfPath(path) !== null);
            const active = latest.current.isActive?.() ?? true;
            const onTarget = inside(drag.position);
            end();
            if (files.length > 0 && active && onTarget) latest.current.onDrop(files);
            break;
          }
          default:
            end();
        }
      })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      unlisten?.();
      setState(null);
    };
  }, [enabled, latest]);

  return state;
}
