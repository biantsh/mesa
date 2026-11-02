import { useEffect, useRef, type ReactNode } from "react";
import { cx } from "../cx";
import styles from "./Modal.module.css";

interface Props {
  open: boolean;
  onClose: () => void;
  className?: string;
  label?: string;
  labelledBy?: string;
  children: ReactNode;
}

export function Modal({ open, onClose, className, label, labelledBy, children }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const el = dialog.current;
    if (!el) return;
    if (open && !el.open) el.showModal();
    if (!open && el.open) el.close();
  }, [open]);

  return (
    <dialog
      ref={dialog}
      className={cx(styles.dialog, className)}
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
  );
}
