import { useEffect, useRef } from "react";

interface Options {
  active?: boolean;
  ms?: number;
  immediately?: boolean;
}

export function usePoll(
  task: () => Promise<void> | void,
  { active = true, ms = 500, immediately = false }: Options = {},
) {
  const latest = useRef(task);
  useEffect(() => {
    latest.current = task;
  });

  useEffect(() => {
    if (!active) return;
    let running = false;
    const run = async () => {
      if (running || document.hidden) return;
      running = true;
      try {
        await latest.current();
      } catch {
      } finally {
        running = false;
      }
    };
    let timer: ReturnType<typeof setInterval> | undefined;
    const start = () => {
      timer = setInterval(run, ms);
    };
    const onVisibility = () => {
      clearInterval(timer);
      if (!document.hidden) {
        void run();
        start();
      }
    };
    if (!document.hidden) {
      if (immediately) void run();
      start();
    }
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [active, ms, immediately]);
}
