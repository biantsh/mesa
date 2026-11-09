import { useRef } from "react";

export function useFrozenWhile<T>(frozen: boolean, value: T): T {
  const held = useRef(value);
  if (!frozen) held.current = value;
  return frozen ? held.current : value;
}
