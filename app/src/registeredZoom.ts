// WebKit applies page zoom (GNOME's text scaling, for one) twice to lengths held in @property-registered custom
// properties. This measures the extra factor so the CSS can divide it back out (--registered-unzoom), and from it the
// size of a device pixel in CSS pixels (--device-unit): page zoom isn't in devicePixelRatio.

let unit = typeof window === "undefined" ? 1 : 1 / (window.devicePixelRatio || 1);
let unzoom: number | null = null;

export const snapToDevice = (value: number) => Math.round(value / unit) * unit;

function measure() {
  const probe = document.createElement("div");
  probe.style.cssText = "position: absolute; visibility: hidden; --zoom-probe: 100px; width: var(--zoom-probe);";
  document.body.append(probe);
  const width = probe.getBoundingClientRect().width;
  probe.remove();
  const nextUnzoom = width > 0 ? 100 / width : 1;
  const nextUnit = nextUnzoom / (window.devicePixelRatio || 1);
  // Setting them restyles the whole page, and this runs on every resize event.
  if (nextUnzoom === unzoom && nextUnit === unit) return;
  unzoom = nextUnzoom;
  unit = nextUnit;
  document.documentElement.style.setProperty("--registered-unzoom", String(unzoom));
  document.documentElement.style.setProperty("--device-unit", `${unit}px`);
}

if (typeof document !== "undefined" && document.body && typeof CSS !== "undefined" && "registerProperty" in CSS) {
  try {
    CSS.registerProperty({ name: "--zoom-probe", syntax: "<length>", inherits: false, initialValue: "0px" });
  } catch {}
  measure();
  window.addEventListener("resize", measure);
}
