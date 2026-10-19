/**
 * A modal dialog over the main window (ESPEC-INTERFACE §1.3): a title bar
 * with a close button, the body, and the buttons at the bottom right.
 * Escape closes it like Cancel.
 */
import { useEffect, type ReactNode } from 'react';
import { ICONS } from '../shell/icons';

interface Props {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** The buttons, rightmost last (usually OK then Cancel). */
  buttons: ReactNode;
  /** Wider dialogs hold tables. */
  wide?: boolean;
}

export function Modal({ title, onClose, children, buttons, wide = false }: Props) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => event.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);
  return (
    <div className="modal-backdrop" role="presentation">
      <div className={`modal ${wide ? 'wide' : ''}`} role="dialog" aria-modal="true" aria-label={title}>
        <header className="modal-title">
          <span>{title}</span>
          <button className="iconbutton" aria-label="Fechar" onClick={onClose}>
            {ICONS.close}
          </button>
        </header>
        <div className="modal-body">{children}</div>
        <footer className="modal-buttons">{buttons}</footer>
      </div>
    </div>
  );
}
