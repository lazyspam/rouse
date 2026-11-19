import { useId, type InputHTMLAttributes } from "react";
import { cn } from "../../utils/cn";

type FieldProps = InputHTMLAttributes<HTMLInputElement> & { label: string; hint?: string; mono?: boolean };

export function Field({ label, hint, mono, className, ...props }: FieldProps) {
  const id = useId();
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={id} className="text-callout text-muted">
        {label}
      </label>
      <input
        id={id}
        className={cn(
          "h-8 rounded-md bg-bg px-2.5 text-body text-fg ring-1 ring-line placeholder:text-dim focus:ring-accent/50",
          mono && "font-mono text-callout",
          className,
        )}
        {...props}
      />
      {hint && <span className="text-subheadline text-dim">{hint}</span>}
    </div>
  );
}
