/**
 * Our own toolbar and explorer icons: simple 16 px line drawings in the
 * current text colour, so they follow the theme and the disabled state. They
 * are drawn for this product; none is traced from the reference product.
 */
import type { ReactNode } from 'react';

/** Icon edge, px (toolbar density, ESPEC-INTERFACE §7). */
const ICON_SIZE = 16;
const STROKE_WIDTH = 1.5;

function Icon({ children, title }: { children: ReactNode; title?: string }) {
  return (
    <svg
      width={ICON_SIZE}
      height={ICON_SIZE}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={STROKE_WIDTH}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden={title ? undefined : true}
      role={title ? 'img' : undefined}
    >
      {title && <title>{title}</title>}
      {children}
    </svg>
  );
}

/** Every icon the shell uses, by name. */
export const ICONS = {
  newFile: (
    <Icon>
      <path d="M4 1.5h5l3 3v10H4z" />
      <path d="M9 1.5v3h3" />
      <path d="M8 7.5v5M5.5 10h5" />
    </Icon>
  ),
  open: (
    <Icon>
      <path d="M1.5 4.5v8h11l2-5.5H4.5l-2 5.5" />
      <path d="M1.5 4.5v-2h4l1 1.5h5v3" />
    </Icon>
  ),
  save: (
    <Icon>
      <path d="M2 2h10l2 2v10H2z" />
      <path d="M5 2v4h6V2M4.5 14v-4.5h7V14" />
    </Icon>
  ),
  addDataset: (
    <Icon>
      <path d="M8 3v10M3 8h10" />
    </Icon>
  ),
  removeDataset: (
    <Icon>
      <path d="M3 8h10" />
    </Icon>
  ),
  moveUp: (
    <Icon>
      <path d="M8 13V3M4 7l4-4 4 4" />
    </Icon>
  ),
  moveDown: (
    <Icon>
      <path d="M8 3v10M4 9l4 4 4-4" />
    </Icon>
  ),
  configure: (
    <Icon>
      <circle cx="8" cy="8" r="2.2" />
      <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" />
    </Icon>
  ),
  append: (
    <Icon>
      <path d="M2.5 4.5h7M2.5 8h7M2.5 11.5h4" />
      <path d="M11.5 9.5v5M9 12h5" />
    </Icon>
  ),
  flagManual: (
    <Icon>
      <path d="M3.5 14.5v-13" />
      <path d="M3.5 2h8l-2 3 2 3h-8" />
    </Icon>
  ),
  flagRules: (
    <Icon>
      <path d="M3 14.5v-13" />
      <path d="M3 2h6l-1.5 2.5L9 7H3" />
      <path d="M10 10.5h4.5M10 13h4.5" />
    </Icon>
  ),
  inspectFlags: (
    <Icon>
      <circle cx="6.5" cy="6.5" r="4" />
      <path d="M9.5 9.5l5 5" />
      <path d="M5 4.5v4M5 4.5h3l-.8 1.2.8 1.3H5" />
    </Icon>
  ),
  timeSeries: (
    <Icon>
      <path d="M1.5 14.5h13" />
      <path d="M2 11l3-4 2.5 2.5L11 4l3 3" />
    </Icon>
  ),
  histogram: (
    <Icon>
      <path d="M1.5 14.5h13" />
      <path d="M3 14.5v-4h2.5v4M6.5 14.5v-9H9v9M10 14.5v-6h2.5v6" />
    </Icon>
  ),
  windRose: (
    <Icon>
      <circle cx="8" cy="8" r="6.5" />
      <path d="M8 8l1.5-5.5L8 8l4 3.5L8 8l-5 1.5z" />
    </Icon>
  ),
  diurnal: (
    <Icon>
      <path d="M1.5 14.5h13" />
      <path d="M2 10c2-5 4-6 6-6s4 1 6 6" />
      <circle cx="8" cy="4" r="1" />
    </Icon>
  ),
  coverage: (
    <Icon>
      <path d="M1.5 4h5M8.5 4h6M1.5 8h9M12 8h2.5M1.5 12h3M6 12h8.5" />
    </Icon>
  ),
  shear: (
    <Icon>
      <path d="M2.5 14.5v-13" />
      <path d="M2.5 13h3M2.5 9.5h6M2.5 6h8.5M2.5 2.5h10" />
    </Icon>
  ),
  turbulence: (
    <Icon>
      <path d="M1.5 5c2-2 3 2 5 0s3 2 5 0 2-1 3-1" />
      <path d="M1.5 11c2-2 3 2 5 0s3 2 5 0 2-1 3-1" />
    </Icon>
  ),
  undo: (
    <Icon>
      <path d="M5.5 3.5L2.5 6.5l3 3" />
      <path d="M2.5 6.5h7a4 4 0 0 1 0 8H7" />
    </Icon>
  ),
  redo: (
    <Icon>
      <path d="M10.5 3.5l3 3-3 3" />
      <path d="M13.5 6.5h-7a4 4 0 0 0 0 8H9" />
    </Icon>
  ),
  help: (
    <Icon>
      <circle cx="8" cy="8" r="6.5" />
      <path d="M6.2 6.3a1.9 1.9 0 1 1 2.6 1.8c-.5.2-.8.6-.8 1.1v.6" />
      <path d="M8 12h.01" />
    </Icon>
  ),
  close: (
    <Icon>
      <path d="M4 4l8 8M12 4l-8 8" />
    </Icon>
  ),
  tower: (
    <Icon>
      <path d="M8 1.5v13M5 14.5h6M8 3.5l-3 1.2M8 3.5l3 1.2M8 7l-2.5 1M8 7l2.5 1" />
    </Icon>
  ),
} satisfies Record<string, ReactNode>;

/** The product's mark: a vertical axis crossed by three wind streams. */
export function Logo({ size = ICON_SIZE }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
      <rect width="32" height="32" rx="7" fill="var(--brand)" />
      <path d="M16 5v22" stroke="var(--brand-text)" strokeWidth="2.4" strokeLinecap="round" />
      <path d="M5 11c4-3 8 3 12 0s7-2 10 0M5 17c4-3 8 3 12 0s7-2 10 0M5 23c4-3 8 3 12 0" fill="none" stroke="var(--brand-text)" strokeWidth="1.8" strokeLinecap="round" opacity="0.85" />
    </svg>
  );
}
