<script lang="ts">
  export let value: string;
  export let id: string = '';
  // `runtimeInfo.session_type` from backend (`macos`, `windows`, `x11`, `wayland`, `unknown`).
  // Used only for display and capture ergonomics.
  export let platform: string = 'unknown';

  let capturing = false;

  function formatHotkey(event: KeyboardEvent): string {
    const parts: string[] = [];

    // Order: Ctrl/Cmd, Alt, Shift, Key
    // On macOS, Cmd is `metaKey` and Control is `ctrlKey`.
    // Don't collapse them into CommandOrControl, otherwise Ctrl+… becomes Cmd+… on macOS.
    if (platform === 'macos') {
      if (event.metaKey) parts.push('CommandOrControl');
      if (event.ctrlKey) parts.push('Control');
    } else {
      if (event.ctrlKey || event.metaKey) parts.push('CommandOrControl');
    }
    if (event.altKey) parts.push('Alt');
    if (event.shiftKey) parts.push('Shift');

    // Get the key itself (excluding modifier keys)
    const key = event.key;
    if (!['Control', 'Alt', 'Shift', 'Meta'].includes(key)) {
      const normalizedKey = normalizeKey(key);
      if (normalizedKey) parts.push(normalizedKey);
    }

    return parts.join('+');
  }

  function normalizeKey(key: string): string {
    const keyMap: Record<string, string> = {
      ' ': 'Space',
      Space: 'Space',
      Spacebar: 'Space',
      ArrowUp: 'Up',
      ArrowDown: 'Down',
      ArrowLeft: 'Left',
      ArrowRight: 'Right',
      Escape: 'Escape',
      Enter: 'Enter',
      Tab: 'Tab',
      Backspace: 'Backspace',
      Delete: 'Delete',
      Home: 'Home',
      End: 'End',
      PageUp: 'PageUp',
      PageDown: 'PageDown',
    };
    return keyMap[key] || (key.length === 1 ? key.toUpperCase() : key);
  }

  function displayFormat(hotkeyStr: string): string {
    const isMac = platform === 'macos';

    // Convert internal modifier names to a more readable format.
    // Note: on macOS, CommandOrControl means Command (Cmd).
    return hotkeyStr
      .replaceAll('CommandOrControl', isMac ? 'Cmd' : 'Ctrl')
      .replaceAll('CmdOrControl', isMac ? 'Cmd' : 'Ctrl')
      .replaceAll('CmdOrCtrl', isMac ? 'Cmd' : 'Ctrl')
      .replaceAll('Command', isMac ? 'Cmd' : 'Meta')
      .replaceAll('Control', 'Ctrl')
      .replaceAll('Alt', isMac ? 'Option' : 'Alt');
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!capturing) return;

    event.preventDefault();
    event.stopPropagation();

    // Only capture if we have a non-modifier key
    if (!['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) {
      value = formatHotkey(event);
      capturing = false;
    }
  }

  function startCapture() {
    capturing = true;
  }

  function handleBlur() {
    capturing = false;
  }
</script>

<svelte:window on:keydown|capture={handleKeydown} on:blur={handleBlur} />

<button
  type="button"
  {id}
  class="hotkey-input"
  class:capturing
  on:click={startCapture}
  on:blur={handleBlur}
  aria-label={capturing ? 'Press key combination' : `Hotkey: ${value}`}
>
  {#if capturing}
    <span class="hotkey-prompt">Press keys...</span>
  {:else}
    <kbd class="hotkey-display">{value ? displayFormat(value) : 'Not set'}</kbd>
  {/if}
</button>
