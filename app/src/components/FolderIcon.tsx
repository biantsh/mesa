interface Props {
  full: boolean;
  size?: number;
  className?: string;
}

const BODY =
  "M4 14a4 4 0 0 1 4-4h14.5a4 4 0 0 1 2.9 1.25l3.7 3.9a4 4 0 0 0 2.9 1.25H56a4 4 0 0 1 4 4V50a4 4 0 0 1-4 4H8a4 4 0 0 1-4-4Z";
const BACK_SHEET = "M17 24v-2.5a2 2 0 0 1 2-2h26a2 2 0 0 1 2 2V24Z";
const FRONT_SHEET = "M12 30v-6a2 2 0 0 1 2-2h36a2 2 0 0 1 2 2v6Z";

const VIEW = { x: 4, y: 10, width: 56, height: 44 };

export function FolderIcon({ full, size = 76, className }: Props) {
  return (
    <svg
      className={className}
      width={size}
      height={(size * VIEW.height) / VIEW.width}
      viewBox={`${VIEW.x} ${VIEW.y} ${VIEW.width} ${VIEW.height}`}
      aria-hidden="true"
      focusable="false"
    >
      <path d={BODY} fill="currentColor" />
      {full && (
        <>
          <path d={BACK_SHEET} fill="var(--color-folder-paper)" fillOpacity={0.55} />
          <path d={FRONT_SHEET} fill="var(--color-folder-paper)" />
        </>
      )}
    </svg>
  );
}
