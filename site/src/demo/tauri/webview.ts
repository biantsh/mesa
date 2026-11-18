import { isWebKit } from "../../../../app/src/platform";

type Position = { x: number; y: number };
type DragDrop =
  | { type: "enter"; paths: string[]; position: Position }
  | { type: "over"; position: Position }
  | { type: "drop"; paths: string[]; position: Position }
  | { type: "leave" };
type Handler = (event: { payload: DragDrop }) => void;

const handlers = new Set<Handler>();

export function getCurrentWebview() {
  return {
    onDragDropEvent: async (handler: Handler): Promise<() => void> => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
  };
}

const said = ({ x, y }: Position): Position => {
  const scale = isWebKit ? 1 : window.devicePixelRatio;
  return { x: x * scale, y: y * scale };
};

const say = (payload: DragDrop) => {
  for (const handler of handlers) handler({ payload });
};

export const dragFiles = {
  enter: (paths: string[], at: Position) => say({ type: "enter", paths, position: said(at) }),
  over: (at: Position) => say({ type: "over", position: said(at) }),
  drop: (paths: string[], at: Position) => say({ type: "drop", paths, position: said(at) }),
  leave: () => say({ type: "leave" }),
};
