<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import type { AudioDevice, RuntimeInfo, Settings } from '../api';
  import HotkeyInput from './HotkeyInput.svelte';
  import { onboardingSteps as steps } from '../onboarding';

  export let open = false;
  export let step = 0;
  export let settings: Settings | null = null;
  export let runtimeInfo: RuntimeInfo | null = null;
  export let audioDevices: AudioDevice[] = [];
  export let errorMessage = '';

  const dispatch = createEventDispatcher<{
    back: void;
    skipStep: void;
    skipAll: void;
    finish: void;
    advance: {
      step: number;
      hotkeys?: { record_toggle: string; paste_last: string; open_app: string };
      automation?: {
        auto_paste_enabled: boolean;
        paste_delay_ms: number;
        copy_to_clipboard: boolean;
        preserve_clipboard: boolean;
        paste_method: string;
      };
    };
    selectAudioDevice: { id: string };
  }>();

  const totalSteps = steps.length;

  let initializedStep: number | null = null;

  let selectedInputDeviceId = 'default';
  let recordHotkey = 'Ctrl+Alt+Space';
  let pasteHotkey = 'Ctrl+Alt+V';
  let openHotkey = 'Ctrl+Alt+O';

  let autoPasteEnabled = false;
  let copyToClipboard = true;
  let preserveClipboard = false;
  let pasteDelayMs = 0;
  let pasteMethod = 'auto';

  const handleAudioDeviceSelectChange = () => {
    // Avoid dispatching bogus values when no devices have been detected yet.
    if (!selectedInputDeviceId) return;
    if (audioDevices.length === 0) return;
    dispatch('selectAudioDevice', { id: selectedInputDeviceId });
  };

  $: if (open && settings && initializedStep !== step) {
    if (step === 1) {
      selectedInputDeviceId = settings.audio.input_device_id || 'default';
    } else if (step === 2) {
      recordHotkey = settings.hotkeys.record_toggle;
      pasteHotkey = settings.hotkeys.paste_last;
      openHotkey = settings.hotkeys.open_app;
    } else if (step === 3) {
      autoPasteEnabled = settings.automation.auto_paste_enabled;
      copyToClipboard = settings.automation.copy_to_clipboard;
      preserveClipboard = settings.automation.preserve_clipboard;
      pasteDelayMs = settings.automation.paste_delay_ms;
      pasteMethod = settings.automation.paste_method;
    }
    initializedStep = step;
  }

  // Keep the select in a sane state as devices load/unload while the onboarding step is visible.
  $: if (open && step === 1) {
    if (audioDevices.length === 0) {
      selectedInputDeviceId = '';
    } else {
      const allowed = new Set(['default', ...audioDevices.map((d) => d.id)]);
      if (!allowed.has(selectedInputDeviceId)) {
        selectedInputDeviceId = settings?.audio.input_device_id || 'default';
        if (!allowed.has(selectedInputDeviceId)) selectedInputDeviceId = 'default';
      }
    }
  }

  const handleNext = () => {
    if (step === 2) {
      dispatch('advance', {
        step,
        hotkeys: {
          record_toggle: recordHotkey,
          paste_last: pasteHotkey,
          open_app: openHotkey,
        },
      });
      return;
    }
    if (step === 3) {
      dispatch('advance', {
        step,
        automation: {
          auto_paste_enabled: autoPasteEnabled,
          paste_delay_ms: pasteDelayMs,
          copy_to_clipboard: copyToClipboard,
          preserve_clipboard: preserveClipboard,
          paste_method: pasteMethod,
        },
      });
      return;
    }
    dispatch('advance', { step });
  };
</script>

{#if open}
  <div class="onboard-backdrop" role="presentation">
    <div class="onboard-card" role="dialog" aria-modal="true" aria-labelledby="onboard-title">
      <header class="onboard-header">
        <div>
          <p class="onboard-kicker">Quick Start</p>
          <h2 id="onboard-title">{steps[step].title}</h2>
          <p class="onboard-subtitle">{steps[step].subtitle}</p>
        </div>
        <button class="onboard-skip" type="button" on:click={() => dispatch('skipAll')}>
          Skip tour
        </button>
      </header>

      {#if errorMessage}
        <div class="banner error" role="alert">
          <span>{errorMessage}</span>
        </div>
      {/if}

      <div class="onboard-progress">
        <span>Step {step + 1} of {totalSteps}</span>
        <div class="onboard-dots">
          {#each Array(totalSteps) as _, index}
            <span class={`onboard-dot ${index === step ? 'active' : ''}`}></span>
          {/each}
        </div>
      </div>

      <div class="onboard-body">
        {#key step}
          <div class="onboard-step">
            {#if step === 0}
              <div class="onboard-grid">
                <div>
                  <h3>What Whispr does</h3>
                  <ul>
                    <li>Record audio with a single shortcut.</li>
                    <li>Transcribe locally on your device.</li>
                    <li>Copy and format your notes instantly.</li>
                  </ul>
                </div>
                <div class="onboard-example">
                  <p class="onboard-example-title">Example transcript</p>
                  <p>“Meeting Notes: [Topic] - [Date]”</p>
                  <p>“Action items: send invoice, schedule a follow‑up.”</p>
                </div>
              </div>
            {:else if step === 1}
              <div class="onboard-grid">
                <div>
                  <h3>Recommended setup</h3>
                  <p>Use a headset or an external mic when possible.</p>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="onboard-input-device">Input device</label>
                      <p class="settings-hint">This is used for recording.</p>
                    </div>
                    <div class="settings-control">
                      <select
                        id="onboard-input-device"
                        class="select-wide"
                        disabled={audioDevices.length === 0}
                        bind:value={selectedInputDeviceId}
                        on:change={handleAudioDeviceSelectChange}
                      >
                        {#if audioDevices.length === 0}
                          <option value="">No devices found</option>
                        {:else}
                          <option value="default">Use system default</option>
                          {#each audioDevices as device}
                            <option value={device.id}>{device.name}{device.is_default ? ' (default)' : ''}</option>
                          {/each}
                        {/if}
                      </select>
                    </div>
                  </div>
                </div>
                <div class="onboard-example">
                  <p class="onboard-example-title">Detected devices</p>
                  {#if audioDevices.length > 0}
                    <ul>
                      {#each audioDevices.slice(0, 4) as device}
                        <li>{device.name}{device.is_default ? ' (default)' : ''}</li>
                      {/each}
                    </ul>
                  {:else}
                    <p>No devices detected.</p>
                  {/if}
                </div>
              </div>
            {:else if step === 2}
              <div class="onboard-grid">
                <div>
                  <h3>Speed with hotkeys</h3>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="onboard-record-hotkey">Record toggle</label>
                    </div>
                    <div class="settings-control">
                      <HotkeyInput id="onboard-record-hotkey" bind:value={recordHotkey} />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="onboard-paste-hotkey">Paste last</label>
                    </div>
                    <div class="settings-control">
                      <HotkeyInput id="onboard-paste-hotkey" bind:value={pasteHotkey} />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="onboard-open-hotkey">Open app</label>
                    </div>
                    <div class="settings-control">
                      <HotkeyInput id="onboard-open-hotkey" bind:value={openHotkey} />
                    </div>
                  </div>
                </div>
                <div class="onboard-example">
                  <p class="onboard-example-title">System note</p>
                  {#if runtimeInfo?.session_type === 'wayland'}
                    <p>Wayland limits hotkeys and paste helpers on some systems.</p>
                  {:else if runtimeInfo?.session_type === 'windows'}
                    <p>Windows tray supports click‑to‑record and quick access.</p>
                  {:else}
                    <p>Shortcuts work best on X11 and Windows.</p>
                  {/if}
                </div>
              </div>
            {:else}
              <div class="onboard-grid">
                <div>
                  <h3>Make it yours</h3>
                  <p>Pick copy formats: plain, markdown, or bullet list.</p>
                  <p>Enable auto‑paste for instant delivery.</p>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="onboard-auto-paste">Auto-paste</label>
                      <p class="settings-hint">Automatically paste after transcription</p>
                    </div>
                    <div class="settings-control">
                      <input id="onboard-auto-paste" type="checkbox" bind:checked={autoPasteEnabled} />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="onboard-copy-to-clipboard">Copy to clipboard</label>
                      <p class="settings-hint">Copy output to clipboard automatically</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="onboard-copy-to-clipboard"
                        type="checkbox"
                        bind:checked={copyToClipboard}
                        on:change={() => {
                          if (copyToClipboard) preserveClipboard = false;
                        }}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="onboard-preserve-clipboard">Preserve clipboard contents</label>
                      <p class="settings-hint">Auto-paste without leaving the transcript in your clipboard</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="onboard-preserve-clipboard"
                        type="checkbox"
                        bind:checked={preserveClipboard}
                        disabled={copyToClipboard}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="onboard-paste-delay">Paste delay (ms)</label>
                      <p class="settings-hint">Delay before pasting transcription</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="onboard-paste-delay"
                        class="input-compact"
                        type="number"
                        min="0"
                        step="50"
                        bind:value={pasteDelayMs}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="onboard-paste-method">Paste method</label>
                      <p class="settings-hint">Override auto detection when needed.</p>
                    </div>
                    <div class="settings-control">
                      <select id="onboard-paste-method" class="select-compact" bind:value={pasteMethod}>
                        <option value="auto">Auto</option>
                        <option value="x11_ctrl_v">Keyboard paste (Ctrl/Cmd+V)</option>
                        <option value="wayland_wtype">Wayland (wtype)</option>
                        <option value="wayland_ydotool">Wayland (ydotool)</option>
                        <option value="clipboard_only">Clipboard only</option>
                      </select>
                    </div>
                  </div>
                </div>
                <div class="onboard-example">
                  <p class="onboard-example-title">Quick workflow</p>
                  <ol>
                    <li>Record</li>
                    <li>Preview</li>
                    <li>Copy or paste</li>
                  </ol>
                </div>
              </div>
            {/if}
          </div>
        {/key}
      </div>

      <footer class="onboard-footer">
        <div class="onboard-actions">
          <button class="btn-secondary" type="button" on:click={() => dispatch('skipStep')}>
            Skip step
          </button>
          <button class="btn-secondary" type="button" on:click={() => dispatch('back')} disabled={step === 0}>
            Back
          </button>
        </div>
        <div class="onboard-actions">
          {#if step < totalSteps - 1}
            <button class="btn-primary" type="button" on:click={handleNext}>
              Next
            </button>
          {:else}
            <button class="btn-primary" type="button" on:click={() => dispatch('finish')}>
              Finish setup
            </button>
          {/if}
        </div>
      </footer>
    </div>
  </div>
{/if}
