import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent } from "react";
import { ChevronLeft, ChevronRight, X } from "lucide-react";
import { api, largePreviewUrl, mediaUrl, type SearchHit } from "../api";
import { cx } from "../cx";
import { pageFromText, pagingOf, pagingWord } from "../paging";
import { prefersReducedMotion } from "../motion";
import { usePixelRatio } from "../usePreview";
import { Button } from "./Button";
import { IconButton } from "./IconButton";
import { Modal } from "./Modal";
import { EASE_CURVE, EASE_MS } from "./PanelSplitter";
import styles from "./PreviewViewer.module.css";

const LARGEST = 1600;
const LEAVING_MS = 200;
const RESHAPED = 0.02;

export interface ViewedFrom {
  rect: () => DOMRect | null;
  src: string | null;
  shape: number | null;
}

interface Props {
  hit: SearchHit;
  page: number;
  from: ViewedFrom;
  onOpen: () => void;
  onClosed: () => void;
}

export function PreviewViewer({ hit, page: firstPage, from, onOpen, onClosed }: Props) {
  const paging = pagingOf(hit.name);
  const paged = paging !== null;
  const video = hit.category === "video";
  const frame = useRef<HTMLDivElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const ratio = usePixelRatio();
  const [size] = useState(() => Math.min(LARGEST, Math.ceil((Math.max(window.innerWidth, window.innerHeight) * ratio) / 128) * 128));
  const [shape, setShape] = useState(from.shape ?? 4 / 3);
  const reshape = (next: number) => setShape((was) => (Math.abs(next / was - 1) > RESHAPED ? next : was));

  const [page, setPage] = useState(firstPage);
  const [pages, setPages] = useState<number | null>(null);
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => {
    if (!paged) return;
    let live = true;
    api.paging(hit.path).then(
      (paging) => {
        if (!live) return;
        setPages(paging?.pages ?? null);
        setNote(paging?.note ?? null);
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [paged, hit.path]);
  const turn = (by: number) => setPage((at) => Math.max(1, Math.min(pages ?? at + by, at + by)));
  const src = largePreviewUrl(hit.path, size, page);
  const [shown, setShown] = useState<string | null>(from.src);
  useEffect(() => {
    let live = true;
    const image = new Image();
    image.src = src;
    image.decode().then(
      () => {
        if (!live) return;
        if (image.naturalWidth > 0 && image.naturalHeight > 0) reshape(image.naturalWidth / image.naturalHeight);
        setShown(src);
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [src]);

  const [playing, setPlaying] = useState<"asking" | "allowed" | "shown" | "failed">(video ? "asking" : "failed");
  useEffect(() => {
    if (!video) return;
    let live = true;
    api.allowPlaying(hit.path).then(
      () => live && setPlaying("allowed"),
      () => live && setPlaying("failed"),
    );
    return () => {
      live = false;
    };
  }, [video, hit.path]);

  const shapeNow = useRef(shape);
  shapeNow.current = shape;
  const place = useRef(() => {
    const room = stage.current;
    const box = frame.current;
    if (!room || !box) return;
    const at = room.getBoundingClientRect();
    if (at.width === 0 || at.height === 0) return;
    const scale = window.devicePixelRatio || 1;
    const ratio = shapeNow.current;
    let width = Math.floor(at.width * scale);
    let height = Math.round(width / ratio);
    if (height > Math.floor(at.height * scale)) {
      height = Math.floor(at.height * scale);
      width = Math.round(height * ratio);
    }
    const left = Math.round(at.left * scale + (at.width * scale - width) / 2) / scale - at.left;
    const top = Math.round(at.top * scale + (at.height * scale - height) / 2) / scale - at.top;
    Object.assign(box.style, { left: `${left}px`, top: `${top}px`, width: `${width / scale}px`, height: `${height / scale}px` });
  }).current;
  useLayoutEffect(place, [shape, place]);
  useEffect(() => {
    const room = stage.current;
    if (!room) return;
    const observer = new ResizeObserver(place);
    observer.observe(room);
    window.addEventListener("resize", place);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", place);
    };
  }, [place]);

  useEffect(() => {
    queueMicrotask(() => {
      place();
      const box = frame.current;
      const start = from.rect();
      if (!box || !start || prefersReducedMotion()) return;
      box.animate([{ transform: towards(start, box.getBoundingClientRect()) }, { transform: "none" }], { duration: EASE_MS, easing: EASE_CURVE });
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const [leaving, setLeaving] = useState(false);
  const [shut, setShut] = useState(false);
  const leave = () => {
    if (leaving) return;
    setLeaving(true);
    const box = frame.current;
    box?.querySelector("video")?.pause();
    const end = from.rect();
    if (!box || !end || prefersReducedMotion()) return setShut(true);
    const moving = box.animate([{ transform: "none" }, { transform: towards(end, box.getBoundingClientRect()) }], {
      duration: LEAVING_MS,
      easing: "ease-in",
      fill: "forwards",
    });
    moving.onfinish = () => setShut(true);
    moving.oncancel = () => setShut(true);
  };
  useEffect(() => {
    if (shut) onClosed();
  }, [shut, onClosed]);

  const turnBy = useRef(turn);
  turnBy.current = turn;
  useEffect(() => {
    if (!paged) return;
    const onKeyDown = (e: globalThis.KeyboardEvent) => {
      if (e.ctrlKey || e.altKey || e.metaKey || e.defaultPrevented) return;
      if (e.target instanceof HTMLInputElement) return;
      const by = e.key === "ArrowLeft" || e.key === "PageUp" ? -1 : e.key === "ArrowRight" || e.key === "PageDown" ? 1 : 0;
      if (!by) return;
      e.preventDefault();
      turnBy.current(by);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [paged]);
  const onClick = (e: MouseEvent) => {
    if (!(e.target as Element).closest("[data-viewer-keep]")) leave();
  };

  const at = hit.at;
  return (
    <Modal open={!shut} onClose={leave} className={cx(styles.viewer, leaving && styles.leaving)} label={hit.name} focusCard>
      <div className={styles.content} onClick={onClick}>
        <div ref={stage} className={styles.stage}>
          <div ref={frame} className={styles.frame} data-viewer-keep>
            {shown !== null && <img className={styles.image} src={shown} alt="" draggable={false} />}
            {(playing === "allowed" || playing === "shown") && (
              <video
                className={cx(styles.video, playing === "shown" && styles.shown)}
                src={mediaUrl(hit.path)}
                controls
                autoPlay
                playsInline
                onLoadedMetadata={(e) => {
                  const media = e.currentTarget;
                  if (media.videoWidth > 0 && media.videoHeight > 0) reshape(media.videoWidth / media.videoHeight);
                  if (at !== null && at > 0 && Number.isFinite(media.duration) && at / 1000 < media.duration - 0.5) media.currentTime = at / 1000;
                }}
                onLoadedData={() => setPlaying("shown")}
                onError={() => setPlaying("failed")}
              />
            )}
          </div>
        </div>
        <div className={styles.bar} data-viewer-keep>
          <span className={styles.name} data-tooltip={hit.name}>
            {hit.name}
          </span>
          {paging && (
            <div className={styles.pages}>
              <IconButton label={`Previous ${paging}`} onClick={() => turn(-1)} disabled={page <= 1}>
                <ChevronLeft size={18} aria-hidden />
              </IconButton>
              <span className={styles.pageLabel}>
                {pagingWord(paging)} <PageField page={page} pages={pages} label={`${pagingWord(paging)} to show`} onGo={setPage} />
                {pages !== null && ` of ${pages}`}
              </span>
              <IconButton label={`Next ${paging}`} onClick={() => turn(1)} disabled={pages !== null && page >= pages}>
                <ChevronRight size={18} aria-hidden />
              </IconButton>
            </div>
          )}
          {note && (
            <span className={styles.note} data-tooltip={note}>
              {note}
            </span>
          )}
          <Button variant="primary" onClick={onOpen}>
            Open
          </Button>
          <IconButton label="Close" onClick={leave}>
            <X size={18} aria-hidden />
          </IconButton>
        </div>
      </div>
    </Modal>
  );
}

function towards(from: DOMRect, to: DOMRect): string {
  if (to.width === 0 || to.height === 0) return "none";
  return `translate(${from.left - to.left}px, ${from.top - to.top}px) scale(${from.width / to.width}, ${from.height / to.height})`;
}

function PageField({ page, pages, label, onGo }: { page: number; pages: number | null; label: string; onGo: (page: number) => void }) {
  const [typed, setTyped] = useState<string | null>(null);
  const invalid = typed !== null && typed !== "" && pageFromText(typed, pages) === null;
  const go = () => {
    if (typed === null) return;
    const to = pageFromText(typed, pages);
    if (to !== null) onGo(to);
    setTyped(null);
  };
  return (
    <input
      className={cx(styles.pageField, invalid && styles.invalid)}
      style={{ width: `calc(${String(pages ?? page).length}ch + 14px)` }}
      value={typed ?? String(page)}
      inputMode="numeric"
      aria-label={label}
      aria-invalid={invalid}
      onFocus={(e) => e.currentTarget.select()}
      onChange={(e) => setTyped(e.currentTarget.value.replace(/\D/g, ""))}
      onBlur={go}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          go();
          e.currentTarget.blur();
        } else if (e.key === "Escape" && typed !== null) {
          e.preventDefault();
          e.stopPropagation();
          setTyped(null);
        }
      }}
    />
  );
}
