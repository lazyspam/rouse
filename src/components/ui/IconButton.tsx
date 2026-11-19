import type { ButtonHTMLAttributes } from "react";
import { HugeiconsIcon, type IconSvgElement } from "@hugeicons/react";
import { cn } from "../../utils/cn";

type IconButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> & {
  icon: IconSvgElement;
  /** Accessible name, also shown as the tooltip. */
  label: string;
  size?: "sm" | "md";
};

/** The one square icon button. `className` replaces the colours, hover included. */
export function IconButton({ icon, label, className, size = "md", ...props }: IconButtonProps) {
  return (
    <button
      aria-label={label}
      title={label}
      className={cn(
        "flex shrink-0 items-center justify-center rounded-md transition-colors disabled:opacity-40",
        size === "md" ? "h-7 w-7" : "h-6 w-6",
        className ?? "text-muted hover:bg-raised hover:text-fg",
      )}
      {...props}
    >
      <HugeiconsIcon icon={icon} size={size === "md" ? 14 : 13} strokeWidth={2} />
    </button>
  );
}
