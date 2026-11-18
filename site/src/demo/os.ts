const NAMES = { mac: "MacIntel", windows: "Win32", linux: "Linux x86_64" } as const;

try {
  const os = window.parent.document.documentElement.dataset.os as keyof typeof NAMES | undefined;
  if (os && os in NAMES) Object.defineProperty(navigator, "platform", { value: NAMES[os], configurable: true });
} catch {
}
