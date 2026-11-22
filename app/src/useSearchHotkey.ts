import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errorText, hotkeyFailure, type HotkeyStatus } from "./api";
import { NONE_HELD, heldKeys, isKeyCode, isModifierCode, anyHeld, shortcutString, type Held } from "./hotkey";

export function useSearchHotkey(open: boolean) {
  const [status, setStatus] = useState<HotkeyStatus | null>(null);
  const [recording, setRecording] = useState(false);
  const [held, setHeld] = useState<Held>(NONE_HELD);
  const [refused, setRefused] = useState<string | null>(null);
  // The key of a shortcut being saved, shown with the held modifiers until recording ends.
  const [chosen, setChosen] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const recordingRef = useRef(false);
  const savingRef = useRef(false);
  const alive = useRef(true);
  const field = useRef<HTMLButtonElement>(null);
  // Replaced with each recording, in case a Super key went up while the window was unfocused.
  const heldNow = useRef(heldKeys());
  const justEnded = useRef(false);
  const refocus = useRef(false);
  // The keys a shortcut was just recorded with, until they're let go: the backend ignores the shortcut until then.
  const settling = useRef<{ code: string; alone: boolean; up: boolean; saved: Promise<unknown> } | null>(null);

  const load = useCallback(() => {
    api
      .getSearchHotkey()
      .then((now) => alive.current && setStatus(now))
      .catch((e) => alive.current && setError(`Couldn't read the search shortcut. ${errorText(e)}`));
  }, []);

  useEffect(() => {
    alive.current = true;
    load();
    return () => {
      alive.current = false;
      if (recordingRef.current) api.endHotkeyRecording().catch(() => {});
    };
  }, [load]);

  useEffect(() => {
    if (open) load();
  }, [open, load]);

  const finish = useCallback((taken: boolean) => {
    const was = recordingRef.current;
    recordingRef.current = false;
    setRecording(false);
    setHeld(NONE_HELD);
    setRefused(null);
    setChosen(null);
    if (was && !taken) api.endHotkeyRecording().catch(() => {});
    if (was) {
      justEnded.current = true;
      setTimeout(() => (justEnded.current = false), 0);
    }
    // Started with the mouse, the field lets go of focus, or the keys just typed would leave it with a focus ring.
    if (was && refocus.current) setTimeout(() => field.current?.focus(), 0);
    else if (was) field.current?.blur();
  }, []);

  const start = useCallback((byKeyboard: boolean) => {
    if (recordingRef.current || savingRef.current) return;
    refocus.current = byKeyboard;
    recordingRef.current = true;
    heldNow.current = heldKeys();
    setError(null);
    setHeld(NONE_HELD);
    setRefused(null);
    setRecording(true);
    api.beginHotkeyRecording().catch(() => {});
  }, []);

  const cancel = useCallback(() => finish(false), [finish]);

  const choose = useCallback(
    (shortcut: string | null, code: string | null = null, held: Held = NONE_HELD) => {
      if (savingRef.current) return;
      savingRef.current = true;
      setSaving(true);
      setError(null);
      setChosen(code);
      const saved = api.setSearchHotkey(shortcut);
      settling.current = code ? { code, alone: !anyHeld(held), up: false, saved } : null;
      // Backspace (back to the default) isn't part of the shortcut, so there's nothing to wait for.
      if (!code) saved.then(() => api.hotkeyKeysReleased()).catch(() => {});
      saved
        .then((now) => {
          if (!alive.current) return;
          setStatus(now);
          finish(true);
        })
        .catch((e) => {
          if (!alive.current) return;
          const failure = hotkeyFailure(e);
          settling.current = null;
          setChosen(null);
          if (failure.kind === "invalid" && recordingRef.current) {
            setRefused(code);
          } else {
            finish(true);
            setError(failure.message);
            load();
          }
        })
        .finally(() => {
          savingRef.current = false;
          if (alive.current) setSaving(false);
        });
    },
    [finish, load],
  );

  const reset = useCallback(() => choose(null), [choose]);

  useEffect(() => {
    if (!recording) return;
    let lastHeld: Held = NONE_HELD;
    const down = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.repeat || savingRef.current) return;
      const now = heldNow.current(event);
      lastHeld = now;
      if (isModifierCode(event.code)) {
        setHeld(now);
        setRefused(null);
        return;
      }
      if (!anyHeld(now) && event.code === "Escape") return cancel();
      if (!anyHeld(now) && event.code === "Backspace") return reset();
      if (!isKeyCode(event.code)) return;
      setHeld(now);
      choose(shortcutString(now, event.code), event.code, now);
    };
    const up = (event: KeyboardEvent) => {
      lastHeld = heldNow.current(event);
      if (isModifierCode(event.code)) setHeld(lastHeld);
    };
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    const outside = (event: MouseEvent) => {
      if (!(event.target instanceof Node && field.current?.contains(event.target))) cancel();
    };
    window.addEventListener("click", outside, true);
    window.addEventListener("blur", cancel);
    const space = listen("system-menu-key", () => {
      if (savingRef.current) return;
      const now = { ...lastHeld, alt: true };
      setHeld(now);
      choose(shortcutString(now, "Space"), "Space", now);
    });
    return () => {
      space.then((unlisten) => unlisten()).catch(() => {});
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
      window.removeEventListener("click", outside, true);
      window.removeEventListener("blur", cancel);
    };
  }, [recording, cancel, reset, choose]);

  useEffect(() => {
    // Once the modifiers are up, the key alone can't press the shortcut, so only a shortcut without modifiers waits
    // for its key (whose key-up the desktop may keep from the window).
    const up = (event: KeyboardEvent) => {
      const keys = settling.current;
      if (!keys) return;
      if (event.code === keys.code) keys.up = true;
      if (anyHeld(heldNow.current(event)) || (keys.alone && !keys.up)) return;
      settling.current = null;
      keys.saved.then(() => api.hotkeyKeysReleased()).catch(() => {});
    };
    window.addEventListener("keyup", up, true);
    return () => window.removeEventListener("keyup", up, true);
  }, []);

  useEffect(() => {
    if (!open && recordingRef.current) {
      refocus.current = false;
      finish(false);
    }
  }, [open, finish]);

  return {
    status,
    recording,
    held,
    refused,
    chosen,
    error,
    saving,
    field,
    start,
    cancel,
    isRecording: () => recordingRef.current || justEnded.current,
  };
}
