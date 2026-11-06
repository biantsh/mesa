import { useRef, type MouseEvent, type PointerEvent } from "react";
import { Copy, Minus, Square, X } from "lucide-react";
import { pressBackground } from "../background";
import { cx } from "../cx";
import { releaseFocus } from "../focus";
import { useAppVersion } from "../appVersion";
import logo from "../logo";
import { windowControls } from "../window";
import styles from "./TitleBar.module.css";

const DRAG_PX = 4;

function useBarPress() {
  const press = useRef<{ id: number; x: number; y: number } | null>(null);
  const onBar = (e: MouseEvent<HTMLElement>) => e.target === e.currentTarget;
  return {
    onPointerDown(e: PointerEvent<HTMLElement>) {
      if (e.button !== 0 || !onBar(e)) return;
      e.preventDefault();
      e.currentTarget.setPointerCapture(e.pointerId);
      press.current = { id: e.pointerId, x: e.clientX, y: e.clientY };
    },
    onPointerMove(e: PointerEvent<HTMLElement>) {
      const start = press.current;
      if (!start || e.pointerId !== start.id) return;
      if (Math.hypot(e.clientX - start.x, e.clientY - start.y) < DRAG_PX) return;
      press.current = null;
      e.currentTarget.releasePointerCapture(e.pointerId);
      windowControls.startDragging();
    },
    onPointerUp(e: PointerEvent<HTMLElement>) {
      const start = press.current;
      press.current = null;
      if (!start || e.pointerId !== start.id) return;
      e.currentTarget.releasePointerCapture(e.pointerId);
      releaseFocus();
      pressBackground();
    },
    onPointerCancel() {
      press.current = null;
    },
    onDoubleClick(e: MouseEvent<HTMLElement>) {
      if (onBar(e)) windowControls.toggleMaximize();
    },
  };
}

export function TitleBar({ maximized }: { maximized: boolean }) {
  const version = useAppVersion();
  const pressed = useBarPress();
  return (
    <header className={styles.bar} {...pressed}>
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
