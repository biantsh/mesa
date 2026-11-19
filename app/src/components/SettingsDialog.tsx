import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import { cx } from "../cx";
import { modifierLabels, keyLabel } from "../hotkey";
import { isWebKit, platform } from "../platform";
import type { HotkeyParts } from "../api";
import { useGpuAcceleration } from "../useGpuAcceleration";
import { useSearchHotkey } from "../useSearchHotkey";
import { useTheme, type ThemeChoice } from "../theme";
import { api, type Settings } from "../api";
import { useSwitchSetting } from "../useSwitchSetting";
import { hasUpdate, type useUpdateStatus } from "../useUpdateStatus";
import { Reveal } from "./Reveal";
import { ShiftingButton } from "./ShiftingButton";
import { Segmented, type Segment } from "./Segmented";
import { IconButton } from "./IconButton";
import { Keycaps } from "./Keycaps";
import { Modal } from "./Modal";
import { ShortcutField } from "./ShortcutField";
import { Switch } from "./Switch";
import styles from "./SettingsDialog.module.css";

interface Props {
  open: boolean;
  onClose: () => void;
  update: ReturnType<typeof useUpdateStatus>;
  onOpenUpdate: () => void;
}

export function SettingsDialog({ open, onClose, update, onOpenUpdate }: Props) {
  const titleId = useId();
  const gpu = useGpuAcceleration();
  const fullSpeed = useSwitchSetting(readFullSpeed, writeFullSpeed);
  const signIn = useSwitchSetting(readSignIn, writeSignIn, open);
  const updateChecks = useSwitchSetting(readUpdateChecks, writeUpdateChecks);
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
            <Group title="General">
              <ThemeRow />
              <SwitchRow
                title="Start when you sign in"
                description="Automatically launch the app in the background."
                {...signIn}
              />
            </Group>
            <Group title="Updates">
              <VersionRow {...update} onOpenUpdate={onOpenUpdate} />
              {update.status?.available && (
                <SwitchRow
                  title="Auto-check for updates"
                  description="Check for updates once per day."
                  {...updateChecks}
                />
              )}
            </Group>
            <Group title="Shortcuts">
              <HotkeyRow {...hotkey} />
            </Group>
            <Group title="Indexing">
              <GpuRow {...gpu} />
              <SwitchRow
                title="Indexing starts at full speed"
                description="Faster, but uses more resources while indexing."
                {...fullSpeed}
              />
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

const THEMES: Segment<ThemeChoice>[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "System" },
];

function ThemeRow() {
  const [theme, setTheme] = useTheme();
  return (
    <Row
      title="Theme"
      description="“System” matches your computer."
      control={(ids) => <Segmented segments={THEMES} value={theme} onChange={setTheme} {...ids} />}
    />
  );
}

const readFullSpeed = (settings: Settings) => settings.alwaysFullSpeed;
const writeFullSpeed = (enabled: boolean) => api.setAlwaysFullSpeed(enabled);
const readSignIn = (settings: Settings) => settings.startAtSignIn;
const writeSignIn = (enabled: boolean) => api.setStartAtSignIn(enabled);
const readUpdateChecks = (settings: Settings) => settings.checkForUpdates;
const writeUpdateChecks = (enabled: boolean) => api.setCheckForUpdates(enabled);

function SwitchRow({ title, description, enabled, pending, error, change }: { title: string; description: string } & ReturnType<typeof useSwitchSetting>) {
  return (
    <>
      <Row
        title={title}
        description={description}
        control={(ids) =>
          enabled === null ? (
            <span className={styles.switchSpace} aria-hidden />
          ) : (
            <Switch checked={pending ?? enabled} disabled={pending !== null} onChange={change} {...ids} />
          )
        }
      />
      <SettingError error={error} />
    </>
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

function VersionRow({ status, check, onOpenUpdate }: ReturnType<typeof useUpdateStatus> & { onOpenUpdate: () => void }) {
  const checking = useHeldOn(status?.state === "checking", CHECKING_AT_LEAST_MS);
  const found = hasUpdate(status) && !checking;
  const note = !status ? "\u00a0" : found ? `Mesa ${status.current} (${status.version} available)` : `Mesa ${status.current}`;
  const failure = status?.available && !found && !checking ? status.error : null;
  const [lastFailure, setLastFailure] = useState(failure);
  if (failure && failure !== lastFailure) setLastFailure(failure);
  return (
    <Row
      title="App version"
      note={<ChangingText text={note} />}
      control={() =>
        status?.available && (
          <span className={styles.controls}>
            <Reveal show={failure !== null} from="right">
              <span className={styles.failure} role="alert">
                {lastFailure}
              </span>
            </Reveal>
            <ShiftingButton
              variant={found ? "primary" : "neutral"}
              className={styles.rowButton}
              label={checking ? "Checking…" : found ? "Update…" : "Check now"}
              onClick={found ? onOpenUpdate : check}
              disabled={checking}
            />
          </span>
        )
      }
    />
  );
}

const CHECKING_AT_LEAST_MS = 600;

function useHeldOn(on: boolean, ms: number): boolean {
  const [held, setHeld] = useState(false);
  const since = useRef(0);
  useEffect(() => {
    if (on) {
      since.current = performance.now();
      setHeld(true);
      return;
    }
    const left = ms - (performance.now() - since.current);
    if (left <= 0) {
      setHeld(false);
      return;
    }
    const timer = setTimeout(() => setHeld(false), left);
    return () => clearTimeout(timer);
  }, [on, ms]);
  return on || held;
}

function ChangingText({ text }: { text: string }) {
  const [first] = useState(text);
  const [changed, setChanged] = useState(false);
  if (text !== first && !changed) setChanged(true);
  return (
    <span key={text} className={changed ? styles.arrive : undefined}>
      {text}
    </span>
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
