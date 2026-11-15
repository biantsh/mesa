import { useEffect, useState } from "react";
import { api } from "./api";
import { useSwitchSetting } from "./useSwitchSetting";

const read = (settings: { gpuAcceleration: boolean }) => settings.gpuAcceleration;
const write = (enabled: boolean) => api.setGpuAcceleration(enabled);

export function useGpuAcceleration() {
  const setting = useSwitchSetting(read, write);
  const [found, setFound] = useState<{ gpu: string | null; sharedGpu: string | null } | undefined>(undefined);

  useEffect(() => {
    let cancelled = false;
    api
      .embeddingGpu()
      .then(
        (found) =>
          !cancelled &&
          setFound({
            gpu: found && !found.shared ? found.name : null,
            sharedGpu: found?.shared ? found.name : null,
          }),
      )
      .catch(() => !cancelled && setFound({ gpu: null, sharedGpu: null }));
    return () => {
      cancelled = true;
    };
  }, []);

  return { ...setting, gpu: found?.gpu, sharedGpu: found?.sharedGpu ?? null };
}
