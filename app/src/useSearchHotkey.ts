import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorText, hotkeyFailure, type HotkeyStatus } from "./api";
import { NONE_HELD, heldFrom, isKeyCode, isModifierCode, anyHeld, shortcutString, type Held } from "./hotkey";

export function useSearchHotkey(open: boolean) {
  const [status, setStatus] = useState<HotkeyStatus | null>(null);
  const [recording, setRecording] = useState(false);
  const [held, setHeld] = useState<Held>(NONE_HELD);
  const [refused, setRefused] = useState<string | null>(null);
  const [hint, setHint] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const recordingRef = useRef(false);
  const savingRef = useRef(false);
  const alive = useRef(true);
  const field = useRef<HTMLButtonElement>(null);
  const justEnded = useRef(false);
  const refocus = useRef(false);

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
    setHint(null);
    if (was && !taken) api.endHotkeyRecording().catch(() => {});
    if (was) {
      justEnded.current = true;
      setTimeout(() => (justEnded.current = false), 0);
    }
    if (was && refocus.current) setTimeout(() => field.current?.focus(), 0);
  }, []);

  const start = useCallback((byKeyboard: boolean) => {
    if (recordingRef.current || savingRef.current) return;
    refocus.current = byKeyboard;
    recordingRef.current = true;
    setError(null);
    setHeld(NONE_HELD);
    setRefused(null);
    setHint(null);
    setRecording(true);
    api.beginHotkeyRecording().catch(() => {});
  }, []);

  const cancel = useCallback(() => finish(false), [finish]);

  const choose = useCallback(
    (shortcut: string | null, code: string | null = null) => {
      if (savingRef.current) return;
      savingRef.current = true;
      setSaving(true);
      setError(null);
      api
        .setSearchHotkey(shortcut)
        .then((now) => {
          if (!alive.current) return;
          setStatus(now);
          finish(true);
        })
        .catch((e) => {
          if (!alive.current) return;
          const failure = hotkeyFailure(e);
          if (failure.kind === "invalid" && recordingRef.current) {
            setRefused(code);
            setHint(failure.message);
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
    const down = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.repeat || savingRef.current) return;
      const now = heldFrom(event);
      if (isModifierCode(event.code)) {
        setHeld(now);
        setRefused(null);
        setHint(null);
        return;
      }
      if (!anyHeld(now) && event.code === "Escape") return cancel();
      if (!anyHeld(now) && event.code === "Backspace") return reset();
      if (!isKeyCode(event.code)) return;
      setHeld(now);
      choose(shortcutString(now, event.code), event.code);
    };
    const up = (event: KeyboardEvent) => {
      if (isModifierCode(event.code)) setHeld(heldFrom(event));
    };
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    const outside = (event: MouseEvent) => {
      if (!(event.target instanceof Node && field.current?.contains(event.target))) cancel();
    };
    window.addEventListener("click", outside, true);
    window.addEventListener("blur", cancel);
    return () => {
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
      window.removeEventListener("click", outside, true);
      window.removeEventListener("blur", cancel);
    };
  }, [recording, cancel, reset, choose]);

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
    hint,
    error,
    saving,
    field,
    start,
    cancel,
    reset,
    isRecording: () => recordingRef.current || justEnded.current,
  };
}
