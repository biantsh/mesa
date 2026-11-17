import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronLeft, ChevronRight, Files } from "lucide-react";
import type { Category } from "../api";
import { cx } from "../cx";
import { CATEGORY_LABELS } from "../format";
import { MONTH_LABELS, monthOf, type Month } from "../understand";
import { CATEGORY_ICONS } from "./ResultGrid";
import styles from "./FilterPickers.module.css";

const EDGE_GAP = 8;
const ANCHOR_GAP = 6;
const FIRST_YEAR = 1970;
const YEARS_SHOWN = 12;

interface PopoverProps {
  anchor: HTMLElement;
  role: "menu" | "dialog";
  label: string;
  onClose: () => void;
  children: ReactNode;
}

function Popover({ anchor, role, label, onClose, children }: PopoverProps) {
  const popover = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: 0, top: 0 });

  useLayoutEffect(() => {
    const el = popover.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const place = anchor.getBoundingClientRect();
    setPosition({
      left: Math.max(EDGE_GAP, Math.min(place.left, window.innerWidth - width - EDGE_GAP)),
      top: Math.max(EDGE_GAP, Math.min(place.bottom + ANCHOR_GAP, window.innerHeight - height - EDGE_GAP)),
    });
  }, [anchor]);

  useEffect(() => {
    const onPointerDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (!popover.current?.contains(target) && !anchor.contains(target)) onClose();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      onClose();
    };
    document.addEventListener("mousedown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("scroll", onClose, true);
    window.addEventListener("resize", onClose);
    window.addEventListener("blur", onClose);
    return () => {
      document.removeEventListener("mousedown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("scroll", onClose, true);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("blur", onClose);
    };
  }, [anchor, onClose]);

  return createPortal(
    <div
      ref={popover}
      className={styles.popover}
      style={position}
      data-popup=""
      role={role}
      aria-label={label}
      onMouseDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
      onClick={(e) => e.stopPropagation()}
      onWheel={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {children}
    </div>,
    document.body,
  );
}

const TYPES: Category[] = ["image", "audio", "video", "document", "code"];

interface TypePickerProps {
  anchor: HTMLElement;
  value: Category | null;
  indexed: Category[] | null;
  onChoose: (value: Category | null) => void;
  onClose: () => void;
}

export function TypePicker({ anchor, value, indexed, onChoose, onClose }: TypePickerProps) {
  const item = (type: Category | null, icon: ReactNode, name: string) => {
    const off = type !== null && indexed !== null && !indexed.includes(type);
    return (
      <button
        key={name}
        type="button"
        role="menuitemradio"
        aria-checked={value === type}
        className={cx(styles.item, off && styles.itemOff)}
        onClick={() => {
          onClose();
          onChoose(type);
        }}
      >
        {icon}
        <span className={styles.itemName}>{name}</span>
        {off && <span className={styles.itemNote}>Not indexed</span>}
        {value === type && <Check size={14} aria-hidden />}
      </button>
    );
  };
  return (
    <Popover anchor={anchor} role="menu" label="File type" onClose={onClose}>
      {item(null, <Files size={14} aria-hidden />, "Any type")}
      {TYPES.map((type) => {
        const Icon = CATEGORY_ICONS[type];
        return item(type, <Icon size={14} aria-hidden />, CATEGORY_LABELS[type]);
      })}
    </Popover>
  );
}

interface MonthPickerProps {
  anchor: HTMLElement;
  title: string;
  value: Month | null;
  now: Date;
  onChoose: (value: Month | null) => void;
  onClose: () => void;
}

export function MonthPicker({ anchor, title, value, now, onChoose, onClose }: MonthPickerProps) {
  const thisYear = now.getFullYear();
  const thisMonth = monthOf(thisYear, now.getMonth());
  const [year, setYear] = useState(value !== null ? Math.floor(value / 12) : thisYear);
  const [listFrom, setListFrom] = useState<number | null>(null);

  const choose = (month: Month | null) => {
    onClose();
    onChoose(month);
  };
  const openList = () => setListFrom(year - ((year - FIRST_YEAR) % YEARS_SHOWN));

  const arrows =
    listFrom !== null
      ? {
          back: listFrom > FIRST_YEAR ? () => setListFrom(listFrom - YEARS_SHOWN) : undefined,
          on: listFrom + YEARS_SHOWN <= thisYear ? () => setListFrom(listFrom + YEARS_SHOWN) : undefined,
        }
      : {
          back: year > FIRST_YEAR ? () => setYear(year - 1) : undefined,
          on: year < thisYear ? () => setYear(year + 1) : undefined,
        };

  return (
    <Popover anchor={anchor} role="dialog" label={title} onClose={onClose}>
      <div className={styles.title}>{title}</div>
      <div className={styles.header}>
        <button type="button" className={styles.arrow} aria-label="Earlier" disabled={!arrows.back} onClick={arrows.back}>
          <ChevronLeft size={16} aria-hidden />
        </button>
        <button
          type="button"
          className={styles.heading}
          aria-label={listFrom !== null ? "Back to the months" : "Choose a year"}
          onClick={() => (listFrom !== null ? setListFrom(null) : openList())}
        >
          {listFrom !== null ? `${listFrom} – ${Math.min(listFrom + YEARS_SHOWN - 1, thisYear)}` : year}
        </button>
        <button type="button" className={styles.arrow} aria-label="Later" disabled={!arrows.on} onClick={arrows.on}>
          <ChevronRight size={16} aria-hidden />
        </button>
      </div>
      <div className={styles.grid}>
        {listFrom !== null
          ? Array.from({ length: YEARS_SHOWN }, (_, offset) => listFrom + offset).map((shown) => (
              <button
                key={shown}
                type="button"
                className={cx(styles.cell, shown === year && styles.chosen)}
                disabled={shown > thisYear}
                onClick={() => {
                  setYear(shown);
                  setListFrom(null);
                }}
              >
                {shown}
              </button>
            ))
          : MONTH_LABELS.map((label, number) => {
              const month = monthOf(year, number);
              return (
                <button
                  key={label}
                  type="button"
                  className={cx(styles.cell, month === value && styles.chosen)}
                  disabled={month > thisMonth}
                  onClick={() => choose(month)}
                >
                  {label}
                </button>
              );
            })}
      </div>
      <button type="button" className={cx(styles.item, styles.anyTime)} onClick={() => choose(null)}>
        <span className={styles.itemName}>Any time</span>
        {value === null && <Check size={14} aria-hidden />}
      </button>
    </Popover>
  );
}
