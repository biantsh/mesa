import { ArrowUpRight } from "lucide-react";
import { api } from "../api";
import styles from "./LegalCorner.module.css";

const LICENSE = "MIT License";

export function LegalCorner() {
  return (
    <span className={styles.legal}>
      <span>{LICENSE}</span>
      <span className={styles.divider} aria-hidden />
      <button
        type="button"
        className={styles.project}
        onClick={() => api.openProjectPage().catch(() => {})}
      >
        GitHub
        <ArrowUpRight size={12} aria-hidden />
      </button>
    </span>
  );
}
