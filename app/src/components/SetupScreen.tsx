import { api, type ModelStatus } from "../api";
import { cx } from "../cx";
import { describeDownload } from "../format";
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
  const percent = downloading && status.total > 0 ? Math.min(100, Math.floor((status.done / status.total) * 100)) : 0;
  const determinate = downloading || status.state === "ready";

  return (
    <div className={cx(styles.screen, leaving && styles.leaving)} role="status" aria-live="polite">
      <img className={styles.logo} src={logo} alt="Mesa" draggable={false} />
      <div className={styles.block}>
        <p className={styles.title}>
          {failed ? "Search couldn’t be set up" : downloading ? "Downloading EmbeddingGemma 2" : "Setting up EmbeddingGemma 2"}
        </p>
        {!failed && (
          <div
            className={cx(styles.track, !determinate && styles.indeterminate)}
            role="progressbar"
            aria-label={downloading ? "Downloading EmbeddingGemma 2" : "Setting up EmbeddingGemma 2"}
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
      </div>
      {(downloading || failed) && (
        <p className={styles.note}>The search model is downloaded once. Everything else stays on your computer.</p>
      )}
    </div>
  );
}
