import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

export function useTauriEvent<T = void>(name: string, handler: (payload: T) => void) {
  const latest = useRef(handler);
  useEffect(() => {
    latest.current = handler;
  });

  useEffect(() => {
    const listening = listen<T>(name, (event) => latest.current(event.payload));
    return () => void listening.then((stop) => stop()).catch(() => {});
  }, [name]);
}
