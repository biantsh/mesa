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
    if (immediately) run();
    const timer = setInterval(run, ms);
    return () => clearInterval(timer);
  }, [active, ms, immediately]);
}
