import { useEffect, useState } from "react";

export interface Arrival {
  playing: boolean;
  late: number;
}

const ARRIVING_MS = 1000;

export function useFirstArrival(active: boolean, loaded: boolean): Arrival | null {
  const [cameAt, setCameAt] = useState<number | null>(null);
  const came = cameAt ?? (active ? performance.now() : null);
  if (came !== cameAt) setCameAt(came);
  const [arrival, setArrival] = useState<Arrival | null>(null);
  if (arrival === null && came !== null && active && loaded) {
    setArrival({ playing: true, late: Math.round(performance.now() - came) });
  }
  useEffect(() => {
    if (!arrival?.playing) return;
    const timer = setTimeout(() => setArrival({ ...arrival, playing: false }), ARRIVING_MS);
    return () => clearTimeout(timer);
  }, [arrival]);
  return arrival;
}
