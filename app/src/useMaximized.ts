import { useEffect, useState } from "react";
import { currentWindow } from "./window";

export function useMaximized(): boolean {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const window = currentWindow();
    if (!window) return;
    let cancelled = false;
    let frame = 0;
    const read = () =>
      window
        .isMaximized()
        .then((now) => !cancelled && setMaximized(now))
        .catch(() => {});
    read();
    const unlisten = window.onResized(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(read);
    });
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
      unlisten.then((stop) => stop()).catch(() => {});
    };
  }, []);

  useEffect(() => {
    document.documentElement.toggleAttribute("data-maximized", maximized);
  }, [maximized]);

  return maximized;
}
