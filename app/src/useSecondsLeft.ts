import { useEffect, useRef, useState } from "react";
import type { Progress } from "./api";
import { addSample, speedOf, type Sample } from "./eta";

const NOTHING: Progress = {
  name: "",
  queued: 0,
  counting: false,
  found: 0,
  done: 0,
  total: 0,
  paused: false,
  updating: false,
  folders: [],
  turbo: false,
};

export function useSecondsLeft(progress: Progress | null): number | null {
  const { done, total, paused, counting, turbo } = progress ?? NOTHING;
  const samples = useRef<Sample[]>([]);
  const lastDone = useRef(done);
  const [speed, setSpeed] = useState<number | null>(null);
  useEffect(() => {
    const another = done < lastDone.current;
    lastDone.current = done;
    if (total === 0 || done >= total) {
      samples.current = [];
      setSpeed(null);
    } else if (paused || counting) {
      samples.current = [];
      if (another) setSpeed(null);
    } else {
      samples.current = addSample(samples.current, { t: Date.now(), done, turbo });
      setSpeed((current) => speedOf(samples.current) ?? (another ? null : current));
    }
  }, [done, total, paused, counting, turbo]);
  if (paused || counting || speed === null || total === 0 || done >= total) return null;
  return (total - done) / speed;
}
