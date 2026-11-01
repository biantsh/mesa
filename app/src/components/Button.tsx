import type { ButtonHTMLAttributes } from "react";
import { cx } from "../cx";
import styles from "./Button.module.css";

type Variant = "primary" | "secondary" | "danger" | "dangerOutline";

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant: Variant;
}

export function Button({ variant, className, type = "button", ...rest }: Props) {
  return <button type={type} className={cx(styles.button, styles[variant], className)} {...rest} />;
}
