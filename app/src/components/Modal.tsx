import { useEffect, useRef, type ReactNode } from "react";
import { cx } from "../cx";
import { lastWasPointer, skipGivingBackTo } from "../lastInput";
import "../registeredZoom";
import styles from "./Modal.module.css";

interface Props {
  open: boolean;
  onClose: () => void;
  className?: string;
  label?: string;
  labelledBy?: string;
  // Focus the card, not the first control: WebKit draws a focus ring on a control focused this way even after a click.
  focusCard?: boolean;
  modeless?: boolean;
  children: ReactNode;
}

export function Modal({ open, onClose, className, label, labelledBy, focusCard, modeless, children }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const openedByPointer = useRef(false);
  const openedAt = useRef(0);

  useEffect(() => {
    const el = dialog.current;
    if (!el) return;
    if (open && !el.open) {
      openedByPointer.current = lastWasPointer();
      openedAt.current = performance.now();
      window.getSelection()?.removeAllRanges();
      if (modeless) el.show();
      else el.showModal();
      if (focusCard) el.focus();
    }
    if (!open && el.open) {
      el.close();
      const back = document.activeElement;
      if (back instanceof HTMLElement && skipGivingBackTo(back, openedByPointer.current)) back.blur();
    }
  }, [open, focusCard, modeless]);

  useEffect(() => {
    if (!open || !modeless) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      } else if (e.key === "Tab" && dialog.current) {
        keepTabInside(dialog.current, e);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, modeless, onClose]);

  return (
    <>
      {modeless && <div className={cx(styles.scrim, open && styles.scrimOpen)} aria-hidden />}
      {modeless && <div className={cx(styles.catcher, open && styles.catcherOpen)} onClick={onClose} aria-hidden />}
      <dialog
        ref={dialog}
        className={cx(styles.dialog, modeless && styles.modeless, className)}
        tabIndex={focusCard ? -1 : undefined}
        aria-label={label}
        aria-labelledby={labelledBy}
        onMouseDownCapture={(e) => {
          if (performance.now() - openedAt.current < CLICK_THROUGH_GUARD_MS) e.preventDefault();
          else if (e.target instanceof Element && !isSelectable(e.target)) window.getSelection()?.removeAllRanges();
        }}
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

const CLICK_THROUGH_GUARD_MS = 500;

function isSelectable(element: Element) {
  const style = getComputedStyle(element);
  return (style.userSelect || style.webkitUserSelect) !== "none";
}

const TAB_STOPS =
  'a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

function keepTabInside(card: HTMLElement, e: KeyboardEvent) {
  const stops = Array.from(card.querySelectorAll<HTMLElement>(TAB_STOPS)).filter((stop) => stop.getClientRects().length > 0);
  const [start, end] = e.shiftKey ? [stops[stops.length - 1], stops[0]] : [stops[0], stops[stops.length - 1]];
  const at = document.activeElement;
  if (at !== card && card.contains(at) && at !== end) return;
  e.preventDefault();
  start?.focus();
}
