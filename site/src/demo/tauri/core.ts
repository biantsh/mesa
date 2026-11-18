import { answer, pictureOf } from "../backend";

export function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  return answer(command, args) as Promise<T>;
}

export function convertFileSrc(path: string, _protocol = "asset"): string {
  return pictureOf(path);
}
