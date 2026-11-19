import { useCallback, useState } from "react";

export function useSwap<T>(value: T) {
  const [current, setCurrent] = useState(value);
  const [previous, setPrevious] = useState<{ value: T } | null>(null);
  if (value !== current) {
    setPrevious({ value: current });
    setCurrent(value);
  }
  const gone = useCallback(() => setPrevious(null), []);
  return { current, previous: previous?.value ?? null, changing: previous !== null, gone };
}
