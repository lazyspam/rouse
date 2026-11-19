import { getCurrentWindow } from "@tauri-apps/api/window";
import { Cancel01Icon, MinusSignIcon, SquareIcon } from "@hugeicons/core-free-icons";
import { IconButton } from "./ui/IconButton";

/** Minimize, maximize and close-to-tray, for the undecorated window. */
export function WindowControls() {
  const win = getCurrentWindow();
  return (
    <div className="flex items-center gap-1">
      <IconButton icon={MinusSignIcon} label="Minimize" onClick={() => win.minimize()} />
      <IconButton icon={SquareIcon} label="Maximize" onClick={() => win.toggleMaximize()} />
      <IconButton
        icon={Cancel01Icon}
        label="Hide to tray (services keep running)"
        onClick={() => win.hide()}
        className="text-muted hover:bg-danger hover:text-bg"
      />
    </div>
  );
}
