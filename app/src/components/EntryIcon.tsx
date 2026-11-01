import { File, FileMusic, HardDrive } from "lucide-react";
import type { Entry } from "../api";
import { FolderIcon } from "./FolderIcon";
import styles from "./EntryIcon.module.css";

interface Props {
  entry: Entry;
  size: number;
}

export function EntryIcon({ entry, size }: Props) {
  const props = { size, strokeWidth: size >= 40 ? 1.4 : 1.8, "aria-hidden": true } as const;
  if (entry.kind === "drive") return <HardDrive {...props} className={styles.drive} />;
  if (entry.kind === "folder") {
    return <FolderIcon full={entry.hasContents} size={(size * 76) / 60} className={styles.folder} />;
  }
  if (entry.category === "audio") return <FileMusic {...props} className={styles.file} />;
  return <File {...props} className={styles.file} />;
}
