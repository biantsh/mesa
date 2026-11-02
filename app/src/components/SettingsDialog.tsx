import { useId, type ReactNode } from "react";
import { X } from "lucide-react";
import { useGpuAcceleration } from "../useGpuAcceleration";
import { useTheme } from "../theme";
import { IconButton } from "./IconButton";
import { Modal } from "./Modal";
import { Switch } from "./Switch";
import styles from "./SettingsDialog.module.css";

interface Props {
  open: boolean;
  onClose: () => void;
}

export function SettingsDialog({ open, onClose }: Props) {
  const titleId = useId();
  return (
    <Modal open={open} onClose={onClose} className={styles.dialog} labelledBy={titleId}>
      {open && (
        <div className={styles.content}>
          <header className={styles.header}>
            <h2 id={titleId} className={styles.title}>
              Settings
            </h2>
            <IconButton label="Close settings" onClick={onClose}>
              <X size={18} aria-hidden />
            </IconButton>
          </header>
          <div className={styles.body}>
            <Group title="Appearance">
              <ThemeRow />
            </Group>
            <Group title="Indexing">
              <GpuRow />
            </Group>
          </div>
        </div>
      )}
    </Modal>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className={styles.group}>
      <h3 className={styles.groupTitle}>{title}</h3>
      <div className={styles.rows}>{children}</div>
    </section>
  );
}

interface RowProps {
  title: string;
  description?: string;
  note?: ReactNode;
  control: (ids: { labelledBy: string; describedBy: string }) => ReactNode;
}

function Row({ title, description, note, control }: RowProps) {
  const titleId = useId();
  const textId = useId();
  return (
    <div className={styles.row}>
      <div className={styles.text}>
        <div id={titleId} className={styles.rowTitle}>
          {title}
        </div>
        <div id={textId} className={styles.description}>
          {description}
          {note && <span className={styles.note}>{note}</span>}
        </div>
      </div>
      {control({ labelledBy: titleId, describedBy: textId })}
    </div>
  );
}

function ThemeRow() {
  const [theme, setTheme] = useTheme();
  return (
    <Row
      title="Dark theme"
      description="Light text on a dark background."
      control={(ids) => <Switch checked={theme === "dark"} onChange={(dark) => setTheme(dark ? "dark" : "light")} {...ids} />}
    />
  );
}

function GpuRow() {
  const { enabled, gpu, pending, error, change } = useGpuAcceleration();
  const on = pending ?? (enabled === true && gpu != null);
  const known = enabled !== null && gpu !== undefined;
  const disabled = !known || gpu === null || pending !== null;
  const note = gpu === undefined ? "Looking for a GPU…" : (gpu ?? "No usable GPU found");

  return (
    <>
      <Row
        title="GPU acceleration"
        note={note}
        control={(ids) => <Switch checked={on} disabled={disabled} onChange={change} {...ids} />}
      />
      {error && (
        <div className={styles.error} role="alert">
          {error}
        </div>
      )}
    </>
  );
}
