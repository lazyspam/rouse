// The app's live data: the saved tree and every service's run state, kept current from the
// Rust side's status events.

import { useCallback, useEffect, useState } from "react";
import { api, message, onEvent, type State, type Tree } from "../services/tauri";

export function useRouse(notify: (text: string, tone?: "info" | "error") => void) {
  const [tree, setTree] = useState<Tree | null>(null);
  const [states, setStates] = useState<Record<string, State>>({});

  const refresh = useCallback(() => {
    api.tree().then(setTree).catch((e) => notify(message(e), "error"));
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

  return { tree, states, refresh };
}
