import { useId, type ReactNode } from "react";
import { X } from "lucide-react";
import { cx } from "../cx";
import { modifierLabels, keyLabel } from "../hotkey";
import { isWebKit, platform } from "../platform";
import type { HotkeyParts } from "../api";
import { useGpuAcceleration } from "../useGpuAcceleration";
import { useSearchHotkey } from "../useSearchHotkey";
import { useTheme } from "../theme";
import { IconButton } from "./IconButton";
import { Keycaps } from "./Keycaps";
import { Modal } from "./Modal";
import { ShortcutField } from "./ShortcutField";
import { Switch } from "./Switch";
import styles from "./SettingsDialog.module.css";

interface Props {
  open: boolean;
  onClose: () => void;
}

export function SettingsDialog({ open, onClose }: Props) {
  const titleId = useId();
  const gpu = useGpuAcceleration();
  const hotkey = useSearchHotkey(open);
  const close = () => {
    if (!hotkey.isRecording()) onClose();
  };
  return (
    <Modal open={open} onClose={close} className={styles.dialog} labelledBy={titleId} focusCard={isWebKit} modeless>
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
            <Group title="Shortcuts">
              <HotkeyRow {...hotkey} />
            </Group>
            <Group title="Indexing">
              <GpuRow {...gpu} />
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
  disabled?: boolean;
}

function Row({ title, description, note, control, disabled }: RowProps) {
  const titleId = useId();
  const textId = useId();
  return (
    <div className={cx(styles.row, disabled && styles.disabled)} data-setting-row>
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

function HotkeyRow(hotkey: ReturnType<typeof useSearchHotkey>) {
  const chosenIn = hotkey.status?.chosenIn;
  if (chosenIn?.kind === "desktop") return <DesktopHotkeyRow active={hotkey.status?.active ?? null} />;
  if (chosenIn?.kind === "unavailable") return <UnavailableHotkeyRow desktop={chosenIn.desktop} />;
  return <RecordedHotkeyRow {...hotkey} />;
}

function DesktopHotkeyRow({ active }: { active: HotkeyParts | null }) {
  return (
    <Row
      title="Open search"
      description="Search from anywhere on your computer."
      note="Change it in your desktop's keyboard settings."
      control={() => (active ? <Keycaps keys={active.keys} /> : <span className={styles.none}>None</span>)}
    />
  );
}

function UnavailableHotkeyRow({ desktop }: { desktop: string | null }) {
  const why = desktop ? `Mesa can't set shortcuts in ${desktop} on Wayland.` : "Mesa can't set shortcuts in this Wayland session.";
  return (
    <Row
      title="Open search"
      note={why}
      disabled
      control={(ids) => (
        <ShortcutField
          listening={false}
          live={[]}
          refusedLast={false}
          prompt=""
          disabled
          label={`Search shortcut: none. ${why}`}
          describedBy={ids.describedBy}
          tooltip={why}
          onClick={() => {}}
        >
          <span className={styles.none}>None</span>
        </ShortcutField>
      )}
    />
  );
}

function RecordedHotkeyRow({ status, recording, held, refused, error, saving, field, start, cancel }: ReturnType<typeof useSearchHotkey>) {
  const active = status?.active ?? null;
  const live = [...modifierLabels(held, platform), ...(refused ? [keyLabel(refused)] : [])];

  return (
    <>
      <Row
        title="Open search"
        description="Search from anywhere on your computer."
        control={(ids) => (
          <ShortcutField
            ref={field}
            listening={recording}
            live={live}
            refusedLast={refused !== null}
            prompt="Type shortcut"
            disabled={!status || saving}
            label={recording ? "Recording the new shortcut. Type it, or press Escape to cancel." : `Search shortcut: ${active?.name ?? "none"}. Press to change.`}
            describedBy={ids.describedBy}
            tooltip={recording ? "Press Escape to cancel" : "Click to change"}
            onClick={(e) => (recording ? cancel() : start(e.detail === 0))}
          >
            {status ? active ? <Keycaps keys={active.keys} /> : <span className={styles.none}>None</span> : null}
          </ShortcutField>
        )}
      />
      <SettingError error={error} />
    </>
  );
}

function GpuRow({ enabled, gpu, sharedGpu, pending, error, change }: ReturnType<typeof useGpuAcceleration>) {
  const on = pending ?? (enabled === true && gpu != null);
  const known = enabled !== null && gpu !== undefined;
  const disabled = !known || gpu === null || pending !== null;

  const note = !known ? "\u00a0" : gpu ? `Use your “${gpu}” GPU.` : sharedGpu ? "Not available on integrated GPUs." : "No usable GPU found.";

  return (
    <>
      <Row
        title="GPU acceleration"
        note={note}
        disabled={sharedGpu !== null}
        control={(ids) =>
          known ? (
            <Switch checked={on} disabled={disabled} onChange={change} {...ids} />
          ) : (
            <span className={styles.switchSpace} aria-hidden />
          )
        }
      />
      <SettingError error={error} />
    </>
  );
}

function SettingError({ error }: { error: string | null }) {
  return (
    error && (
      <div className={styles.error} role="alert">
        {error}
      </div>
    )
  );
}
