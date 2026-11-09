import { api, errorText, type Progress } from "./api";

export function useIndexControls(
  progress: Progress,
  onRefresh: () => Promise<void>,
  onError: (message: string) => void,
) {
  const togglePause = async () => {
    try {
      await api.setIndexingPaused(!progress.paused);
    } catch (e) {
      onError(`Couldn't ${progress.paused ? "resume" : "pause"} indexing. ${errorText(e)}`);
    }
    onRefresh();
  };

  const toggleTurbo = async () => {
    try {
      await api.setIndexingTurbo(!progress.turbo);
    } catch (e) {
      onError(`Couldn't change how fast indexing goes. ${errorText(e)}`);
    }
    onRefresh();
  };

  return { togglePause, toggleTurbo };
}
