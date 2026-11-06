export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface Placed extends Box {
  path: string;
}

export function boxBetween(a: { x: number; y: number }, b: { x: number; y: number }): Box {
  return { left: Math.min(a.x, b.x), top: Math.min(a.y, b.y), right: Math.max(a.x, b.x), bottom: Math.max(a.y, b.y) };
}

export function intersect(a: Box, b: Box): Box | null {
  const box = {
    left: Math.max(a.left, b.left),
    top: Math.max(a.top, b.top),
    right: Math.min(a.right, b.right),
    bottom: Math.min(a.bottom, b.bottom),
  };
  return box.right >= box.left && box.bottom >= box.top ? box : null;
}

export function areaOf(box: Box): number {
  return (box.right - box.left) * (box.bottom - box.top);
}

export function touching(items: readonly Placed[], box: Box): string[] {
  return items
    .filter((item) => item.right >= box.left && item.left <= box.right && item.bottom >= box.top && item.top <= box.bottom)
    .map((item) => item.path);
}
