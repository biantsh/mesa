import { TitleBar } from "../../../app/src/components/TitleBar";
import { LegalCorner } from "../../../app/src/components/LegalCorner";
import { describeIndexTotals } from "../../../app/src/format";
import appStyles from "../../../app/src/App.module.css";
import { TOTALS } from "./backend";

export default function DemoShell() {
  return (["mac", "other"] as const).map((system) => (
    <div key={system} className={appStyles.window} data-shell={system}>
      <div className={appStyles.titleArea}>
        <TitleBar maximized={false} mac={system === "mac"} version={__MESA_VERSION__} />
      </div>
      <div className={appStyles.screens}>
        <footer className={appStyles.totals}>{describeIndexTotals(TOTALS.files, TOTALS.bytes)}</footer>
        <footer className={appStyles.legalCorner}>
          <LegalCorner onOpenLicenses={() => {}} />
        </footer>
      </div>
    </div>
  ));
}
