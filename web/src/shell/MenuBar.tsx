/**
 * The menu bar (ESPEC-INTERFACE §1.2), desktop style: a click opens a menu,
 * moving over another title while one is open switches to it, Escape or a
 * click outside closes. Items not built yet stay visible and disabled
 * (§7: disabled, not hidden), with a tip saying so.
 */
import { useEffect, useRef, useState } from 'react';

/** One command of a menu. */
export interface MenuItem {
  label: string;
  action?: () => void;
  /** Greyed out; also when there is no action. */
  disabled?: boolean;
  /** Shown at the right, e.g. "Ctrl+Z". */
  shortcut?: string;
  /** Tooltip, e.g. why the item is disabled. */
  title?: string;
  /** Shows a check mark (a view that can be shown or hidden). */
  checked?: boolean;
}

/** A separator line between groups of items. */
export const SEPARATOR = 'separator' as const;
export type MenuEntry = MenuItem | typeof SEPARATOR;

/** One top-level menu. */
export interface Menu {
  label: string;
  items: MenuEntry[];
}

/** Tip of an item that is still to come. */
export const NOT_YET = 'Ainda não disponível nesta versão';

export function MenuBar({ menus }: { menus: Menu[] }) {
  const [open, setOpen] = useState<number | null>(null);
  const bar = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open == null) return;
    const close = (event: MouseEvent | KeyboardEvent) => {
      if (event instanceof KeyboardEvent ? event.key === 'Escape' : !bar.current?.contains(event.target as Node)) setOpen(null);
    };
    window.addEventListener('mousedown', close);
    window.addEventListener('keydown', close);
    return () => {
      window.removeEventListener('mousedown', close);
      window.removeEventListener('keydown', close);
    };
  }, [open]);

  return (
    <div className="menubar" role="menubar" ref={bar}>
      {menus.map((menu, index) => (
        <div key={menu.label} className="menu">
          <button
            role="menuitem"
            aria-haspopup="menu"
            aria-expanded={open === index}
            className={open === index ? 'open' : ''}
            onClick={() => setOpen(open === index ? null : index)}
            onMouseEnter={() => open != null && setOpen(index)}
          >
            {menu.label}
          </button>
          {open === index && (
            <div className="dropdown" role="menu">
              {menu.items.map((item, position) =>
                item === SEPARATOR ? (
                  <div key={`separator-${position}`} className="separator" role="separator" />
                ) : (
                  <button
                    key={item.label}
                    role="menuitem"
                    disabled={item.disabled || !item.action}
                    title={item.title ?? (item.action ? undefined : NOT_YET)}
                    onClick={() => {
                      setOpen(null);
                      item.action?.();
                    }}
                  >
                    <span className="check">{item.checked ? '✓' : ''}</span>
                    <span className="label">{item.label}</span>
                    <span className="shortcut">{item.shortcut}</span>
                  </button>
                ),
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
