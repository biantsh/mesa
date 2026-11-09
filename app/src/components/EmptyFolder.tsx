import styles from "./EmptyFolder.module.css";

export function EmptyFolder() {
  return (
    <div className={styles.empty}>
      <p className={styles.title}>Nothing to show here.</p>
      <p className={styles.hint}>Only folders and supported files (images, video, audio and documents) are listed.</p>
    </div>
  );
}
