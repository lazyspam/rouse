// The app's live data: the saved tree and every service's run state, kept current from the
// Rust side's status events.

import { useCallback, useEffect, useState } from "react";
import { api, message, onEvent, type Logo, type State, type Tree } from "../services/tauri";

export function useRouse(notify: (text: string, tone?: "info" | "error") => void) {
  const [tree, setTree] = useState<Tree | null>(null);
  const [states, setStates] = useState<Record<string, State>>({});
  const [icons, setIcons] = useState<Record<string, Logo>>({});

  // Logos are re-read with every refresh, so a changed favicon shows up without a rescan.
  const refresh = useCallback(() => {
    api.tree().then(setTree).catch((e) => notify(message(e), "error"));
    api.projectIcons().then(setIcons).catch(() => {});
  }, [notify]);

  useEffect(() => {
    refresh();
    api.states().then(setStates).catch(() => {});
    const unlisten = onEvent((e) => {
      if (e.kind === "status") setStates((s) => ({ ...s, [e.id]: e.status }));
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [refresh]);

  return { tree, states, icons, refresh };
}
