<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { getCurrentWindow, LogicalPosition, LogicalSize } from '@tauri-apps/api/window';
  import { getRecordingLevel, getRecordingState, toggleRecording, type Settings } from '../api';

  type RecordingEvent = {
    recording: boolean;
    started_at_ms: number | null;
  };

  const hudWindow = getCurrentWindow();

  let recording = false;
  let startedAtMs: number | null = null;
  let seconds = 0;
  let stopping = false;

  let level = 0; // 0..1 (smoothed)
  let unlisten: UnlistenFn | null = null;
  let unlistenSettings: UnlistenFn | null = null;
  let timerInterval: number | null = null;
  let meterInterval: number | null = null;
  let hudEnabled = true;

  const clamp01 = (v: number) => Math.max(0, Math.min(1, v));

  const formatElapsed = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${s.toString().padStart(2, '0')}`;
  };

  const stopIntervals = () => {
    if (timerInterval) window.clearInterval(timerInterval);
    if (meterInterval) window.clearInterval(meterInterval);
    timerInterval = null;
    meterInterval = null;
  };

  const positionHudWindow = async () => {
    // Logical pixels (CSS pixels).
    const width = 412;
    const height = 64;
    const marginBottom = 16;

    const availLeft = (window.screen as any).availLeft ?? 0;
    const availTop = (window.screen as any).availTop ?? 0;
    const availWidth = window.screen.availWidth || window.innerWidth;
    const availHeight = window.screen.availHeight || window.innerHeight;

    const x = Math.round(availLeft + (availWidth - width) / 2);
    const y = Math.round(availTop + availHeight - height - marginBottom);

    try {
      await hudWindow.setSize(new LogicalSize(width, height));
      await hudWindow.setPosition(new LogicalPosition(x, y));
    } catch {
      // Best-effort; if the window API fails, we still render correctly.
    }
  };

  const applyRecordingState = async (nextRecording: boolean, nextStartedAtMs: number | null) => {
    if (nextRecording && !hudEnabled) {
      // If the user disabled the HUD, keep the window hidden even if recording is active.
      recording = false;
      startedAtMs = null;
      stopping = false;
      stopIntervals();
      seconds = 0;
      level = 0;
      hudWindow.hide().catch(() => {});
      return;
    }

    recording = nextRecording;
    startedAtMs = nextStartedAtMs;
    stopping = false;

    stopIntervals();

    if (!recording) {
      seconds = 0;
      level = 0;
      // Animate out before hiding the window.
      window.setTimeout(() => {
        hudWindow.hide().catch(() => {});
      }, 220);
      return;
    }

    await positionHudWindow();
    await hudWindow.show();

    // Timer
    seconds =
      typeof startedAtMs === 'number'
        ? Math.max(0, Math.floor((Date.now() - startedAtMs) / 1000))
        : 0;
    timerInterval = window.setInterval(() => {
      seconds += 1;
    }, 1000);

    // Meter polling (IPC is fine at this frequency).
    meterInterval = window.setInterval(async () => {
      const raw = await getRecordingLevel().catch(() => null);
      const target = clamp01(typeof raw === 'number' ? raw : 0);

      // Fast attack / slower release (feels like a real meter).
      const attack = 0.55;
      const release = 0.14;
      level = level < target ? level + (target - level) * attack : level + (target - level) * release;
    }, 80);
  };

  const handleStop = async () => {
    if (!recording || stopping) return;
    stopping = true;
    try {
      await toggleRecording();
      // Window hide is driven by the recording-state event.
    } catch {
      stopping = false;
    }
  };

  onMount(async () => {
    await positionHudWindow();

    unlisten = await listen<RecordingEvent>('recording-state', (event) => {
      applyRecordingState(event.payload.recording, event.payload.started_at_ms);
    });

    unlistenSettings = await listen<Settings>('settings-updated', (event) => {
      const next = Boolean(event.payload?.ui?.recording_hud_enabled);
      hudEnabled = next;
      if (!hudEnabled) {
        applyRecordingState(false, null);
      }
    });

    // If recording was started before the HUD webview finished initializing,
    // sync state immediately so we still show.
    const state = await getRecordingState().catch(() => null);
    if (state) {
      hudEnabled = Boolean(state.hud_enabled);
    }
    if (state?.recording && hudEnabled) {
      applyRecordingState(true, state.started_at_ms);
    }
  });

  onDestroy(() => {
    stopIntervals();
    if (unlisten) unlisten();
    if (unlistenSettings) unlistenSettings();
  });
</script>

  <div class={`hud ${recording ? 'is-open' : ''} ${stopping ? 'is-stopping' : ''}`} role="status" aria-live="polite">
    <div class="hud-shell">
    <div class="hud-left">
      <div class="rec-dot" aria-hidden="true"></div>
      <div class="hud-text">
        <div class="hud-label">{stopping ? 'Stopping…' : 'Recording'}</div>
        <div class="hud-time">{formatElapsed(seconds)}</div>
      </div>
    </div>

    <div class="hud-meter" aria-hidden="true">
      {#each Array.from({ length: 14 }) as _, i}
        <div
          class="meter-bar"
          style={`--h:${Math.max(0.15, Math.min(1, level * 1.15 + Math.sin((i / 14) * Math.PI * 2) * 0.08 + 0.08))}`}
        ></div>
      {/each}
    </div>

    <button class="hud-stop" type="button" on:click={handleStop} disabled={!recording || stopping} aria-label="Stop recording">
      <span class="stop-icon" aria-hidden="true"></span>
      <span class="stop-text">{stopping ? 'Stopping' : 'Stop'}</span>
    </button>
  </div>
</div>

<style>
  .hud {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    pointer-events: none;
    overflow: hidden;
  }

  .hud-shell {
    pointer-events: auto;
    width: calc(100% - 0px);
    height: calc(100% - 0px);
    border-radius: 999px;
    padding: 9px 11px;
    display: grid;
    grid-template-columns: 1fr auto auto;
    align-items: center;
    gap: 12px;

    background: rgba(0, 0, 0, 0.92);
    box-shadow:
      0 18px 44px rgba(0, 0, 0, 0.45),
      0 6px 18px rgba(0, 0, 0, 0.35);
    border: 1px solid rgba(255, 255, 255, 0.08);

    transform: translateY(8px);
    opacity: 0;
    transition: transform 180ms ease, opacity 160ms ease;
  }

  .is-open .hud-shell {
    transform: translateY(0px);
    opacity: 1;
  }

  .hud-left {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .rec-dot {
    width: 10px;
    height: 10px;
    border-radius: 999px;
    background: #e36b70;
    box-shadow:
      0 0 0 2px rgba(227, 107, 112, 0.22),
      0 0 18px rgba(227, 107, 112, 0.25);
    animation: recPulse 1.35s ease-in-out infinite;
  }

  .is-stopping .rec-dot {
    animation: none;
    opacity: 0.6;
  }

  @keyframes recPulse {
    0% { transform: scale(1); opacity: 0.92; }
    55% { transform: scale(1.18); opacity: 1; }
    100% { transform: scale(1); opacity: 0.92; }
  }

  .hud-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .hud-label {
    color: rgba(255, 255, 255, 0.96);
    font-size: 12px;
    font-weight: 650;
    letter-spacing: 0.02em;
    line-height: 1.1;
  }

  .hud-time {
    color: rgba(216, 216, 218, 0.95);
    font-family: "IBM Plex Mono", "Fira Mono", ui-monospace, monospace;
    font-size: 11px;
    letter-spacing: 0.01em;
    line-height: 1.1;
  }

  .hud-meter {
    display: flex;
    align-items: flex-end;
    gap: 4px;
    height: 30px;
    padding: 0 4px 0 0;
  }

  .meter-bar {
    width: 4px;
    height: calc(8px + (22px * var(--h)));
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.92);
    transform-origin: bottom;
    transition: height 80ms linear, background-color 140ms ease;
  }

  .is-stopping .meter-bar {
    background: rgba(255, 255, 255, 0.55);
  }

  .hud-stop {
    height: 36px;
    border-radius: 999px;
    padding: 0 14px 0 12px;
    display: inline-flex;
    align-items: center;
    gap: 10px;

    background: #e36b70;
    color: #0b0b0d;
    font-weight: 700;
    letter-spacing: 0.01em;
    border: 1px solid rgba(0, 0, 0, 0.25);

    transition: transform 120ms ease, filter 120ms ease, opacity 120ms ease;
  }

  .hud-stop:hover:not(:disabled) {
    filter: brightness(1.03);
    transform: translateY(-0.5px);
  }

  .hud-stop:active:not(:disabled) {
    transform: translateY(0.5px);
  }

  .hud-stop:disabled {
    opacity: 0.65;
    cursor: not-allowed;
  }

  .stop-icon {
    width: 12px;
    height: 12px;
    border-radius: 3px;
    background: rgba(0, 0, 0, 0.72);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.08);
  }

  .stop-text {
    font-size: 12px;
  }
</style>
