import { useEffect, type RefObject } from "react";

export function useRevealOnMount(element: RefObject<HTMLElement | null>) {
  useEffect(() => {
    element.current?.scrollIntoView({ block: "nearest" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}
