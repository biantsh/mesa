import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cx } from "../cx";
import styles from "./IconButton.module.css";

interface Props extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "title" | "children"> {
  label: string;
  tone?: "neutral" | "danger";
  children: ReactNode;
}

export function IconButton({ label, tone = "neutral", className, type = "button", children, ...rest }: Props) {
  return (
    <button
      type={type}
      className={cx(styles.button, tone === "danger" && styles.danger, className)}
      data-tooltip={label}
      aria-label={label}
      {...rest}
    >
      {children}
    </button>
  );
}
