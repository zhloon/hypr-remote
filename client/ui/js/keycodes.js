/**
 * Maps Web KeyboardEvent.code strings to Linux kernel input-event-codes (linux/input.h).
 * This allows the Hyprland host (/dev/uinput) to directly consume standard scancodes.
 */
export const WEB_TO_LINUX_KEYMAP = {
  // Letters
  "KeyA": 30, "KeyB": 48, "KeyC": 46, "KeyD": 32, "KeyE": 18,
  "KeyF": 33, "KeyG": 34, "KeyH": 35, "KeyI": 23, "KeyJ": 36,
  "KeyK": 37, "KeyL": 38, "KeyM": 50, "KeyN": 49, "KeyO": 24,
  "KeyP": 25, "KeyQ": 16, "KeyR": 19, "KeyS": 31, "KeyT": 20,
  "KeyU": 22, "KeyV": 47, "KeyW": 17, "KeyX": 45, "KeyY": 21,
  "KeyZ": 44,

  // Numbers (Top Row)
  "Digit1": 2, "Digit2": 3, "Digit3": 4, "Digit4": 5, "Digit5": 6,
  "Digit6": 7, "Digit7": 8, "Digit8": 9, "Digit9": 10, "Digit0": 11,
  "Minus": 12, "Equal": 13, "Backspace": 14,

  // Modifiers (Crucial for Hyprland: Meta/Super = 125)
  "MetaLeft": 125,     // Windows / Command / Super
  "MetaRight": 126,    // Right Windows / Command
  "ControlLeft": 29,
  "ControlRight": 97,
  "ShiftLeft": 42,
  "ShiftRight": 54,
  "AltLeft": 56,
  "AltRight": 100,
  "CapsLock": 58,

  // Navigation & Control
  "Escape": 1,
  "Tab": 15,
  "Enter": 28,
  "Space": 57,
  "ArrowUp": 103,
  "ArrowDown": 108,
  "ArrowLeft": 105,
  "ArrowRight": 106,
  "Insert": 110,
  "Delete": 111,
  "Home": 102,
  "End": 107,
  "PageUp": 104,
  "PageDown": 109,

  // Symbols
  "BracketLeft": 26,
  "BracketRight": 27,
  "Backslash": 43,
  "Semicolon": 39,
  "Quote": 40,
  "Backquote": 41,
  "Comma": 51,
  "Period": 52,
  "Slash": 53,

  // Function Keys
  "F1": 59, "F2": 60, "F3": 61, "F4": 62, "F5": 63, "F6": 64,
  "F7": 65, "F8": 66, "F9": 67, "F10": 68, "F11": 87, "F12": 88,

  // Numpad & Special
  "Numpad0": 82, "Numpad1": 79, "Numpad2": 80, "Numpad3": 81,
  "Numpad4": 75, "Numpad5": 76, "Numpad6": 77, "Numpad7": 71,
  "Numpad8": 72, "Numpad9": 73, "NumpadDecimal": 83,
  "NumpadDivide": 98, "NumpadMultiply": 55, "NumpadSubtract": 74,
  "NumpadAdd": 78, "NumpadEnter": 96,
  "NumLock": 69,
  "PrintScreen": 99,
  "ScrollLock": 70,
  "Pause": 119,
  "ContextMenu": 127,
  "IntlBackslash": 86
};

