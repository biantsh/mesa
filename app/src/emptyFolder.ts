export function showsEmptyFolder(entryCount: number, loading: boolean, openingAnother: boolean): boolean {
  return entryCount === 0 && !(loading && openingAnother);
}
