// The window has no native frame, so its edges get thin handles that start a resize drag.

import { getCurrentWindow } from "@tauri-apps/api/window";

type Dir = "North" | "South" | "East" | "West" | "NorthEast" | "NorthWest" | "SouthEast" | "SouthWest";

const EDGES: { dir: Dir; className: string }[] = [
  { dir: "North", className: "top-0 left-2 right-2 h-1 cursor-ns-resize" },
  { dir: "South", className: "bottom-0 left-2 right-2 h-1 cursor-ns-resize" },
  { dir: "West", className: "left-0 top-2 bottom-2 w-1 cursor-ew-resize" },
  { dir: "East", className: "right-0 top-2 bottom-2 w-1 cursor-ew-resize" },
  { dir: "NorthWest", className: "top-0 left-0 h-2 w-2 cursor-nwse-resize" },
  { dir: "SouthEast", className: "bottom-0 right-0 h-2 w-2 cursor-nwse-resize" },
  { dir: "NorthEast", className: "top-0 right-0 h-2 w-2 cursor-nesw-resize" },
  { dir: "SouthWest", className: "bottom-0 left-0 h-2 w-2 cursor-nesw-resize" },
];

export function ResizeHandles() {
  return (
    <>
      {EDGES.map(({ dir, className }) => (
        <div
          key={dir}
          className={`absolute z-[60] ${className}`}
          onMouseDown={(e) => {
            if (e.button === 0) void getCurrentWindow().startResizeDragging(dir);
          }}
        />
      ))}
    </>
  );
}
