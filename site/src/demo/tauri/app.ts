import tauri from "../../../../app/src-tauri/tauri.conf.json";

export async function getVersion(): Promise<string> {
  return tauri.version;
}
