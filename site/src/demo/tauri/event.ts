type Handler = (event: { event: string; payload: unknown; id: number }) => void;

const handlers = new Map<string, Set<Handler>>();

export async function listen<T>(name: string, handler: (event: { event: string; payload: T; id: number }) => void): Promise<() => void> {
  const set = handlers.get(name) ?? new Set<Handler>();
  handlers.set(name, set);
  set.add(handler as Handler);
  return () => set.delete(handler as Handler);
}

export async function emit(name: string, payload?: unknown): Promise<void> {
  for (const handler of handlers.get(name) ?? []) handler({ event: name, payload, id: 0 });
}
