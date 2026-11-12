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
