export interface Layout {
  columns: number;
  stride: number;
  divider: number;
  gap: number;
}

export interface ResultWindow {
  start: number;
  end: number;
  divider: boolean;
  before: number;
  after: number;
}

function linesOf(count: number, weakAt: number | null, columns: number) {
  const close = weakAt === null ? count : weakAt;
  const closeRows = Math.ceil(close / columns);
  const restRows = weakAt === null ? 0 : Math.ceil((count - weakAt) / columns);
  return { close, closeRows, restRows, lines: closeRows + (weakAt === null ? 0 : 1) + restRows };
}

export function resultWindow(count: number, weakAt: number | null, layout: Layout, from: number, to: number, overscan: number): ResultWindow {
  const { columns, stride, divider, gap } = layout;
  const { close, closeRows, lines } = linesOf(count, weakAt, columns);
  if (lines === 0) return { start: 0, end: 0, divider: false, before: 0, after: 0 };
  const hasDivider = weakAt !== null;
  const top = (line: number) => (!hasDivider || line <= closeRows ? line * stride : closeRows * stride + divider + (line - closeRows - 1) * stride);
  const lineAt = (y: number) => {
    if (!hasDivider || y < closeRows * stride) return Math.floor(y / stride);
    if (y < closeRows * stride + divider) return closeRows;
    return closeRows + 1 + Math.floor((y - closeRows * stride - divider) / stride);
  };
  const first = Math.min(lines - 1, Math.max(0, lineAt(Math.max(0, from)) - overscan));
  const last = Math.max(first, Math.min(lines - 1, lineAt(Math.max(0, to)) + overscan));
  const startOf = (line: number) => (!hasDivider || line <= closeRows ? Math.min(close, line * columns) : close + (line - closeRows - 1) * columns);
  const endOf = (line: number) =>
    !hasDivider || line < closeRows ? Math.min(close, (line + 1) * columns) : line === closeRows ? close : Math.min(count, close + (line - closeRows) * columns);
  const total = top(lines);
  return {
    start: startOf(first),
    end: endOf(last),
    divider: hasDivider && first <= closeRows && closeRows <= last,
    before: first > 0 ? top(first) - gap : 0,
    after: last < lines - 1 ? total - top(last + 1) - gap : 0,
  };
}

export function rowTop(at: number, weakAt: number | null, layout: Layout): number {
  const { columns, stride, divider } = layout;
  if (weakAt === null || at < weakAt) return Math.floor(at / columns) * stride;
  return Math.ceil(weakAt / columns) * stride + divider + Math.floor((at - weakAt) / columns) * stride;
}

export type ResultKey = "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End";

export function moveAmong(at: number, key: ResultKey, count: number, weakAt: number | null, columns: number): number | null {
  if (count === 0) return null;
  switch (key) {
    case "Home":
      return 0;
    case "End":
      return count - 1;
    case "ArrowLeft":
      return at > 0 ? at - 1 : null;
    case "ArrowRight":
      return at < count - 1 ? at + 1 : null;
  }
  const close = weakAt === null || weakAt <= 0 ? count : weakAt;
  const [start, end] = at < close ? [0, close] : [close, count];
  const place = at - start;
  const column = place % columns;
  const row = Math.floor(place / columns);
  const lastRow = Math.floor((end - start - 1) / columns);
  if (key === "ArrowDown") {
    if (start + place + columns < end) return start + place + columns;
    if (row < lastRow) return end - 1;
    return end < count ? Math.min(end + column, count - 1) : null;
  }
  if (place >= columns) return at - columns;
  if (start > 0) return Math.min(Math.floor((close - 1) / columns) * columns + column, close - 1);
  return null;
}

const TILE_MIN = 148;
const TILE_GAP = 4;
const TILE_PADDING = 6;
const ROW_BESIDE_PICTURE = 6 + 8 + 6 + 18 + 17 + 2 * 2 + TILE_GAP;
export const PANEL_COLUMNS = 2;
const MOST_PANEL_COLUMNS = 4;
export const DEFAULT_PANEL_COLUMNS = 3;
const SCROLLBAR_ROOM = 8;
const PANEL_INSET = TILE_GAP + 2 * TILE_PADDING + SCROLLBAR_ROOM;

export function panelBeside(side: number, track: number): number | null {
  let fits = 0;
  for (let columns = PANEL_COLUMNS; columns <= DEFAULT_PANEL_COLUMNS; columns++) if (panelWidth(track, columns) <= side) fits = columns;
  return fits >= PANEL_COLUMNS ? fits : null;
}

export function panelWidth(track: number, taken: number): number {
  return Math.max(0, taken * (track + TILE_GAP) - PANEL_INSET);
}

export type PanelLayout = "narrow" | "middle" | "wide";

export function panelLayout(width: number): PanelLayout {
  return width >= 560 ? "wide" : width >= 380 ? "middle" : "narrow";
}

export function nearestPanelColumns(width: number, track: number, least: number, most: number): number {
  let nearest = least;
  for (let columns = least; columns <= most; columns++) {
    if (Math.abs(panelWidth(track, columns) - width) < Math.abs(panelWidth(track, nearest) - width)) nearest = columns;
  }
  return nearest;
}

export function panelRoom(width: number): number {
  return width + PANEL_INSET;
}

export function resultColumns(
  width: number,
  fixed?: number,
  wanted = PANEL_COLUMNS,
): { columns: number; track: number; besidePanel: number; taken: number; least: number; most: number; panel: number; room: number } {
  const columns = fixed ?? Math.max(1, Math.floor((width + TILE_GAP) / (TILE_MIN + TILE_GAP)));
  const track = (width - (columns - 1) * TILE_GAP) / columns;
  const most = Math.max(1, Math.min(MOST_PANEL_COLUMNS, columns - PANEL_COLUMNS));
  const least = Math.min(PANEL_COLUMNS, most);
  const taken = Math.max(least, Math.min(most, Math.round(wanted)));
  return {
    columns,
    track,
    besidePanel: Math.max(1, columns - taken),
    taken,
    least,
    most,
    panel: panelWidth(track, taken),
    room: taken * (track + TILE_GAP),
  };
}

export function previewHeight(track: number, pixelRatio: number): number {
  const square = track - 2 * TILE_PADDING;
  const row = Math.ceil((square + ROW_BESIDE_PICTURE) * pixelRatio - 1e-6) / pixelRatio;
  return row - ROW_BESIDE_PICTURE;
}
