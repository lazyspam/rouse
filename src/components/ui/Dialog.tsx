import { useEffect, type ReactNode } from "react";
import { Cancel01Icon } from "@hugeicons/core-free-icons";
import { IconButton } from "./IconButton";
import { cn } from "../../utils/cn";

type DialogProps = {
  title: string;
  description?: string;
  onClose: () => void;
  footer?: ReactNode;
  wide?: boolean;
  children: ReactNode;
};

/** A centred sheet over the window. Escape or the backdrop closes it. */
export function Dialog({ title, description, onClose, footer, wide, children }: DialogProps) {
  useEffect(() => {
    const esc = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("keydown", esc);
    return () => document.removeEventListener("keydown", esc);
  }, [onClose]);

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/50 p-8" onMouseDown={onClose}>
      <div
        onMouseDown={(e) => e.stopPropagation()}
        className={cn(
          "sheet-in flex max-h-full w-full flex-col rounded-xl bg-surface shadow-2xl shadow-black/50 ring-1 ring-line",
          wide ? "max-w-3xl" : "max-w-md",
        )}
      >
        <div className="flex items-start gap-3 px-5 pt-4 pb-3">
          <div className="min-w-0 flex-1">
            <h2 className="text-title3">{title}</h2>
            {description && <p className="mt-1 text-callout text-muted">{description}</p>}
          </div>
          <IconButton icon={Cancel01Icon} label="Close" onClick={onClose} />
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-5 pb-4">{children}</div>
        {footer && <div className="flex items-center justify-end gap-2 border-t border-line px-5 py-3">{footer}</div>}
      </div>
    </div>
  );
}
