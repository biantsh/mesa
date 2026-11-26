import { useId } from "react";
import type { UpdateStatus } from "../api";
import { cx } from "../cx";
import { useEased } from "../useEased";
import { useSwap } from "../useSwap";
import { downloadFraction } from "../useUpdateStatus";
import { Button } from "./Button";
import { Modal } from "./Modal";
import { ReleaseNotes } from "./ReleaseNotes";
import confirm from "./ConfirmDialog.module.css";
import styles from "./UpdateDialog.module.css";

interface Props {
  open: boolean;
  status: UpdateStatus | null;
  onInstall: () => void;
  onClose: () => void;
}

export function UpdateDialog({ open, status, onInstall, onClose }: Props) {
  const titleId = useId();
  const state = status?.state;
  const busy = state === "downloading" || state === "installing";
  const close = () => {
    if (!busy) onClose();
  };
  return (
    <Modal open={open} onClose={close} className={cx(confirm.dialog, styles.dialog)} labelledBy={titleId} focusCard modeless>
      {open && status && (
        <div className={cx(confirm.content, styles.content)}>
          <h2 id={titleId} className={cx(confirm.title, styles.title)}>
            Mesa {status.version} is available <span className={styles.current}>(you have {status.current})</span>
          </h2>
          <div className={confirm.body}>
            {status.notes && <ReleaseNotes notes={status.notes} />}
            {status.asksPassword && <p>Installing it will ask for an administrator's password.</p>}
            {status.error && !busy && (
              <p className={styles.error} role="alert">
                {status.error}
              </p>
            )}
          </div>
          <div className={styles.actions} data-busy={busy || undefined}>
            <div className={cx(confirm.actions, styles.buttons)} aria-hidden={busy || undefined}>
              <Button variant="secondary" onClick={onClose} disabled={busy}>
                Not Now
              </Button>
              <Button variant="primary" onClick={onInstall} disabled={busy || state !== "available"}>
                Install and Restart
              </Button>
            </div>
            {busy && <Progress status={status} />}
          </div>
        </div>
      )}
    </Modal>
  );
}

function Progress({ status }: { status: UpdateStatus }) {
  const installing = status.state === "installing";
  const known = installing ? 1 : downloadFraction(status);
  const shown = useEased(known ?? 0);
  return (
    <div className={styles.progress} role="status">
      <div className={styles.track}>
        <div className={known === null ? styles.sweep : styles.fill} style={known === null ? undefined : { transform: `scaleX(${shown})` }} />
      </div>
      <PhaseLabel installing={installing} percent={known === null ? null : Math.floor(shown * 100)} />
    </div>
  );
}

const INSTALLING = "Installing update…";

function PhaseLabel({ installing, percent }: { installing: boolean; percent: number | null }) {
  const { current, previous, changing, gone } = useSwap(installing);
  const say = (phase: boolean) => (phase ? INSTALLING : `Downloading…${percent === null ? "" : ` ${percent}%`}`);
  return (
    <span className={styles.label}>
      <span className={styles.labelSizer} aria-hidden>
        Downloading… 100%
      </span>
      <span className={styles.labelSizer} aria-hidden>
        {INSTALLING}
      </span>
      {previous !== null && (
        <span key={`was:${previous}`} className={cx(styles.phase, styles.phaseLeaving)} onAnimationEnd={gone} aria-hidden>
          {previous ? INSTALLING : "Downloading… 100%"}
        </span>
      )}
      <span key={String(current)} className={cx(styles.phase, changing && styles.phaseComing)}>
        {say(current)}
      </span>
    </span>
  );
}
