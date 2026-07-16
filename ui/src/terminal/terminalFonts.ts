import "./terminalFonts.css";

const TERMINAL_WEB_FONT = "Wakey JetBrainsMono Nerd Font Mono";
const TERMINAL_FALLBACK_FONTS =
  '"SFMono-Regular", Consolas, "Liberation Mono", monospace';

let terminalFontsPromise: Promise<string> | undefined;

/**
 * Resolves the terminal font stack only after browser glyph metrics are ready.
 * xterm caches those metrics when it opens, so a late webfont load permanently
 * misaligns cells for that terminal instance.
 */
export function loadTerminalFontFamily(): Promise<string> {
  terminalFontsPromise ??= document.fonts
    .load(`400 14px "${TERMINAL_WEB_FONT}"`, "\uE0B0")
    .then((faces) => {
      if (faces.length === 0) {
        throw new Error("terminal webfont was not registered");
      }
      return `"${TERMINAL_WEB_FONT}", ${TERMINAL_FALLBACK_FONTS}`;
    })
    .catch((error: unknown) => {
      console.warn(
        "Could not load terminal webfont; using system monospace",
        error,
      );
      return TERMINAL_FALLBACK_FONTS;
    });
  return terminalFontsPromise;
}
