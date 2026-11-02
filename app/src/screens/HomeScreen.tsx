import { Database, Search, Settings } from "lucide-react";
import { Button } from "../components/Button";
import logo from "../logo";
import styles from "./HomeScreen.module.css";

interface Props {
  onOpenIndex: () => void;
  onOpenSettings: () => void;
}

export function HomeScreen({ onOpenIndex, onOpenSettings }: Props) {
  return (
    <main className={styles.home}>
      <img className={styles.logo} src={logo} alt="Mesa" draggable={false} />
      <div className={styles.row}>
        <label className={styles.field}>
          <Search className={styles.searchIcon} size={18} aria-hidden />
          <input
            className={styles.input}
            type="text"
            placeholder="Describe what you're looking for…"
            aria-label="Search"
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        <Button variant="primary" className={styles.indexButton} onClick={onOpenIndex}>
          <Database size={16} aria-hidden />
          Index
        </Button>
        <Button
          variant="neutral"
          className={styles.iconOnly}
          onClick={onOpenSettings}
          title="Settings"
          aria-label="Settings"
        >
          <Settings size={18} aria-hidden />
        </Button>
      </div>
    </main>
  );
}
