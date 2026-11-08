import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cx } from "../cx";
import styles from "./ContextMenu.module.css";

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  trailing?: ReactNode;
  ariaLabel?: string;
  onSelect: () => void;
  disabled?: boolean;
  danger?: boolean;
}

interface Props {
  x: number;
  y: number;
  groups: MenuItem[][];
  onClose: () => void;
}

const EDGE_GAP = 8;

export function ContextMenu({ x, y, groups, onClose }: Props) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: x, top: y });

  useLayoutEffect(() => {
    const el = menu.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setPosition({
      left: Math.max(EDGE_GAP, Math.min(x, window.innerWidth - width - EDGE_GAP)),
      top: Math.max(EDGE_GAP, Math.min(y, window.innerHeight - height - EDGE_GAP)),
    });
  }, [x, y]);

  useEffect(() => {
    menu.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();

    const onPointerDown = (e: MouseEvent) => {
      if (!menu.current?.contains(e.target as Node)) onClose();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("scroll", onClose, true);
    window.addEventListener("resize", onClose);
    window.addEventListener("blur", onClose);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("scroll", onClose, true);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  const onMenuKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = Array.from(
      menu.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [],
    );
    if (items.length === 0) return;
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    const step = e.key === "ArrowDown" ? 1 : -1;
    items[(current + step + items.length) % items.length].focus();
  };

  return createPortal(
    <div
      ref={menu}
      className={styles.menu}
      style={position}
      role="menu"
      onKeyDown={onMenuKeyDown}
      onContextMenu={(e) => e.preventDefault()}
    >
      {groups.map((group, i) => (
        <div key={i} role="group">
          {i > 0 && <div className={styles.separator} role="separator" />}
          {group.map((item, n) => (
            <button
              key={`${n} ${item.label}`}
              aria-label={item.ariaLabel}
              type="button"
              role="menuitem"
              className={cx(styles.item, item.danger && styles.danger)}
              disabled={item.disabled}
              onClick={() => {
                onClose();
                item.onSelect();
              }}
            >
              {item.icon}
              <span className={styles.label}>{item.label}</span>
              {item.trailing}
            </button>
          ))}
        </div>
      ))}
    </div>,
    document.body,
  );
}
