// Clipboard access from the webview. The async Clipboard API needs a user gesture (a click
// or key press counts); older WebKitGTK builds may lack it, so writing falls back to a
// hidden textarea and execCommand.

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    return ok;
  }
}

export async function readText(): Promise<string> {
  try {
    return await navigator.clipboard.readText();
  } catch {
    return "";
  }
}

/** Terminal output as plain text: colour and cursor codes removed, CRLF folded. */
export function plainText(output: string): string {
  return output
    .replace(/\x1b\][^\x07\x1b]*(\x07|\x1b\\)/g, "")
    .replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "")
    .replace(/\x1b[()][0-9A-Za-z]/g, "")
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "");
}
