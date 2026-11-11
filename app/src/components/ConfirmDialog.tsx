import { useId, type ReactNode } from "react";
import { isMac } from "../platform";
import { Button } from "./Button";
import { Modal } from "./Modal";
import styles from "./ConfirmDialog.module.css";

interface Props {
  open: boolean;
  title: string;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  alternative?: { label: string; onChoose: () => void };
  cancel?: boolean;
  children: ReactNode;
}

export function ConfirmDialog({
  open,
  title,
  confirmLabel,
  danger = false,
  onConfirm,
  onCancel,
  alternative,
  cancel = true,
  children,
}: Props) {
  const titleId = useId();

  return (
    <Modal open={open} onClose={onCancel} className={styles.dialog} labelledBy={titleId} focusCard={isMac}>
      <div className={styles.content}>
        <h2 id={titleId} className={styles.title}>
          {title}
        </h2>
        <div className={styles.body}>{children}</div>
        <div className={styles.actions}>
          {cancel && (
            <Button variant="secondary" onClick={onCancel}>
              Cancel
            </Button>
          )}
          {alternative && (
            <Button variant="secondary" onClick={alternative.onChoose}>
              {alternative.label}
            </Button>
          )}
          <Button variant={danger ? "danger" : "primary"} onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
