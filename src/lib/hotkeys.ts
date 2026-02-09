import { register, unregisterAll } from '@tauri-apps/plugin-global-shortcut';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Settings } from './api';

export type HotkeyHandlers = {
  onToggle: () => Promise<void>;
  onPasteLast: () => Promise<void>;
};

// Keep this list generous: we validate before normalization, and users may have older saved values.
const modifierKeys = new Set([
  'CommandOrControl',
  'CmdOrControl',
  'CmdOrCtrl',
  'Command',
  'Cmd',
  'Control',
  'Ctrl',
  'Alt',
  'Option',
  'Shift',
  'Meta',
]);

const normalizeModifier = (value: string) => {
  const normalized = value.trim();
  const lower = normalized.toLowerCase();

  // Tauri global shortcut plugin expects CommandOrControl / CmdOrControl (not CmdOrCtrl).
  if (lower === 'cmdorctrl' || lower === 'cmdorcontrol' || lower === 'commandorcontrol') {
    return 'CommandOrControl';
  }

  if (lower === 'cmd' || lower === 'command' || lower === 'meta') return 'Command';
  if (lower === 'ctrl' || lower === 'control') return 'Control';
  if (lower === 'option') return 'Alt';

  return normalized;
};

export const normalizeHotkeyString = (value: string) =>
  value
    .split('+')
    .map((part) => {
      const normalized = part.trim();
      if (!normalized) return '';
      return normalizeModifier(normalized);
    })
    .filter(Boolean)
    .join('+');

const hasNonModifier = (value: string) => {
  const parts = value.split('+').map((part) => part.trim()).filter(Boolean);
  return parts.some((part) => !modifierKeys.has(part));
};

export const validateHotkeys = (settings: Settings): string | null => {
  const entries: Array<[string, string]> = [
    ['Record toggle', normalizeHotkeyString(settings.hotkeys.record_toggle)],
    ['Paste last', normalizeHotkeyString(settings.hotkeys.paste_last)],
    ['Open app', normalizeHotkeyString(settings.hotkeys.open_app)],
  ];

  for (const [label, combo] of entries) {
    if (!combo) {
      return `${label} hotkey is required.`;
    }
    if (!hasNonModifier(combo)) {
      return `${label} hotkey needs a non-modifier key.`;
    }
  }

  const map = new Map<string, string[]>();
  for (const [label, combo] of entries) {
    if (!map.has(combo)) map.set(combo, []);
    map.get(combo)?.push(label);
  }

  for (const [combo, labels] of map.entries()) {
    if (labels.length > 1) {
      return `Duplicate hotkeys detected (${combo}): ${labels.join(' and ')}.`;
    }
  }

  return null;
};

export const registerHotkeys = async (settings: Settings, handlers: HotkeyHandlers) => {
  const validationError = validateHotkeys(settings);
  if (validationError) {
    throw new Error(validationError);
  }

  await unregisterAll();

  await register(normalizeHotkeyString(settings.hotkeys.record_toggle), async (event) => {
    if (event.state !== 'Pressed') return;
    await handlers.onToggle();
  });

  await register(normalizeHotkeyString(settings.hotkeys.paste_last), async (event) => {
    if (event.state !== 'Pressed') return;
    await handlers.onPasteLast();
  });

  await register(normalizeHotkeyString(settings.hotkeys.open_app), async (event) => {
    if (event.state !== 'Pressed') return;
    const window = getCurrentWindow();
    await window.show();
    await window.setFocus();
  });
};
