// Right-click to copy: copies the text and reports it as copied for a moment, so the clicked
// control can say so in place.

import { useEffect, useRef, useState } from "react";
import { copyText } from "../utils/clipboard";

export function useCopy() {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  function copy(text: string) {
    void copyText(text).then((ok) => {
      if (!ok) return;
      setCopied(true);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(false), 1200);
    });
  }
  return { copied, copy };
}
