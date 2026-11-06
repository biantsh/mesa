import type { ButtonHTMLAttributes, Ref } from "react";
import { cx } from "../cx";
import styles from "./Button.module.css";

type Variant = "primary" | "secondary" | "neutral" | "danger" | "dangerOutline";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant: Variant;
  ref?: Ref<HTMLButtonElement>;
}

export function Button({ variant, className, type = "button", ...rest }: ButtonProps) {
  return <button type={type} className={cx(styles.button, styles[variant], className)} {...rest} />;
}
