import { Copy, Minus, Square, X } from "lucide-react";
import { cx } from "../cx";
import { useAppVersion } from "../appVersion";
import logo from "../logo";
import { windowControls } from "../window";
import styles from "./TitleBar.module.css";

export function TitleBar({ maximized }: { maximized: boolean }) {
  const version = useAppVersion();
  return (
    <header className={styles.bar} data-tauri-drag-region>
      <img className={styles.logo} src={logo} alt="" draggable={false} />
      <span className={styles.title}>
        <span className={styles.name}>Mesa</span>
        {version && <span className={styles.version}>v{version}</span>}
      </span>
      <div className={styles.controls}>
        <button type="button" className={styles.control} aria-label="Minimize" title="Minimize" onClick={windowControls.minimize}>
          <Minus size={15} aria-hidden />
        </button>
        <button
          type="button"
          className={styles.control}
          aria-label={maximized ? "Restore" : "Maximize"}
          title={maximized ? "Restore" : "Maximize"}
          onClick={windowControls.toggleMaximize}
        >
          {maximized ? <Copy size={13} aria-hidden /> : <Square size={12} aria-hidden />}
        </button>
        <button type="button" className={cx(styles.control, styles.close)} aria-label="Close" title="Close" onClick={windowControls.close}>
          <X size={16} aria-hidden />
        </button>
      </div>
    </header>
  );
}
