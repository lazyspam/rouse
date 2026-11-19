import { useEffect, useRef, useState, type ReactNode } from "react";
import { MoreHorizontalIcon } from "@hugeicons/core-free-icons";
import { IconButton } from "./IconButton";
import { cn } from "../../utils/cn";

export type MenuItem = { label: string; onSelect: () => void; danger?: boolean } | "divider";

/** A "…" button with a small popover of actions. Destructive items go last. */
export function Menu({ items, label, className, trigger }: { items: MenuItem[]; label: string; className?: string; trigger?: ReactNode }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => ref.current?.contains(e.target as Node) || setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);

  return (
    <div ref={ref} className={cn("relative", className)} onClick={(e) => e.stopPropagation()}>
      {trigger ? (
        <span onClick={() => setOpen((o) => !o)}>{trigger}</span>
      ) : (
        <IconButton icon={MoreHorizontalIcon} label={label} size="sm" onClick={() => setOpen((o) => !o)} />
      )}
      {open && (
        <div className="sheet-in absolute top-full right-0 z-40 mt-1 min-w-44 rounded-lg bg-raised p-1 shadow-xl shadow-black/40 ring-1 ring-line">
          {items.map((item, i) =>
            item === "divider" ? (
              <div key={i} className="my-1 h-px bg-line" />
            ) : (
              <button
                key={item.label}
                onClick={() => {
                  setOpen(false);
                  item.onSelect();
                }}
                className={cn(
                  "flex h-7 w-full items-center rounded-md px-2.5 text-left text-body transition-colors",
                  item.danger ? "text-danger hover:bg-danger/10" : "text-fg hover:bg-white/5",
                )}
              >
                {item.label}
              </button>
            ),
          )}
        </div>
      )}
    </div>
  );
}
