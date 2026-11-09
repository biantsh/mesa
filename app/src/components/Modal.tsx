import { useEffect, useRef, type ReactNode } from "react";
import { cx } from "../cx";
import styles from "./Modal.module.css";

interface Props {
  open: boolean;
  onClose: () => void;
  className?: string;
  label?: string;
  labelledBy?: string;
  focusCard?: boolean;
  modeless?: boolean;
  children: ReactNode;
}

export function Modal({ open, onClose, className, label, labelledBy, focusCard, modeless, children }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const el = dialog.current;
    if (!el) return;
    if (open && !el.open) {
      if (modeless) el.show();
      else el.showModal();
      if (focusCard) el.focus();
    }
    if (!open && el.open) el.close();
  }, [open, focusCard, modeless]);

  useEffect(() => {
    if (!open || !modeless) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      e.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, modeless, onClose]);

  return (
    <>
      {modeless && <div className={cx(styles.scrim, open && styles.scrimOpen)} onClick={onClose} aria-hidden />}
      <dialog
        ref={dialog}
        className={cx(styles.dialog, modeless && styles.modeless, className)}
        tabIndex={focusCard ? -1 : undefined}
        aria-label={label}
        aria-labelledby={labelledBy}
        onCancel={(e) => {
          e.preventDefault();
          onClose();
        }}
        onClick={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
      >
        {children}
      </dialog>
    </>
  );
}
