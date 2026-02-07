// File: src/shared/lib/clipboard.ts
// Clipboard utilities with fallback for non-secure contexts

/**
 * Copy text to clipboard with fallback for HTTP environments
 */
export async function copyToClipboard(text: string): Promise<boolean> {
  // Try modern Clipboard API first
  if (navigator.clipboard && window.isSecureContext) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch (err) {
      console.error('[Clipboard] API copy failed:', err);
    }
  }

  // Fallback to execCommand for non-secure contexts
  try {
    const textArea = document.createElement('textarea');
    textArea.value = text;

    textArea.style.position = 'fixed';
    textArea.style.left = '-9999px';
    textArea.style.top = '0';
    document.body.appendChild(textArea);

    textArea.focus();
    textArea.select();

    const successful = document.execCommand('copy');
    document.body.removeChild(textArea);

    return successful;
  } catch (err) {
    console.error('[Clipboard] execCommand copy failed:', err);
    return false;
  }
}
