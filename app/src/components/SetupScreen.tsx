import { useEffect, useRef, useState } from "react";
import { api, type ModelStatus } from "../api";
import { cx } from "../cx";
import { describeDownload } from "../format";
import { prefersReducedMotion } from "../motion";
import logo from "../logo";
import { Button } from "./Button";
import styles from "./SetupScreen.module.css";

interface Props {
  status: ModelStatus;
  leaving: boolean;
}

function describe(status: ModelStatus): string {
  switch (status.state) {
    case "downloading":
      return describeDownload(status.done, status.total);
    case "verifying":
      return "Checking the download…";
    case "finalizing":
      return "Finalizing setup…";
    case "starting":
      return "Starting…";
    case "preparing":
      return "Preparing for your graphics card…";
    case "ready":
      return "Ready";
    case "failed":
      return status.reason;
  }
}

export function SetupScreen({ status, leaving }: Props) {
  const failed = status.state === "failed";
  const downloading = status.state === "downloading";
  const access = useFolderAccessStep(downloading);
  const percent = downloading && status.total > 0 ? Math.min(100, Math.floor((status.done / status.total) * 100)) : 0;
  const determinate = downloading || status.state === "ready";
  const step = downloading ? "Downloading model weights…" : "Setting up EmbeddingGemma 2…";

  return (
    <div className={cx(styles.screen, leaving && styles.leaving)} role="status" aria-live="polite">
      <img className={styles.logo} src={logo} alt="Mesa" draggable={false} />
      <div className={styles.block}>
        <p className={styles.title}>
          {failed ? "Search couldn’t be set up" : step}
        </p>
        {!failed && (
          <div
            className={cx(styles.track, !determinate && styles.indeterminate)}
            role="progressbar"
            aria-label={step}
            aria-valuemin={0}
            aria-valuemax={downloading ? status.total : undefined}
            aria-valuenow={downloading ? status.done : undefined}
            aria-valuetext={describe(status)}
          >
            <div className={styles.fill} style={determinate ? { width: `${status.state === "ready" ? 100 : percent}%` } : undefined} />
          </div>
        )}
        <p className={cx(styles.detail, failed && styles.error)}>{describe(status)}</p>
        {failed && (
          <Button variant="primary" className={styles.retry} onClick={() => api.retryModelSetup().catch(() => {})}>
            Try again
          </Button>
        )}
        {access.folders.length > 0 && (
          <p className={cx(styles.access, access.leaving && styles.accessLeaving)}>
            Mesa needs access to your personal files in order to
            <br />
            index and search them.{" "}
            <button type="button" className={styles.accessLink} onClick={access.allow} disabled={access.asking}>
              Allow access
            </button>
          </p>
        )}
      </div>
      {(downloading || failed) && (
        <p className={styles.note}>The models are downloaded once. Everything else stays on your computer.</p>
      )}
    </div>
  );
}

const ACCESS_OUT_MS = 240;

function useFolderAccessStep(downloading: boolean) {
  const [folders, setFolders] = useState<string[]>([]);
  const [asking, setAsking] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const started = useRef(false);
  useEffect(() => {
    if (!downloading || started.current) return;
    started.current = true;
    api
      .surveyUserFiles(false)
      .then((survey) => setFolders(survey.notAsked))
      .catch(() => {});
  }, [downloading]);
  const allow = () => {
    setAsking(true);
    api
      .surveyUserFiles(true)
      .catch(() => {})
      .finally(() => {
        setAsking(false);
        if (prefersReducedMotion()) {
          setFolders([]);
          return;
        }
        setLeaving(true);
        window.setTimeout(() => {
          setFolders([]);
          setLeaving(false);
        }, ACCESS_OUT_MS);
      });
  };
  return { folders, asking, leaving, allow };
}
