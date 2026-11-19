import type { ReactNode } from "react";
import { WindowControls } from "./WindowControls";

/** Drag strip above the page: title, optional subtitle, page actions and window controls. */
export function TitleBar({ title, subtitle, children }: { title: string; subtitle?: ReactNode; children?: ReactNode }) {
  return (
    <header data-tauri-drag-region className="flex h-12 shrink-0 items-center gap-2 pr-3 pl-6">
      <div className="pointer-events-none flex min-w-0 flex-1 items-baseline gap-2">
        <h1 className="truncate text-title3">{title}</h1>
        {subtitle && <span className="truncate text-callout text-dim">{subtitle}</span>}
      </div>
      {children && (
        <>
          <div className="flex items-center gap-1.5">{children}</div>
          <div className="mx-1 h-4 w-px bg-line" />
        </>
      )}
      <WindowControls />
    </header>
  );
}
