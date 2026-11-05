import { useEffect, useState } from "react";
import { api } from "./api";
import { useSwitchSetting } from "./useSwitchSetting";

const read = (settings: { gpuAcceleration: boolean }) => settings.gpuAcceleration;
const write = (enabled: boolean) => api.setGpuAcceleration(enabled);

export function useGpuAcceleration() {
  const setting = useSwitchSetting(read, write);
  const [gpu, setGpu] = useState<string | null | undefined>(undefined);

  useEffect(() => {
    let cancelled = false;
    api
      .embeddingGpu()
      .then((name) => !cancelled && setGpu(name))
      .catch(() => !cancelled && setGpu(null));
    return () => {
      cancelled = true;
    };
  }, []);

  return { ...setting, gpu };
}
