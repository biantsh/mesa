import { api, type ModelStatus } from "./api";
import logo from "./logo";

export const startup: { modelStatus: ModelStatus | null } = { modelStatus: null };

const GIVE_UP_MS = 1500;

export async function ready(): Promise<void> {
  const image = new Image();
  image.src = logo;
  const loading = Promise.all([
    document.fonts.load('600 14px "Inter Variable"', "Index"),
    image.decode(),
    api
      .modelStatus()
      .then((status) => {
        startup.modelStatus = status;
      })
      .catch(() => {}),
  ]);
  const giveUp = new Promise<void>((resolve) => setTimeout(resolve, GIVE_UP_MS));
  await Promise.race([loading, giveUp]).catch(() => {});
}
