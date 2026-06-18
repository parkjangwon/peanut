export async function copyTextToClipboard(text: string): Promise<void> {
  if (copyTextWithTextArea(text)) {
    return;
  }

  if (typeof navigator !== "undefined" && navigator.clipboard) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch (error) {
      throw normalizeCopyError(error);
    }
  }

  throw new Error("Clipboard copy is not available in this browser.");
}

export function copyTextWithTextArea(text: string) {
  if (typeof document === "undefined" || !document.body) {
    return false;
  }

  const selection = document.getSelection();
  const previousRange = selection && selection.rangeCount > 0 ? selection.getRangeAt(0).cloneRange() : null;
  const textArea = document.createElement("textarea");
  textArea.value = text;
  textArea.readOnly = true;
  textArea.setAttribute("aria-hidden", "true");
  textArea.style.position = "fixed";
  textArea.style.insetBlockStart = "0";
  textArea.style.insetInlineStart = "0";
  textArea.style.width = "1px";
  textArea.style.height = "1px";
  textArea.style.opacity = "0";

  document.body.appendChild(textArea);
  textArea.focus();
  textArea.select();

  try {
    return document.execCommand("copy");
  } finally {
    textArea.remove();
    if (selection && previousRange) {
      selection.removeAllRanges();
      selection.addRange(previousRange);
    }
  }
}

export function normalizeCopyError(error: unknown) {
  return error instanceof Error ? error : new Error("Clipboard copy failed.");
}
