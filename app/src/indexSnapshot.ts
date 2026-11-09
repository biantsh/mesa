import { sameProgress, type Overview, type Progress } from "./api";

export class IndexSnapshot {
  private asked = 0;
  private progressFrom = 0;
  private totalsFrom = 0;
  private totals: Overview | null = null;
  private progress: Progress | null = null;
  private shown: Overview | null = null;

  ask(): number {
    return ++this.asked;
  }

  takeOverview(asking: number, next: Overview): boolean {
    let changed = false;
    if (asking > this.totalsFrom) {
      this.totalsFrom = asking;
      if (!this.totals || !sameTotals(this.totals, next)) {
        this.totals = next;
        changed = true;
      }
    }
    return this.takeProgressFrom(asking, next.progress) || changed ? this.rebuild() : false;
  }

  takeProgress(asking: number, next: Progress | null): boolean {
    return this.takeProgressFrom(asking, next) ? this.rebuild() : false;
  }

  get overview(): Overview | null {
    return this.shown;
  }

  private takeProgressFrom(asking: number, next: Progress | null): boolean {
    if (asking <= this.progressFrom) return false;
    this.progressFrom = asking;
    if (sameProgress(this.progress, next)) return false;
    this.progress = next;
    return true;
  }

  private rebuild(): boolean {
    if (!this.totals) return false;
    this.shown = { ...this.totals, progress: this.progress, empty: this.totals.empty && this.progress === null };
    return true;
  }
}

function sameTotals(a: Overview, b: Overview): boolean {
  return a.empty === b.empty && a.everythingIndexed === b.everythingIndexed && a.files === b.files && a.bytes === b.bytes;
}
