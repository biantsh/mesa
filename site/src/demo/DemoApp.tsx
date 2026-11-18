import "./os";
import "./paged";
import { dragFiles } from "./tauri/webview";
import { useEffect, useState } from "react";
import App from "../../../app/src/App";
import { isMac, markPlatform } from "../../../app/src/platform";
import { ready } from "../../../app/src/ready";
import { guardAgainstStaleHover } from "../../../app/src/staleHover";
import { installTooltips } from "../../../app/src/tooltip";
import { followTheme } from "../../../app/src/theme";
import { setTipsAreOnceForTests } from "../../../app/src/tipsOnce";
import "@fontsource-variable/inter";
import "../../../app/src/styles/tokens.css";
import "../../../app/src/styles/global.css";
import appStyles from "../../../app/src/App.module.css";

setTipsAreOnceForTests(true);
const KEPT: [string, string][] = [
  ["mesa.shortcutTipSeen", "1"],
  ["mesa.speedTipSeen", "1"],
  ["mesa.backgroundHintSeen", "1"],
  ["mesa.resultPanelColumns", "3"],
];
for (const [key, value] of KEPT) {
  try {
    localStorage.setItem(key, value);
  } catch {
  }
}

markPlatform();
guardAgainstStaleHover();
installTooltips();

for (const name of ["setPointerCapture", "releasePointerCapture"] as const) {
  const real = Element.prototype[name];
  Element.prototype[name] = function (this: Element, id: number) {
    try {
      real.call(this, id);
    } catch {
    }
  };
}

if (window.parent !== window) {
  try {
    const site = window.parent.document.documentElement;
    const follow = () => followTheme(site.dataset.theme === "dark" ? "dark" : "light");
    follow();
    new MutationObserver(follow).observe(site, { attributes: true, attributeFilter: ["data-theme"] });
  } catch {
  }
}

let run = 0;
class Stopped extends Error {}

let held = false;
const letGo: (() => void)[] = [];
function release() {
  letGo.splice(0).forEach((go) => go());
}
const whileHeld = () => new Promise<void>((go) => letGo.push(go));
let resuming = false;

async function delay(ms: number, mine: number) {
  let left = ms;
  while (left > 0) {
    if (mine !== run) throw new Stopped();
    if (held) {
      await whileHeld();
      continue;
    }
    const step = Math.min(left, 50);
    await new Promise((done) => setTimeout(done, step));
    if (!held) left -= step;
  }
  if (mine !== run) throw new Stopped();
}

const wait = (ms: number) => delay(ms, run);

async function field(): Promise<HTMLInputElement> {
  for (;;) {
    const input = [...document.querySelectorAll<HTMLInputElement>("input[type='text'], input:not([type])")].find((each) => !each.closest("dialog"));
    if (input) return input;
    await wait(100);
  }
}

function setText(input: HTMLInputElement, text: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, text);
  input.dispatchEvent(new Event("input", { bubbles: true }));
  caret.typed();
}

const framed = window.parent !== window;
const reallyInFront = document.hasFocus.bind(document);
let demoMoving = false;
if (framed) document.hasFocus = () => demoMoving || reallyInFront();

function said(input: HTMLInputElement, move: () => void, event: "focus" | "blur") {
  const bubbling = event === "focus" ? "focusin" : "focusout";
  let given = false;
  const heard = () => (given = true);
  demoMoving = true;
  try {
    input.addEventListener(bubbling, heard, { once: true });
    move();
    input.removeEventListener(bubbling, heard);
    if (!given) input.dispatchEvent(new FocusEvent(bubbling, { bubbles: true }));
  } finally {
    demoMoving = false;
  }
}

function focusField(input: HTMLInputElement) {
  said(input, () => input.focus({ preventScroll: true }), "focus");
  caret.show(input);
}

function leaveField(input: HTMLInputElement) {
  caret.hide();
  said(input, () => input.blur(), "blur");
}

const caret = (() => {
  let element: HTMLElement | null = null;
  let input: HTMLInputElement | null = null;
  let frame = 0;
  let typedAt = 0;
  const measure = document.createElement("canvas").getContext("2d")!;
  const place = () => {
    if (!element || !input) return;
    const style = getComputedStyle(input);
    measure.font = style.font;
    measure.letterSpacing = style.letterSpacing === "normal" ? "0px" : style.letterSpacing;
    const box = input.getBoundingClientRect();
    const height = parseFloat(style.fontSize) * 1.25;
    const x = box.left + parseFloat(style.borderLeftWidth) + parseFloat(style.paddingLeft) + measure.measureText(input.value).width - input.scrollLeft;
    element.style.transform = `translate(${x}px, ${box.top + (box.height - height) / 2}px)`;
    element.style.height = `${height}px`;
    element.style.background = style.caretColor === "auto" ? style.color : style.caretColor;
    const since = performance.now() - typedAt;
    const browsers = reallyInFront() && document.activeElement === input;
    element.style.opacity = !held && !browsers && (since < 500 || Math.floor((since - 500) / 530) % 2 === 1) ? "1" : "0";
    frame = requestAnimationFrame(place);
  };
  return {
    show(field: HTMLInputElement) {
      input = field;
      typedAt = performance.now();
      element ??= Object.assign(document.body.appendChild(document.createElement("div")), {
        style: "position: fixed; top: 0; left: 0; width: 1.5px; pointer-events: none; z-index: 2147483647",
      });
      element.hidden = false;
      cancelAnimationFrame(frame);
      place();
    },
    typed() {
      typedAt = performance.now();
    },
    hide() {
      cancelAnimationFrame(frame);
      input = null;
      if (element) element.hidden = true;
    },
  };
})();

let onScreen = false;
const shownAgain: (() => void)[] = [];

function watchOnScreen() {
  let inView = false;
  const update = () => {
    onScreen = inView && document.visibilityState === "visible";
    if (onScreen) shownAgain.splice(0).forEach((resume) => resume());
  };
  new IntersectionObserver(([entry]) => {
    inView = entry.isIntersecting;
    update();
  }, { threshold: 0.5 }).observe(document.documentElement);
  document.addEventListener("visibilitychange", update);
}

function seen(): Promise<void> {
  return onScreen ? Promise.resolve() : new Promise((resume) => shownAgain.push(resume));
}

const hovers = (() => {
  const mirror = document.createElement("style");
  let made = -1;
  const copy = (rules: CSSRuleList): string => {
    let text = "";
    for (const rule of Array.from(rules)) {
      if (rule instanceof CSSStyleRule && rule.selectorText.includes(":hover")) {
        text += `${rule.selectorText.replaceAll(":hover", "[data-demo-hover]")} { ${rule.style.cssText} }\n`;
      } else if (rule instanceof CSSMediaRule || rule instanceof CSSSupportsRule) {
        const inner = copy(rule.cssRules);
        if (inner) text += `@${rule instanceof CSSMediaRule ? "media" : "supports"} ${rule.conditionText} {\n${inner}}\n`;
      }
    }
    return text;
  };
  return {
    keep() {
      if (document.styleSheets.length === made) return;
      const sheets = Array.from(document.styleSheets).filter((sheet) => sheet.ownerNode !== mirror);
      let text = "";
      for (const sheet of sheets) {
        try {
          text += copy(sheet.cssRules);
        } catch {
        }
      }
      mirror.textContent = text;
      if (!mirror.isConnected) document.head.appendChild(mirror);
      made = document.styleSheets.length;
    },
  };
})();

const pointer = (() => {
  const windows = !isMac && !/Linux/i.test(navigator.platform);
  const element = document.createElement("div");
  element.setAttribute("aria-hidden", "true");
  element.popover = "manual";
  element.style.cssText =
    "position: fixed; inset: 0 auto auto 0; margin: 0; padding: 0; border: 0; background: none; overflow: visible; " +
    "pointer-events: none; z-index: 2147483647; " +
    "transition: opacity 160ms ease; filter: drop-shadow(0 1px 1px rgba(0, 0, 0, 0.3));";
  let over: Element | null = null;
  const raise = () => {
    const cards = document.querySelectorAll("dialog[open]");
    const last = cards.length ? cards[cards.length - 1] : null;
    if (last === over || !element.isConnected) return;
    over = last;
    if (!last) return;
    try {
      element.hidePopover();
      element.showPopover();
    } catch {
    }
  };
  const fill = windows ? "#fff" : "#000";
  const edge = windows ? "#000" : "#fff";
  const svg = (width: number, height: number, box: string, body: string) =>
    `<svg width="${width}" height="${height}" viewBox="${box}" style="display:block">${body}</svg>`;
  const shaped = (d: string, edgeWidth = 1.4) => `<path d="${d}" fill="${fill}" stroke="${edge}" stroke-width="${edgeWidth}" stroke-linejoin="round"/>`;
  const RESIZE = "M11.4 1.5 H14.6 V19.5 H11.4 Z M9.6 10.5 L4.6 5.6 V8.8 H1.5 V12.2 H4.6 V15.4 Z M16.4 10.5 L21.4 5.6 V8.8 H24.5 V12.2 H21.4 V15.4 Z";
  const lens = (sign: "+" | "-") =>
    `<circle cx="8.5" cy="8.5" r="6" fill="none" stroke="${edge}" stroke-width="4.2"/>` +
    `<path d="M13 13 L19.5 19.5" stroke="${edge}" stroke-width="5" stroke-linecap="round"/>` +
    `<circle cx="8.5" cy="8.5" r="6" fill="${edge}" stroke="${fill}" stroke-width="1.8"/>` +
    `<path d="M13 13 L19.5 19.5" stroke="${fill}" stroke-width="2.4" stroke-linecap="round"/>` +
    `<path d="M5.6 8.5 H11.4${sign === "+" ? " M8.5 5.6 V11.4" : ""}" stroke="${fill}" stroke-width="1.6" stroke-linecap="round"/>`;
  const SHAPES = {
    arrow: svg(13.5, 19.5, "0 0 18 26", shaped("M1.5 1.5 L1.5 20 L6 15.8 L9.2 23.2 L12.3 21.9 L9.2 14.7 L15.2 14.7 Z", 1.6)),
    text: svg(9, 18, "0 0 11 22", shaped("M1.5 1.5 H4.5 Q5.5 1.5 5.5 2.5 Q5.5 1.5 6.5 1.5 H9.5 V3.6 H7 V18.4 H9.5 V20.5 H6.5 Q5.5 20.5 5.5 19.5 Q5.5 20.5 4.5 20.5 H1.5 V18.4 H4 V3.6 H1.5 Z", 1.2)),
    hand: svg(16, 19, "0 0 21 25", shaped(
      "M7.6 1.5 C6.6 1.5 5.9 2.3 5.9 3.2 V13 L4.6 11.6 C3.9 10.9 2.8 10.9 2.1 11.6 C1.5 12.2 1.4 13.1 1.9 13.8 L5.9 19.6 " +
      "C7.1 21.5 9 23.5 11.6 23.5 H13.5 C16.9 23.5 19.5 20.9 19.5 17.5 V11.8 C19.5 10.9 18.8 10.2 17.9 10.2 C17.4 10.2 17 10.4 16.7 10.8 " +
      "C16.6 9.9 15.9 9.2 15 9.2 C14.4 9.2 13.9 9.5 13.6 10 C13.4 9.2 12.7 8.6 11.9 8.6 C11.3 8.6 10.8 8.9 10.4 9.3 Z " +
      "M5.9 3.2 C5.9 2.3 6.6 1.5 7.6 1.5 C8.6 1.5 9.3 2.3 9.3 3.2 V9.5 C9.6 9 10.2 8.6 10.9 8.6")),
    resize: svg(21, 17, "0 0 26 21", shaped(RESIZE)),
    resizeUpDown: svg(17, 21, "0 0 21 26", `<g transform="rotate(90 10.5 13) translate(-2.5 2.5)">${shaped(RESIZE)}</g>`),
    zoomIn: svg(17, 17, "0 0 21 21", lens("+")),
    zoomOut: svg(17, 17, "0 0 21 21", lens("-")),
  };
  const POINT = { arrow: [1.1, 1.1], text: [4.5, 9], hand: [5.8, 1.2], resize: [10.5, 8.5], resizeUpDown: [8.5, 10.5], zoomIn: [6.9, 6.9], zoomOut: [6.9, 6.9] } as const;

  const shapeAt = (under: Element | null): keyof typeof POINT => {
    if (!under) return "arrow";
    const style = getComputedStyle(under);
    const cursor = style.cursor.split(",").pop()!.trim();
    switch (cursor) {
      case "pointer":
      case "grab":
      case "grabbing":
        return "hand";
      case "text":
      case "vertical-text":
        return "text";
      case "col-resize":
      case "ew-resize":
      case "e-resize":
      case "w-resize":
        return "resize";
      case "row-resize":
      case "ns-resize":
      case "n-resize":
      case "s-resize":
        return "resizeUpDown";
      case "zoom-in":
        return "zoomIn";
      case "zoom-out":
        return "zoomOut";
      case "auto": {
        const field = under.closest("input, textarea, [contenteditable='true'], [contenteditable='']");
        if (field && !(field instanceof HTMLInputElement && ["button", "submit", "checkbox", "radio", "range", "color", "file"].includes(field.type)))
          return "text";
        if (under.closest("a[href]")) return "hand";
        const ownText = Array.from(under.childNodes).some((node) => node.nodeType === Node.TEXT_NODE && node.textContent?.trim());
        return ownText && style.userSelect !== "none" ? "text" : "arrow";
      }
      default:
        return "arrow";
    }
  };
  let shape: keyof typeof POINT = "arrow";
  const shapeBox = element.appendChild(document.createElement("div"));
  shapeBox.innerHTML = SHAPES.arrow;
  const carried = element.appendChild(document.createElement("div"));
  carried.style.cssText =
    "position: absolute; display: none; flex-direction: column; align-items: center; gap: 4px; opacity: 0.82; " +
    "font: 500 11px/1.3 system-ui, -apple-system, 'Segoe UI', sans-serif; color: #fff; white-space: nowrap;";
  let x = 0;
  let y = 0;
  let hold: (holding: boolean) => void = () => {};
  let away = false;
  let pressed: (() => void) | null = null;
  const place = () => (element.style.transform = `translate(${x - POINT[shape][0]}px, ${y - POINT[shape][1]}px)`);
  const reach = (time: number) => {
    const t = Math.pow(time, 0.82);
    return t * t * t * (10 - 15 * t + 6 * t * t);
  };
  const random = (from: number, to: number) => from + Math.random() * (to - from);

  const stroke = (toX: number, toY: number, ms: number, bow: number, each?: (x: number, y: number) => void) => {
    const mine = run;
    const own = resuming;
    const fromX = x;
    const fromY = y;
    const length = Math.hypot(toX - fromX, toY - fromY) || 1;
    const sideX = (-(toY - fromY) / length) * length * bow;
    const sideY = ((toX - fromX) / length) * length * bow;
    let begun = performance.now();
    let last = begun;
    return new Promise<void>((done, fail) => {
      const step = (now: number) => {
        if (mine !== run) return fail(new Stopped());
        if (held || (resuming && !own)) {
          begun += now - last;
          last = now;
          return void requestAnimationFrame(step);
        }
        last = now;
        const t = Math.min(1, Math.max(0, (now - begun) / ms));
        const k = reach(t);
        const arc = 4 * k * (1 - k);
        x = fromX + (toX - fromX) * k + sideX * arc;
        y = fromY + (toY - fromY) * k + sideY * arc;
        place();
        each?.(x, y);
        if (t < 1) requestAnimationFrame(step);
        else done();
      };
      requestAnimationFrame(step);
    });
  };

  return {
    start() {
      document.body.appendChild(element);
      try {
        element.showPopover();
      } catch {
      }
      hold = this.follow();
      this.home();
    },
    home() {
      x = innerWidth * 0.74;
      y = innerHeight * 0.8;
      place();
    },
    async moveTo(toX: number, toY: number, correct = false, each?: (x: number, y: number) => void) {
      if (!away) element.style.opacity = "1";
      const dx = toX - x;
      const dy = toY - y;
      const distance = Math.hypot(dx, dy);
      if (distance < 2) return;
      const ms = (150 + 80 * Math.log2(1 + distance / 14)) * random(0.92, 1.08);
      const bow = random(0.025, 0.05) * (Math.random() < 0.5 ? -1 : 1);
      if (!correct) return stroke(toX, toY, ms, bow, each);
      const short = random(0.03, 0.06);
      const aside = random(-0.015, 0.015) * distance;
      await stroke(toX - dx * short - (dy / distance) * aside, toY - dy * short + (dx / distance) * aside, ms, bow, each);
      await wait(random(25, 60));
      await stroke(toX, toY, random(110, 160), 0, each);
    },
    carry(file: { picture: string; name: string } | null) {
      hold(file !== null);
      if (file === null) {
        carried.style.display = "none";
        carried.replaceChildren();
        return;
      }
      const picture = document.createElement("img");
      picture.src = file.picture;
      picture.alt = "";
      picture.style.cssText = "display: block; width: 104px; border-radius: 4px; box-shadow: 0 2px 8px rgba(0, 0, 0, 0.35);";
      const name = document.createElement("span");
      name.textContent = file.name;
      name.style.cssText = "padding: 1px 6px; border-radius: 4px; background: rgba(32, 32, 32, 0.85);";
      carried.replaceChildren(picture, name);
      carried.style.left = `${POINT[shape][0] + 14}px`;
      carried.style.top = `${POINT[shape][1] + 18}px`;
      carried.style.display = "flex";
    },
    async moveOnto(target: Element, across = 0.42, down = 0.55, correct = false) {
      const box = target.getBoundingClientRect();
      await this.moveTo(box.left + box.width * across, box.top + box.height * down, correct);
    },
    async drag(target: Element, dx: number) {
      const event = (type: string, atX: number, atY: number, buttons: number) =>
        target.dispatchEvent(
          new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 1, pointerType: "mouse", isPrimary: true, button: 0, buttons, clientX: atX, clientY: atY }),
        );
      hold(true);
      event("pointerdown", x, y, 1);
      pressed = () => event("pointerup", x, y, 0);
      try {
        await wait(random(90, 150));
        await stroke(x + dx, y, (220 + 60 * Math.log2(1 + Math.abs(dx) / 14)) * random(0.95, 1.05), 0, (atX, atY) => event("pointermove", atX, atY, 1));
        await wait(random(60, 110));
      } finally {
        this.letGo();
      }
    },
    get pressing() {
      return pressed !== null;
    },
    letGo() {
      const up = pressed;
      pressed = null;
      up?.();
      hold(false);
    },
    async press() {
      await wait(random(90, 200));
    },
    away() {
      away = true;
      element.style.transition = "opacity 160ms ease";
      element.style.opacity = "0";
    },
    back() {
      away = false;
      element.style.transition = "opacity 320ms ease";
      requestAnimationFrame(() => requestAnimationFrame(() => (element.style.opacity = "1")));
    },
    follow() {
      let held = false;
      let hovered: Element[] = [];
      const look = () => {
        hovers.keep();
        raise();
        const top = away ? null : document.elementFromPoint(x, y);
        const now: Element[] = [];
        for (let one: Element | null = top; one; one = one.parentElement) now.push(one);
        for (const one of hovered) if (!now.includes(one)) one.removeAttribute("data-demo-hover");
        for (const one of now) if (!hovered.includes(one)) one.setAttribute("data-demo-hover", "");
        hovered = now;
        if (!held) {
          const next = shapeAt(top);
          if (next !== shape) {
            shape = next;
            shapeBox.innerHTML = SHAPES[next];
            place();
          }
        }
        if (onScreen) requestAnimationFrame(look);
        else void seen().then(() => requestAnimationFrame(look));
      };
      requestAnimationFrame(look);
      return (holding: boolean) => (held = holding);
    },
  };
})();

async function searchByItself(pause = 2000) {
  try {
    await doing(pause);
  } catch (error) {
    if (!(error instanceof Stopped)) throw error;
  }
}

const SCENARIOS = {
  photo: { query: "photo of my dog at the beach in 2024", looking: 1860, then: openResizeAndFindSimilar },
  slide: { query: "subscription churn", looking: 900, then: openSlidesAndTurn },
  chart: { query: "", looking: 0, then: dropChartAndOpenCode },
};
type Scenario = keyof typeof SCENARIOS;
const ORDER = Object.keys(SCENARIOS) as Scenario[];
let scenario = 0;
let untyped = -1;
function playing() {
  if (framed) window.parent.postMessage({ type: "mesa-demo-playing", scenario: ORDER[scenario] }, location.origin);
}

async function doing(pause: number) {
  const input = await field();
  for (let first = true; ; first = false) {
    playing();
    untyped = run;
    await seen();
    if (first) await wait(pause);
    await seen();
    if (SCENARIOS[ORDER[scenario]].query) {
      await pointer.moveOnto(input, 0.3);
      await pointer.press();
      focusField(input);
      await wait(600);
    }
    untyped = -1;
    const { query, looking, then } = SCENARIOS[ORDER[scenario]];
    for (let i = 1; i <= query.length; i++) {
      setText(input, query.slice(0, i));
      await wait(73 + Math.random() * 60);
    }
    await wait(looking);
    await then(input);
    scenario = (scenario + 1) % ORDER.length;
  }
}

async function openResizeAndFindSimilar(input: HTMLInputElement) {
  const first = document.querySelector<HTMLElement>('[data-result="0"]');
  const second = document.querySelector<HTMLElement>('[data-result="1"]');
  if (!first) return;
  const column = second ? second.getBoundingClientRect().left - first.getBoundingClientRect().left : 160;
  await pointer.moveOnto(first, 0.5, 0.4, true);
  await pointer.press();
  leaveField(input);
  first.click();
  await wait(2390);
  const handle = document.querySelector<HTMLElement>('[role="separator"][aria-label="Width of the panel"]');
  if (handle) {
    await pointer.moveOnto(handle, 0.5, 0.42);
    await pointer.press();
    const wider = Number(handle.getAttribute("aria-valuenow")) < 4;
    await pointer.drag(handle, (wider ? -1 : 1) * column * (0.66 + Math.random() * 0.1));
    await wait(930);
    await pointer.moveTo(handle.getBoundingClientRect().left + 70, handle.getBoundingClientRect().top + handle.getBoundingClientRect().height * 0.42 + 6);
    await wait(670);
  }
  const similar = [...document.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent?.trim() === "Find similar");
  if (similar) {
    await pointer.moveOnto(similar, 0.4, 0.55);
    await pointer.press();
    similar.click();
    await wait(3500);
  }
  await clearSearch();
}

async function openSlidesAndTurn(input: HTMLInputElement) {
  const first = document.querySelector<HTMLElement>('[data-result="0"]');
  if (!first) return;
  await pointer.moveOnto(first, 0.3, 0.68);
  await pointer.press();
  leaveField(input);
  first.click();
  await wait(1500);
  const picture = await until(() => document.querySelector<HTMLElement>('[aria-label="Show larger"]'));
  if (picture) {
    await pointer.moveOnto(picture, 0.5, 0.5);
    await pointer.press();
    picture.click();
    await wait(1400);
    const next = await until(() => document.querySelector<HTMLButtonElement>('[aria-label="Next slide"]'));
    const back = document.querySelector<HTMLButtonElement>('[aria-label="Previous slide"]');
    if (next && back) {
      await pointer.moveOnto(next, 0.5, 0.5);
      for (let i = 0; i < 3; i++) {
        await pointer.press();
        next.click();
        await wait(i < 2 ? 900 : 1400);
      }
      await pointer.moveOnto(back, 0.5, 0.5);
      for (let i = 0; i < 3; i++) {
        await (i === 0 ? pointer.press() : wait(50 + Math.random() * 40));
        back.click();
        await wait(i < 2 ? 280 : 650);
      }
    }
    await closeViewer();
    await wait(900);
  }
  await clearSearch();
}

const CHART = { path: "/Users/demo/Desktop/confusion_matrix.png", picture: "/demo/files/charts/confusion-matrix.svg", name: "confusion_matrix.png" };

async function dropChartAndOpenCode(input: HTMLInputElement) {
  await pointer.moveTo(innerWidth + 40, innerHeight * 0.62);
  await wait(450);
  pointer.carry(CHART);
  let inside = false;
  let dropped = false;
  try {
    const box = input.getBoundingClientRect();
    const onField = { x: box.left + box.width * 0.42, y: box.top + box.height * 0.55 };
    await pointer.moveTo(onField.x, onField.y, false, (x, y) => {
      if (inside) dragFiles.over({ x, y });
      else if (x < innerWidth) {
        inside = true;
        dragFiles.enter([CHART.path], { x, y });
      }
    });
    await wait(420);
    dragFiles.drop([CHART.path], onField);
    dropped = true;
  } finally {
    pointer.carry(null);
    if (inside && !dropped) dragFiles.leave();
  }
  const first = await until(() => document.querySelector<HTMLElement>('[data-result="0"]'));
  if (!first) return;
  await wait(1300);
  await pointer.moveOnto(first, 0.36, 0.6);
  await pointer.press();
  first.click();
  await wait(1800);
  const whole = await until(() => document.querySelector<HTMLElement>('[aria-label="Show the whole file"]'));
  if (whole) {
    await pointer.moveOnto(whole, 0.5, 0.35);
    await pointer.press();
    whole.click();
    await wait(2600);
    await closeViewer();
    await wait(900);
  }
  await clearSearch();
}

async function until<T>(find: () => T | null | undefined, ms = 3000): Promise<T | null> {
  for (let waited = 0; waited < ms; waited += 100) {
    const found = find();
    if (found) return found;
    await wait(100);
  }
  return find() ?? null;
}

async function closeViewer() {
  const close = await until(() => document.querySelector("dialog[open]")?.querySelector<HTMLElement>('[aria-label="Close"]'));
  if (close) {
    await pointer.moveOnto(close, 0.5, 0.5);
    await pointer.press();
    close.click();
  } else {
    closeDialogs();
  }
}

function closeDialogs() {
  for (const open of document.querySelectorAll("dialog[open]")) open.dispatchEvent(new Event("cancel", { cancelable: true }));
}

async function clearSearch() {
  const clear = document.querySelector<HTMLElement>('[aria-label="Clear search"]');
  if (clear) {
    await pointer.moveOnto(clear, 0.5, 0.5);
    await pointer.press();
    clear.click();
  } else {
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  }
  await wait(700);
}

const visitor = (() => {
  const STOP_CURSOR = `url("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20' viewBox='0 0 20 20'><circle cx='10' cy='10' r='7.6' fill='none' stroke='%23fff' stroke-width='4.4'/><path d='M4.6 15.4 L15.4 4.6' stroke='%23fff' stroke-width='4.4' stroke-linecap='round'/><circle cx='10' cy='10' r='7.6' fill='none' stroke='%23d6402f' stroke-width='2.2'/><path d='M4.6 15.4 L15.4 4.6' stroke='%23d6402f' stroke-width='2.2' stroke-linecap='round'/></svg>") 10 10, not-allowed`;
  const BACK_AFTER_MS = 2000;
  let here = false;
  let back = 0;
  let input: HTMLInputElement | null = null;
  let refusing: string[] = [];
  let marked: Element | null = null;

  const moduleClass = (name: string, beside: string): string | null => {
    const pattern = new RegExp(`\\.(_${name}_[\\w-]+)`);
    const look = (rules: CSSRuleList): string | null => {
      for (const rule of Array.from(rules)) {
        if (rule instanceof CSSStyleRule) {
          if (rule.selectorText.includes(`.${beside}.`)) {
            const found = pattern.exec(rule.selectorText);
            if (found) return found[1];
          }
        } else if ("cssRules" in rule) {
          const inner = look((rule as CSSGroupingRule).cssRules);
          if (inner) return inner;
        }
      }
      return null;
    };
    for (const sheet of Array.from(document.styleSheets)) {
      try {
        const found = look(sheet.cssRules);
        if (found) return found;
      } catch {
      }
    }
    return null;
  };

  const row = () => input?.closest<HTMLElement>('[class*="_row_"]') ?? null;
  const fieldBox = () => input?.closest<HTMLElement>('[class*="_field_"]') ?? input;

  const refusedAt = (target: EventTarget | null): Element | null => {
    if (!(target instanceof Element)) return null;
    const box = fieldBox();
    if (box?.contains(target) && !target.closest('[aria-label="Clear search"]')) return box;
    const control = target.closest("button, a, [role='button'], [role='menuitem']");
    if (!control) return null;
    const label = control.getAttribute("aria-label") ?? "";
    const words = (control.textContent ?? "").trim();
    const refused =
      label === "Settings" ||
      (control.closest("header") !== null && ["Minimize", "Maximize", "Restore", "Close"].includes(label)) ||
      ["Index", "Open", "Copy file", "Copy path", "Copy files", "Copy paths", "MIT License"].includes(words) ||
      words.startsWith("Show in ");
    return refused ? control : null;
  };

  const mark = (target: EventTarget | null) => {
    const refused = refusedAt(target);
    if (refused !== marked) {
      marked?.removeAttribute("data-demo-refused");
      refused?.setAttribute("data-demo-refused", "");
      marked = refused;
    }
    const onField = refused !== null && refused === fieldBox();
    if (!refusing.length) {
      const rowClass = /(_row_[\w-]+)/.exec(row()?.className ?? "")?.[1];
      if (rowClass) refusing = [moduleClass("refusing", rowClass), moduleClass("hot", rowClass)].filter((name): name is string => name !== null);
    }
    for (const name of refusing) row()?.classList.toggle(name, onField);
  };

  let changed = false;
  let unfocused = false;
  let keptFocus = false;

  const state = () => {
    const tile = document.querySelector("[data-result]");
    let results = tile?.parentElement ?? null;
    while (results && !/(auto|scroll)/.test(getComputedStyle(results).overflowY)) results = results.parentElement;
    return JSON.stringify([
      input?.value,
      document.querySelector('[class*="_examples_"]')?.textContent ?? "",
      Array.from(document.querySelectorAll('[class*="_tagsBox_"] button'), (pill) => pill.textContent?.trim()),
      Array.from(document.querySelectorAll('[data-result][class*="_selected_"]'), (one) => (one as HTMLElement).dataset.result),
      document.querySelector('[role="separator"][aria-label="Width of the panel"]')?.getAttribute("aria-valuenow"),
      Math.round(results?.scrollTop ?? 0),
      document.querySelector("dialog[open], [role='menu']") !== null,
      document.querySelector<HTMLInputElement>("dialog[open] input")?.value ?? null,
    ]);
  };
  let before = "";

  const comeIn = () => {
    clearTimeout(back);
    if (here) return;
    if (resuming) changed = true;
    here = true;
    held = true;
    before = state();
    pointer.away();
    if (pointer.pressing) {
      pointer.letGo();
      change();
    }
  };

  const change = () => {
    comeIn();
    changed = true;
  };

  const goOut = () => {
    intent.cancel();
    if (!here) return;
    mark(null);
    clearTimeout(back);
    back = window.setTimeout(goOn, BACK_AFTER_MS);
  };

  const intent = (() => {
    const REST_MS = 480;
    const AIMED_MS = 240;
    const REST_SPEED = 0.12;
    const SCROLLING_MS = 450;
    const MOVED_PX = 4;
    let settle = 0;
    let slowSince: number | null = null;
    let scrolledAt = -Infinity;
    let travelled = 0;
    let last: { x: number; y: number } | null = null;
    const moves: { t: number; x: number; y: number }[] = [];
    const answers = (target: EventTarget | null) =>
      target instanceof Element &&
      target.closest("button, a, input, [role='button'], [role='separator'], [data-result], [tabindex]:not([tabindex='-1'])") !== null;
    const cancel = () => {
      clearTimeout(settle);
      slowSince = null;
      travelled = 0;
      last = null;
      moves.length = 0;
    };
    for (const type of ["focus", "blur"]) window.addEventListener(type, cancel);
    document.addEventListener("visibilitychange", cancel);
    return {
      moved(event: PointerEvent) {
        if (here) return;
        const now = performance.now();
        if (now - scrolledAt < SCROLLING_MS) return cancel();
        travelled += last ? Math.hypot(event.screenX - last.x, event.screenY - last.y) : Math.hypot(event.movementX, event.movementY);
        last = { x: event.screenX, y: event.screenY };
        if (travelled < MOVED_PX) return;
        moves.push({ t: now, x: event.clientX, y: event.clientY });
        while (moves.length > 2 && now - moves[0].t > 100) moves.shift();
        const from = moves[0];
        const speed = now > from.t ? Math.hypot(event.clientX - from.x, event.clientY - from.y) / (now - from.t) : 0;
        const wait = answers(event.target) ? AIMED_MS : REST_MS;
        if (speed > REST_SPEED) slowSince = null;
        else if ((slowSince ??= now) <= now - wait) return comeIn();
        clearTimeout(settle);
        settle = window.setTimeout(comeIn, wait);
      },
      scrolled() {
        scrolledAt = performance.now();
        cancel();
      },
      cancel,
    };
  })();

  const goOn = async () => {
    here = false;
    held = false;
    if (!changed && state() === before) {
      pointer.back();
      const lost = unfocused && input !== null && document.activeElement !== input;
      unfocused = false;
      if (lost && !(await refocus(input!))) return;
      release();
      return;
    }
    startOver();
  };

  const startOver = (pause?: number) => {
    changed = false;
    unfocused = false;
    run++;
    release();
    if (pointer.pressing) pointer.letGo();
    caret.hide();
    const focused = document.activeElement as HTMLElement | null;
    if (input && (focused === input || keptFocus)) leaveField(input);
    else if (focused && focused !== document.body) {
      demoMoving = true;
      try {
        focused.blur();
      } finally {
        demoMoving = false;
      }
    }
    keptFocus = false;
    closeDialogs();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    document.querySelector<HTMLElement>('[aria-label="Clear search"]')?.click();
    pointer.home();
    pointer.back();
    void searchByItself(pause);
  };

  const refocus = async (field: HTMLInputElement) => {
    resuming = true;
    try {
      await pointer.moveOnto(field, 0.3);
      await pointer.press();
      focusField(field);
      return true;
    } catch (error) {
      if (error instanceof Stopped) return false;
      throw error;
    } finally {
      resuming = false;
    }
  };

  return {
    again() {
      clearTimeout(back);
      here = false;
      held = false;
      mark(null);
      startOver(600);
    },
    goOnNow() {
      if (!here) return true;
      if (changed || unfocused || state() !== before) return false;
      clearTimeout(back);
      here = false;
      held = false;
      mark(null);
      pointer.back();
      release();
      return true;
    },
    start(field: HTMLInputElement) {
      input = field;
      const style = document.head.appendChild(document.createElement("style"));
      style.textContent = `[data-demo-refused], [data-demo-refused] * { cursor: ${STOP_CURSOR} !important; }`;
      const theirs = (event: Event) => event.isTrusted;
      window.addEventListener("pointermove", (event) => theirs(event) && intent.moved(event), { capture: true, passive: true });
      let pressedAt = -Infinity;
      for (const type of ["pointerdown", "keydown"]) {
        window.addEventListener(
          type,
          (event) => {
            if (!theirs(event)) return;
            comeIn();
            pressedAt = performance.now();
          },
          { capture: true, passive: true },
        );
      }
      window.addEventListener(
        "wheel",
        (event) => {
          if (!theirs(event)) return;
          let inner: HTMLElement | null = null;
          for (let one = event.target instanceof Element ? event.target : null; one; one = one.parentElement) {
            if (one.scrollHeight > one.clientHeight + 1 && /(auto|scroll)/.test(getComputedStyle(one).overflowY)) {
              inner = one as HTMLElement;
              break;
            }
          }
          const further = inner !== null && (event.deltaY > 0 ? inner.scrollTop + inner.clientHeight < inner.scrollHeight - 1 : inner.scrollTop > 0);
          if (!here && !further) return intent.scrolled();
          comeIn();
          if (inner) inner.style.overscrollBehavior = "contain";
        },
        { capture: true, passive: true },
      );
      window.addEventListener(
        "focusout",
        (event) => {
          if (!theirs(event) || demoMoving) return;
          if ((event as FocusEvent).relatedTarget || performance.now() - pressedAt < 400) {
            if (event.target === input) {
              comeIn();
              unfocused = true;
            }
            return;
          }
          if (event.target === input) keptFocus = true;
          event.stopImmediatePropagation();
        },
        true,
      );
      window.addEventListener("focusin", (event) => event.target === input && (keptFocus = false), true);
      window.addEventListener("pointermove", (event) => theirs(event) && mark(event.target), true);
      document.addEventListener("mouseout", (event) => theirs(event) && !event.relatedTarget && goOut());
      for (const type of ["pointerdown", "mousedown", "pointerup", "mouseup", "click", "dblclick", "auxclick", "contextmenu"]) {
        window.addEventListener(
          type,
          (event) => {
            if (!theirs(event) || !refusedAt(event.target)) return;
            event.preventDefault();
            event.stopImmediatePropagation();
          },
          true,
        );
      }
      window.addEventListener(
        "keydown",
        (event) => {
          if (!theirs(event)) return;
          if (event.target instanceof HTMLInputElement && event.target !== input) return;
          const typing = (event.key.length === 1 && !event.ctrlKey && !event.metaKey) || ["Backspace", "Delete"].includes(event.key);
          const pasting = (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "v";
          if (!typing && !pasting) return;
          event.preventDefault();
          event.stopImmediatePropagation();
        },
        true,
      );
      for (const type of ["dragover", "drop"]) window.addEventListener(type, (event) => event.preventDefault(), true);
    },
  };
})();

function showStill(input: HTMLInputElement) {
  closeDialogs();
  document.querySelector<HTMLElement>('[aria-label="Clear search"]')?.click();
  const { query } = SCENARIOS[ORDER[scenario]];
  if (query) {
    focusField(input);
    setText(input, query);
    return;
  }
  const box = input.getBoundingClientRect();
  const onField = { x: box.left + box.width / 2, y: box.top + box.height / 2 };
  dragFiles.enter([CHART.path], onField);
  dragFiles.drop([CHART.path], onField);
}

async function begin() {
  const input = await field();
  visitor.start(input);
  const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  window.addEventListener("message", (event) => {
    if (event.origin !== location.origin || event.source !== window.parent || event.data?.type !== "mesa-demo-scenario") return;
    const chosen = ORDER.indexOf(event.data.scenario);
    if (chosen < 0) return;
    const same = chosen === scenario;
    scenario = chosen;
    if (still) showStill(input);
    else if (!((same || untyped === run) && visitor.goOnNow())) visitor.again();
  });
  if (still) {
    showStill(input);
    return;
  }
  watchOnScreen();
  pointer.start();
  void searchByItself();
}

function handOver() {
  const shell = document.getElementById("demo-shell");
  const app = document.getElementById("root");
  if (!shell || !app) return;
  const said = (within: Element) =>
    JSON.stringify(["header", `.${appStyles.totals}`, `.${appStyles.legalCorner}`].map((part) => within.querySelector(part)?.textContent));
  const frame = said(shell.firstElementChild!);
  const since = performance.now();
  const look = () => {
    if (said(app) === frame || performance.now() - since > 1000) shell.remove();
    else requestAnimationFrame(look);
  };
  requestAnimationFrame(look);
}

export default function DemoApp() {
  const [isReady, setReady] = useState(false);
  useEffect(() => void ready().then(() => setReady(true)), []);
  useEffect(() => {
    if (!isReady) return;
    handOver();
    void begin();
  }, [isReady]);
  return isReady ? <App /> : null;
}
