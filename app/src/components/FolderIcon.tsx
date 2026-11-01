interface Props {
  full: boolean;
  size?: number;
  className?: string;
}

const BODY =
  "M4 14a4 4 0 0 1 4-4h14.5a4 4 0 0 1 2.9 1.25l3.7 3.9a4 4 0 0 0 2.9 1.25H56a4 4 0 0 1 4 4V50a4 4 0 0 1-4 4H8a4 4 0 0 1-4-4Z";
const FRONT_TOP = 28;
const SHEETS_TOP = 20;
const FRONT = `M4 ${FRONT_TOP + 4}a4 4 0 0 1 4-4h48a4 4 0 0 1 4 4V50a4 4 0 0 1-4 4H8a4 4 0 0 1-4-4Z`;
const BACK_SHEET = `M17 ${SHEETS_TOP + 2}V${SHEETS_TOP}a2 2 0 0 1 2-2h26a2 2 0 0 1 2 2V${SHEETS_TOP + 2}Z`;
const FRONT_SHEET = `M12 ${FRONT_TOP + 2}V${SHEETS_TOP + 2}a2 2 0 0 1 2-2h36a2 2 0 0 1 2 2V${FRONT_TOP + 2}Z`;

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
      <path d={BODY} fill="var(--color-folder-back)" />
      {full && (
        <>
          <path d={BACK_SHEET} fill="var(--color-folder-paper)" fillOpacity={0.55} />
          <path d={FRONT_SHEET} fill="var(--color-folder-paper)" />
        </>
      )}
      <path d={FRONT} fill="currentColor" />
    </svg>
  );
}
