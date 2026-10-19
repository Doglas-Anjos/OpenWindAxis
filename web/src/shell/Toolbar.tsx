/**
 * The toolbar (ESPEC-INTERFACE §1.1, item 3): one row of icon buttons for
 * the most used commands, grouped in the menus' order. Each button carries
 * its name as a tooltip and an accessible label.
 */
import type { ReactNode } from 'react';

/** One toolbar button. */
export interface ToolbarButton {
  icon: ReactNode;
  label: string;
  action: () => void;
  disabled?: boolean;
}

/** Groups of buttons, separated by a thin line. */
export function Toolbar({ groups }: { groups: ToolbarButton[][] }) {
  return (
    <div className="toolbar" role="toolbar" aria-label="Ferramentas">
      {groups.map((group, index) => (
        <div key={index} className="group">
          {group.map((button) => (
            <button key={button.label} title={button.label} aria-label={button.label} disabled={button.disabled} onClick={button.action}>
              {button.icon}
            </button>
          ))}
        </div>
      ))}
    </div>
  );
}
