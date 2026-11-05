import { api } from "./api";
import { useSwitchSetting } from "./useSwitchSetting";

const read = (settings: { turboIndexing: boolean }) => settings.turboIndexing;
const write = (enabled: boolean) => api.setTurboIndexing(enabled);

export function useTurboIndexing() {
  return useSwitchSetting(read, write);
}
