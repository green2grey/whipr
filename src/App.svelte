<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { open, save } from '@tauri-apps/plugin-dialog';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { check } from '@tauri-apps/plugin-updater';
  import { relaunch } from '@tauri-apps/plugin-process';
  import {
    activateModel,
    copyText,
    deleteModel,
    deleteTranscript,
    downloadModel,
    exportTranscript,
    getSettings,
    getStorageStats,
    getRuntimeInfo,
    getPerformanceInfo,
    benchmarkTranscription,
    clearTranscripts,
    listAudioDevices,
    listModels,
    listTranscripts,
    searchTranscripts,
    importAudioFiles,
    pasteLastTranscript,
    saveSettings,
    setUiActive,
    setAudioInputDevice,
    toggleRecording,
    listClips,
    createClip,
    deleteClip,
    updateTranscript,
    type AudioDevice,
    type BenchmarkResult,
    type Clip,
    type ImportFailure,
    type ModelInfo,
    type PerformanceInfo,
    type RuntimeInfo,
    type StorageStats,
    type Settings,
    type Transcript,
    type TranscriptUpdate,
  } from './lib/api';
  import { normalizeHotkeyString, registerHotkeys, validateHotkeys } from './lib/hotkeys';
  import { theme, type ThemePreference } from './lib/theme';
  import ConfirmDialog from './lib/components/ConfirmDialog.svelte';
  import EmptyState from './lib/components/EmptyState.svelte';
  import HotkeyInput from './lib/components/HotkeyInput.svelte';
  import Onboarding from './lib/components/Onboarding.svelte';
  import { onboardingSteps } from './lib/onboarding';
  import transcriptionCompleteSoundUrl from './lib/assets/sounds/transcription-complete.mp3?url';

  let currentPage: 'home' | 'settings' | 'clips' = 'home';
  let search = '';
  let semanticSearchEnabled = false;
  let searchLoading = false;
  let semanticResults: Transcript[] = [];
  let searchTimer: number | null = null;
  let copiedId: string | null = null;
  let clipCopiedId: string | null = null;
  let clipCopyTimer: number | null = null;
  let settings: Settings | null = null;
  let runtimeInfo: RuntimeInfo | null = null;
  let transcripts: Transcript[] = [];
  let baseTranscripts: Transcript[] = [];
  let clips: Clip[] = [];
  let models: ModelInfo[] = [];
  let audioDevices: AudioDevice[] = [];
  let isRecording = false;
  let loading = true;
  let savingSettings = false;
  let errorMessage = '';
  let hotkeyWarning = '';
  const WAYLAND_HOTKEYS_WARNING = 'Global hotkeys are not supported on Wayland.';
  const WAYLAND_HOTKEYS_WARNING_DISMISSED_KEY = 'whispr.wayland_hotkeys_warning.dismissed';
  let waylandHotkeysWarningDismissed = false;
  let modelBusyId: string | null = null;
  let recordingSeconds = 0;
  let timerInterval: number | null = null;
  let unlistenRecording: UnlistenFn | null = null;
  let unlistenTranscript: UnlistenFn | null = null;
  let unlistenOpenSettings: UnlistenFn | null = null;
  let unlistenSettingsUpdated: UnlistenFn | null = null;
  let unlistenPreview: UnlistenFn | null = null;
  let unlistenModelProgress: UnlistenFn | null = null;
  let unlistenImportProgress: UnlistenFn | null = null;
  let unlistenAutomationError: UnlistenFn | null = null;
  let unlistenTranscriptionStarted: UnlistenFn | null = null;
  let deleteConfirmModel: ModelInfo | null = null;
  let clearConfirmOpen = false;
  let clearingTranscripts = false;
  let importing = false;
  let importProgress: { index: number; total: number; path: string } | null = null;
  let importFailures: ImportFailure[] = [];
  let expandedTranscript: Transcript | null = null;
  let detailDraft = '';
  let detailTextareaEl: HTMLTextAreaElement | null = null;
  let detailCopied = false;
  let detailCopyTimer: number | null = null;
  let detailDirty = false;
  let clipSavedId: string | null = null;
  let previewText = '';
  let updateAvailable: { version: string; update: Awaited<ReturnType<typeof check>> } | null = null;
  let updateDismissed = false;
  let updateDownloading = false;
  // Percent [0..100] shown in the UI.
  let updateProgress = 0;
  // Byte counts used to compute updateProgress.
  let updateProgressTotal = 0;
  let updateProgressDownloaded = 0;
  let currentVersion = '';
  let storageStats: StorageStats | null = null;
  let performanceInfo: PerformanceInfo | null = null;
  let benchmarkResult: BenchmarkResult | null = null;
  let benchmarking = false;
  let benchmarkError = '';
  let gpuErrorMessage = '';
  let gpuErrorDismissed = '';
  let activeTagFilters: string[] = [];
  let copyFormat: 'plain' | 'markdown' | 'bullets' = 'plain';
  let modelProgress: Record<string, { downloaded: number; total: number }> = {};
  let onboardingOpen = false;
  let onboardingStep = 0;
  let onboardingErrorMessage = '';
  let settingsFocus: 'audio' | 'hotkeys' | 'automation' | 'app' | null = null;
  let settingsFocusTimer: number | null = null;
  let audioDeviceSaveInFlight = false;
  let audioSectionEl: HTMLDivElement | null = null;
  let hotkeysSectionEl: HTMLDivElement | null = null;
  let automationSectionEl: HTMLDivElement | null = null;
  let appSectionEl: HTMLDivElement | null = null;
  let dateFilter: 'all' | '7d' | '30d' | '90d' = 'all';
  let deleteConfirmTranscript: Transcript | null = null;
  let savingTranscript = false;
  let transcriptSaved = false;
  let transcriptSaveTimer: number | null = null;
  let updateCheckTimer: number | null = null;
  let wlClipboardInstallOpen = false;
  let wlClipboardCopyError = '';

  let transcriptionCompleteAudio: HTMLAudioElement | null = null;
  let lastTranscriptionSoundAt = 0;

  const playTranscriptionCompleteSound = () => {
    if (!transcriptionCompleteAudio) return;
    const now = Date.now();
    // Prevent double-play if multiple triggers arrive close together.
    if (now - lastTranscriptionSoundAt < 1200) return;
    lastTranscriptionSoundAt = now;
    try {
      transcriptionCompleteAudio.currentTime = 0;
    } catch {}
    transcriptionCompleteAudio.play().catch(() => {});
  };

  type NavState = { page: 'home' | 'settings' | 'clips' };
  const appWindow = getCurrentWindow();

  const isNavState = (value: unknown): value is NavState => {
    if (!value || typeof value !== 'object') return false;
    const state = value as { page?: unknown };
    return state.page === 'home' || state.page === 'settings' || state.page === 'clips';
  };

  const navigateTo = (page: NavState['page'], replace = false) => {
    currentPage = page;
    const state: NavState = { page };
    if (replace) {
      window.history.replaceState(state, '');
    } else {
      window.history.pushState(state, '');
    }
  };

  const handleNavBack = () => window.history.back();
  const handleNavForward = () => window.history.forward();

  const handleWindowClose = () => {
    appWindow.close().catch(() => {});
  };

  const handleWindowMinimize = () => {
    appWindow.minimize().catch(() => {});
  };

  const handleWindowToggleMaximize = () => {
    appWindow.toggleMaximize().catch(() => {});
  };

  const handleTitlebarMouseDown = (event: MouseEvent) => {
    // Fallback for environments where drag-region hit testing is unreliable.
    if (event.button !== 0) return;
    const target = event.target as HTMLElement | null;
    if (!target) return;
    if (target.closest('button, a, input, textarea, select, [role="button"]')) return;
    appWindow.startDragging().catch(() => {});
  };

  const tagOptions = ['Meeting', 'Task', 'Personal'];
  const onboardingTotal = onboardingSteps.length;

  const wlClipboardInstallCommands = [
    { label: 'Debian/Ubuntu', command: 'sudo apt install wl-clipboard' },
    { label: 'Fedora', command: 'sudo dnf install wl-clipboard' },
    { label: 'Arch', command: 'sudo pacman -S wl-clipboard' },
    { label: 'openSUSE', command: 'sudo zypper install wl-clipboard' },
  ] as const;

  const closeWlClipboardInstall = () => {
    wlClipboardInstallOpen = false;
    wlClipboardCopyError = '';
  };

  const copyInstallCommand = async (command: string) => {
    wlClipboardCopyError = '';
    try {
      await navigator.clipboard.writeText(command);
    } catch (error) {
      wlClipboardCopyError = error instanceof Error
        ? `Copy failed: ${error.message}`
        : 'Copy failed.';
    }
  };

  const formatTimestamp = (ms: number) => {
    const date = new Date(ms);
    return date.toLocaleString('en-US', {
      month: 'short',
      day: 'numeric',
      year: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
      hour12: true,
    });
  };

  const formatRailDate = (ms: number) => {
    const date = new Date(ms);
    return date.toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
    });
  };

  const formatRailTime = (ms: number) => {
    const date = new Date(ms);
    return date.toLocaleTimeString('en-US', {
      hour: 'numeric',
      minute: '2-digit',
      hour12: true,
    });
  };

  const formatDuration = (ms: number) => {
    const seconds = Math.max(1, Math.round(ms / 1000));
    return `${seconds}s`;
  };

  const formatElapsed = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${s.toString().padStart(2, '0')}`;
  };

  const startTimer = (initialSeconds = 0) => {
    if (timerInterval) {
      clearInterval(timerInterval);
    }
    recordingSeconds = initialSeconds;
    timerInterval = window.setInterval(() => {
      recordingSeconds += 1;
    }, 1000);
  };

  const stopTimer = () => {
    if (timerInterval) {
      clearInterval(timerInterval);
      timerInterval = null;
    }
    recordingSeconds = 0;
  };

  type RecordingEvent = {
    recording: boolean;
    started_at_ms: number | null;
  };

  type PreviewEvent = {
    text: string;
  };

  type ModelProgressEvent = {
    id: string;
    downloaded: number;
    total: number;
  };

  type ImportProgressEvent = {
    index: number;
    total: number;
    path: string;
  };

  type AutomationErrorEvent = {
    message: string;
  };

  const applyRecordingState = (recording: boolean, startedAtMs: number | null) => {
    if (recording) {
      const elapsedSeconds = typeof startedAtMs === 'number'
        ? Math.max(0, Math.floor((Date.now() - startedAtMs) / 1000))
        : 0;
      isRecording = true;
      startTimer(elapsedSeconds);
      return;
    }

    isRecording = false;
    stopTimer();
    previewText = '';
  };

  const startRecordingListener = async () => {
    unlistenRecording = await listen<RecordingEvent>('recording-state', (event) => {
      applyRecordingState(event.payload.recording, event.payload.started_at_ms);
    });
  };

  const startTranscriptListener = async () => {
    unlistenTranscript = await listen<Transcript>('transcript-created', (event) => {
      const incoming = event.payload;
      if (!incoming) return;
      if (transcripts.some((item) => item.id === incoming.id)) return;
      transcripts = [incoming, ...transcripts];
      getStorageStats()
        .then((stats) => {
          storageStats = stats;
        })
        .catch(() => {});
    });
  };

  const startPreviewListener = async () => {
    unlistenPreview = await listen<PreviewEvent>('transcript-preview', (event) => {
      previewText = event.payload.text ?? '';
    });
  };

  let lastUiActive: boolean | null = null;
  const computeUiActive = () => {
    if (document.visibilityState !== 'visible') return false;
    if (typeof document.hasFocus === 'function') return document.hasFocus();
    return true;
  };

  const syncUiActive = async () => {
    const next = computeUiActive();
    if (lastUiActive === next) return;
    lastUiActive = next;
    try {
      await setUiActive(next);
    } catch {
      // Best-effort; preview will fall back to always-on behavior if the backend can't track UI.
    }
  };

  const startModelProgressListener = async () => {
    unlistenModelProgress = await listen<ModelProgressEvent>('model-download-progress', (event) => {
      const { id, downloaded, total } = event.payload;
      modelProgress = {
        ...modelProgress,
        [id]: { downloaded, total },
      };
    });
  };

  const startImportProgressListener = async () => {
    unlistenImportProgress = await listen<ImportProgressEvent>('import-progress', (event) => {
      importProgress = event.payload;
    });
  };

  const startOpenSettingsListener = async () => {
    unlistenOpenSettings = await listen('open-settings', () => {
      navigateTo('settings');
    });
  };

  const startSettingsUpdatedListener = async () => {
    unlistenSettingsUpdated = await listen<Settings>('settings-updated', (event) => {
      const updated = event.payload;
      if (!updated) return;
      if (settings) {
        settings = {
          ...settings,
          app: {
            ...settings.app,
            close_to_tray: updated.app.close_to_tray,
          },
        };
      } else {
        settings = updated;
      }
    });
  };

  const startAutomationErrorListener = async () => {
    unlistenAutomationError = await listen<AutomationErrorEvent>('automation-error', (event) => {
      const message = event.payload?.message || 'Auto-paste failed.';
      errorMessage = message.startsWith('Auto-paste') ? message : `Auto-paste failed: ${message}`;
    });
  };

  const startTranscriptionStartedListener = async () => {
    unlistenTranscriptionStarted = await listen('transcription-started', () => {
      playTranscriptionCompleteSound();
    });
  };

  $: baseTranscripts = semanticSearchEnabled ? semanticResults : transcripts;

  $: filteredTranscripts = (() => {
    const now = Date.now();
    const cutoff = dateFilter === '7d'
      ? now - 7 * 24 * 60 * 60 * 1000
      : dateFilter === '30d'
      ? now - 30 * 24 * 60 * 60 * 1000
      : dateFilter === '90d'
      ? now - 90 * 24 * 60 * 60 * 1000
      : null;
    const normalizedSearch = search.trim().toLowerCase();

    return baseTranscripts.filter((item) => {
      const matchesSearch = semanticSearchEnabled || !normalizedSearch || [
        item.text,
        item.title ?? '',
        item.summary ?? '',
        item.tags.join(' '),
      ].some((value) => value.toLowerCase().includes(normalizedSearch));

      const matchesTags = activeTagFilters.length === 0
        || activeTagFilters.some((tag) => item.tags.includes(tag));

      const matchesDate = cutoff === null || item.created_at >= cutoff;

      return matchesSearch && matchesTags && matchesDate;
    });
  })();

  $: pasteUnavailable = runtimeInfo?.paste_method === 'unavailable';
  $: pasteLimited = runtimeInfo?.paste_method === 'clipboard_only';
  $: wlClipboardMissing = runtimeInfo?.session_type === 'wayland'
    && (runtimeInfo?.missing_helpers?.includes('wl-copy')
      || runtimeInfo?.missing_helpers?.includes('wl-paste'));
  $: pasteActionLabel = pasteLimited ? 'Copy Last' : 'Paste Last';
  $: detailDirty = !!expandedTranscript
    && detailDraft.trim() !== (expandedTranscript.text ?? '').trim();
  $: gpuErrorMessage = performanceInfo?.gpu_error ?? '';
  $: if (!gpuErrorMessage) {
    gpuErrorDismissed = '';
  }
    $: statusLabel = (() => {
      if (isRecording) return `Recording... ${formatElapsed(recordingSeconds)}`;
      if (!runtimeInfo) return 'Ready';
      if (runtimeInfo.session_type === 'wayland') {
        if (pasteUnavailable) return 'Wayland (paste unavailable)';
        if (pasteLimited) return 'Wayland (clipboard only)';
        return 'Ready on Wayland';
      }
      if (runtimeInfo.session_type === 'x11') return 'Ready on X11';
    if (runtimeInfo.session_type === 'macos') return 'Ready on macOS';
    if (runtimeInfo.session_type === 'windows') return 'Ready on Windows';
    return 'Ready';
  })();

  $: if (semanticSearchEnabled) {
    search;
    transcripts.length;
    scheduleSemanticSearch();
  } else {
    if (searchTimer) {
      clearTimeout(searchTimer);
      searchTimer = null;
    }
    semanticResults = transcripts;
    searchLoading = false;
  }

	    const registerHotkeysSafely = async () => {
	      if (!settings) return;
	      hotkeyWarning = '';

	      if (runtimeInfo && runtimeInfo.session_type === 'wayland' && !runtimeInfo.hotkeys_supported) {
	        if (!waylandHotkeysWarningDismissed) {
	          hotkeyWarning = WAYLAND_HOTKEYS_WARNING;
	        }
	        return;
	      }

        if (runtimeInfo?.session_type === 'macos') {
          const normalized = normalizeHotkeyString(settings.hotkeys.record_toggle);
          if (normalized === 'CommandOrControl+Alt+Space' || normalized === 'CmdOrControl+Alt+Space') {
            hotkeyWarning =
              'Record toggle hotkey conflicts with macOS Spotlight/Finder search (Option+Command+Space). Pick a different hotkey (example: CommandOrControl+Shift+Space).';
            return;
          }
        }

	    try {
	      await registerHotkeys(settings, {
	        onToggle: handleToggleRecording,
	        onPasteLast: handlePasteLast,
	      });
      } catch (error) {
        hotkeyWarning = error instanceof Error ? error.message : 'Failed to register hotkeys.';
      }
    };

    const dismissHotkeyWarning = () => {
      if (hotkeyWarning === WAYLAND_HOTKEYS_WARNING) {
        waylandHotkeysWarningDismissed = true;
        try {
          window.localStorage.setItem(WAYLAND_HOTKEYS_WARNING_DISMISSED_KEY, '1');
        } catch {}
      }
      hotkeyWarning = '';
    };

  const loadApp = async () => {
    try {
      settings = await getSettings();
      transcripts = await listTranscripts();
      try {
        clips = await listClips();
      } catch {
        clips = [];
      }
      models = await listModels();
      try {
        runtimeInfo = await getRuntimeInfo();
      } catch {
        runtimeInfo = null;
      }
      try {
        performanceInfo = await getPerformanceInfo();
      } catch {
        performanceInfo = null;
      }
      try {
        audioDevices = await listAudioDevices();
      } catch {
        audioDevices = [{ id: 'default', name: 'Default', is_default: true }];
      }
      try {
        currentVersion = await getVersion();
      } catch {
        currentVersion = '';
      }
      try {
        storageStats = await getStorageStats();
      } catch {
        storageStats = null;
      }
      if (settings && !settings.ui.onboarding_seen) {
        onboardingOpen = true;
        onboardingStep = 0;
      }
      await registerHotkeysSafely();
      if (!updateCheckTimer) {
        updateCheckTimer = window.setTimeout(async () => {
          try {
            const result = await check();
            if (result?.available) {
              updateAvailable = { version: result.version, update: result };
            }
          } catch {
            updateAvailable = null;
          }
        }, 1200);
      }
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to load app state.';
    } finally {
      loading = false;
    }
  };

    onMount(() => {
      transcriptionCompleteAudio = new Audio(transcriptionCompleteSoundUrl);
      transcriptionCompleteAudio.preload = 'auto';
      transcriptionCompleteAudio.volume = 0.65;
      try {
        waylandHotkeysWarningDismissed = window.localStorage.getItem(WAYLAND_HOTKEYS_WARNING_DISMISSED_KEY) === '1';
      } catch {}
      loadApp();
      startRecordingListener();
      startTranscriptListener();
      startPreviewListener();
    startModelProgressListener();
    startImportProgressListener();
    startOpenSettingsListener();
    startSettingsUpdatedListener();
    startAutomationErrorListener();
    syncUiActive();
    const onFocus = () => void syncUiActive();
    const onBlur = () => void syncUiActive();
    const onVisibility = () => void syncUiActive();
    window.addEventListener('focus', onFocus);
    window.addEventListener('blur', onBlur);
    document.addEventListener('visibilitychange', onVisibility);
    startTranscriptionStartedListener();

    // Keep the titlebar back/forward buttons working even without a router.
    if (isNavState(window.history.state)) {
      currentPage = window.history.state.page;
    } else {
      window.history.replaceState({ page: currentPage } satisfies NavState, '');
    }
    const onPopState = (event: PopStateEvent) => {
      if (isNavState(event.state)) {
        currentPage = event.state.page;
      }
    };
    window.addEventListener('popstate', onPopState);
    return () => {
      window.removeEventListener('popstate', onPopState);
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('blur', onBlur);
      document.removeEventListener('visibilitychange', onVisibility);
      try {
        void setUiActive(false);
      } catch {}
    };
  });

  onDestroy(() => {
    stopTimer();
    if (unlistenRecording) {
      unlistenRecording();
      unlistenRecording = null;
    }
    if (unlistenTranscript) {
      unlistenTranscript();
      unlistenTranscript = null;
    }
    if (unlistenPreview) {
      unlistenPreview();
      unlistenPreview = null;
    }
    if (unlistenModelProgress) {
      unlistenModelProgress();
      unlistenModelProgress = null;
    }
    if (unlistenImportProgress) {
      unlistenImportProgress();
      unlistenImportProgress = null;
    }
    if (unlistenOpenSettings) {
      unlistenOpenSettings();
      unlistenOpenSettings = null;
    }
    if (unlistenSettingsUpdated) {
      unlistenSettingsUpdated();
      unlistenSettingsUpdated = null;
    }
    if (unlistenAutomationError) {
      unlistenAutomationError();
      unlistenAutomationError = null;
    }
    if (unlistenTranscriptionStarted) {
      unlistenTranscriptionStarted();
      unlistenTranscriptionStarted = null;
    }
    if (detailCopyTimer) {
      clearTimeout(detailCopyTimer);
      detailCopyTimer = null;
    }
    if (transcriptSaveTimer) {
      clearTimeout(transcriptSaveTimer);
      transcriptSaveTimer = null;
    }
    if (clipCopyTimer) {
      clearTimeout(clipCopyTimer);
      clipCopyTimer = null;
    }
    if (searchTimer) {
      clearTimeout(searchTimer);
      searchTimer = null;
    }
    if (settingsFocusTimer) {
      clearTimeout(settingsFocusTimer);
      settingsFocusTimer = null;
    }
    if (updateCheckTimer) {
      clearTimeout(updateCheckTimer);
      updateCheckTimer = null;
    }
  });

  const handleToggleRecording = async () => {
    try {
      const result = await toggleRecording();
      applyRecordingState(result.recording, result.recording ? Date.now() : null);
      if (result.transcript) {
        if (!transcripts.some((item) => item.id === result.transcript?.id)) {
          transcripts = [result.transcript, ...transcripts];
        }
      }
    } catch (error) {
      isRecording = false;
      stopTimer();
      errorMessage = error instanceof Error ? error.message : 'Failed to toggle recording.';
    }
  };

  const handleAudioDeviceChange = async () => {
    if (!settings) return;
    if (audioDeviceSaveInFlight) return;

    const nextId = settings.audio.input_device_id;
    if (!nextId) return;
    audioDeviceSaveInFlight = true;
    errorMessage = '';

    try {
      // Persist immediately so recording uses this device right away and it survives relaunch.
      settings = await setAudioInputDevice(nextId);
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to save input device.';
      try {
        // Reload last saved settings to keep UI consistent with backend.
        settings = await getSettings();
      } catch {}
    } finally {
      audioDeviceSaveInFlight = false;
    }
  };

  const focusSettingsSection = (section: 'audio' | 'hotkeys' | 'automation' | 'app') => {
    settingsFocus = section;
    if (settingsFocusTimer) {
      clearTimeout(settingsFocusTimer);
    }
    settingsFocusTimer = window.setTimeout(() => {
      settingsFocus = null;
    }, 2200);
  };

  const openSettingsAt = async (section: 'audio' | 'hotkeys' | 'automation' | 'app') => {
    navigateTo('settings');
    await tick();
    focusSettingsSection(section);
    const target = section === 'audio'
      ? audioSectionEl
      : section === 'hotkeys'
      ? hotkeysSectionEl
      : section === 'automation'
      ? automationSectionEl
      : appSectionEl;
    if (target) {
      target.scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  };

  const markOnboardingSeen = async () => {
    if (!settings || settings.ui.onboarding_seen) return;
    const updated = {
      ...settings,
      ui: {
        ...settings.ui,
        onboarding_seen: true,
      },
    };
    settings = updated;
    try {
      settings = await saveSettings(updated);
    } catch {
      // Ignore persistence failure; user can reopen later.
    }
  };

  const finishOnboarding = async () => {
    onboardingOpen = false;
    onboardingErrorMessage = '';
    await markOnboardingSeen();
  };

  const skipOnboarding = async () => {
    onboardingOpen = false;
    onboardingErrorMessage = '';
    await markOnboardingSeen();
  };

  const nextOnboardingStep = () => {
    if (onboardingStep < onboardingTotal - 1) {
      onboardingStep += 1;
    } else {
      finishOnboarding();
    }
  };

  const backOnboardingStep = () => {
    onboardingErrorMessage = '';
    if (onboardingStep > 0) onboardingStep -= 1;
  };

  const skipOnboardingStep = () => {
    onboardingErrorMessage = '';
    if (onboardingStep < onboardingTotal - 1) {
      onboardingStep += 1;
    } else {
      finishOnboarding();
    }
  };

  const handleOnboardingAudioDeviceSelect = async (nextId: string) => {
    if (!settings) return;
    if (audioDeviceSaveInFlight) return;
    if (!nextId) return;

    audioDeviceSaveInFlight = true;
    onboardingErrorMessage = '';

    try {
      // Persist immediately so recording uses this device right away and it survives relaunch.
      settings = await setAudioInputDevice(nextId);
    } catch (error) {
      onboardingErrorMessage = error instanceof Error ? error.message : 'Failed to save input device.';
      try {
        // Reload last saved settings to keep UI consistent with backend.
        settings = await getSettings();
      } catch {}
    } finally {
      audioDeviceSaveInFlight = false;
    }
  };

  const advanceOnboardingStep = async (
    event: CustomEvent<{
      step: number;
      hotkeys?: { record_toggle: string; paste_last: string; open_app: string };
      automation?: {
        auto_paste_enabled: boolean;
        paste_delay_ms: number;
        copy_to_clipboard: boolean;
        preserve_clipboard: boolean;
        paste_method: string;
      };
    }>
  ) => {
    onboardingErrorMessage = '';
    if (!settings) {
      nextOnboardingStep();
      return;
    }

    const { hotkeys, automation } = event.detail;
    if (hotkeys) {
      settings = {
        ...settings,
        hotkeys: {
          ...settings.hotkeys,
          ...hotkeys,
        },
      };
    }
    if (automation) {
      settings = {
        ...settings,
        automation: {
          ...settings.automation,
          ...automation,
        },
      };
    }

    if (hotkeys || automation) {
      const ok = await handleSaveSettings();
      if (!ok) {
        onboardingErrorMessage = errorMessage || 'Failed to save settings.';
        return;
      }
    }

    nextOnboardingStep();
  };

  const handlePasteLast = async () => {
    try {
      await pasteLastTranscript();
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to paste or copy last transcript.';
    }
  };

  const handleCopyTranscript = async (id: string, text: string) => {
    try {
      await copyText(text);
      copiedId = id;
      setTimeout(() => {
        if (copiedId === id) copiedId = null;
      }, 1200);
    } catch {
      copiedId = null;
    }
  };

  const handleCopyClip = async (clip: Clip) => {
    try {
      await copyText(clip.text);
      clipCopiedId = clip.id;
      if (clipCopyTimer) {
        clearTimeout(clipCopyTimer);
      }
      clipCopyTimer = window.setTimeout(() => {
        if (clipCopiedId === clip.id) clipCopiedId = null;
      }, 1200);
    } catch {
      clipCopiedId = null;
    }
  };

  const deriveClipTitle = (text: string) => {
    const words = text.trim().split(/\s+/).slice(0, 6);
    return words.join(' ') || 'Clip';
  };

  const handleSaveClip = async () => {
    if (!expandedTranscript) return;
    const selected = detailTextareaEl
      ? detailDraft.slice(detailTextareaEl.selectionStart ?? 0, detailTextareaEl.selectionEnd ?? 0).trim()
      : '';
    const clipText = selected || detailDraft.trim();
    if (!clipText) {
      errorMessage = 'Select text or enter content before saving a clip.';
      return;
    }

    const title = deriveClipTitle(clipText);
    try {
      const clip = await createClip(title, clipText, expandedTranscript.id);
      clips = [clip, ...clips];
      clipSavedId = clip.id;
      setTimeout(() => {
        if (clipSavedId === clip.id) clipSavedId = null;
      }, 1500);
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to save clip.';
    }
  };

  const handleDeleteClip = async (id: string) => {
    try {
      await deleteClip(id);
      clips = clips.filter((clip) => clip.id !== id);
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to delete clip.';
    }
  };

  const scheduleSemanticSearch = () => {
    if (!semanticSearchEnabled) return;
    if (!search.trim()) {
      semanticResults = transcripts;
      searchLoading = false;
      return;
    }
    if (searchTimer) {
      clearTimeout(searchTimer);
    }
    searchLoading = true;
    searchTimer = window.setTimeout(async () => {
      try {
        semanticResults = await searchTranscripts(search);
      } catch (error) {
        errorMessage = error instanceof Error ? error.message : 'Semantic search failed.';
        semanticResults = transcripts;
      } finally {
        searchLoading = false;
      }
    }, 250);
  };

  const handleImportAudio = async () => {
    if (importing) return;
    let selection: string | string[] | null = null;
    try {
      selection = await open({
        multiple: true,
        filters: [
          {
            name: 'Audio',
            extensions: ['wav', 'mp3', 'm4a', 'mp4', 'aac', 'flac', 'ogg'],
          },
        ],
      });
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to open file picker.';
      return;
    }

    if (!selection) return;
    const paths = Array.isArray(selection) ? selection : [selection];
    if (paths.length === 0) return;

    importing = true;
    importFailures = [];
    importProgress = null;
    errorMessage = '';

    try {
      const result = await importAudioFiles(paths);
      if (result.failures.length) {
        importFailures = result.failures;
        errorMessage = `${result.failures.length} audio file${result.failures.length === 1 ? '' : 's'} failed to import.`;
      }
      if (result.transcripts.length) {
        storageStats = await getStorageStats();
      }
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to import audio.';
    } finally {
      importing = false;
      importProgress = null;
    }
  };

  const handleBenchmark = async () => {
    if (!settings || benchmarking) return;
    benchmarkError = '';
    benchmarkResult = null;

    let selection: string | string[] | null = null;
    try {
      selection = await open({
        multiple: false,
        filters: [
          {
            name: 'Audio',
            extensions: ['wav', 'mp3', 'm4a', 'mp4', 'aac', 'flac', 'ogg'],
          },
        ],
      });
    } catch (error) {
      benchmarkError = error instanceof Error ? error.message : 'Failed to open file picker.';
      return;
    }

    if (!selection || Array.isArray(selection)) return;
    benchmarking = true;
    try {
      benchmarkResult = await benchmarkTranscription(selection);
    } catch (error) {
      benchmarkError = error instanceof Error ? error.message : 'Benchmark failed.';
    } finally {
      benchmarking = false;
    }
  };

  const formatBullets = (text: string) => {
    const sentences = text
      .split(/[.!?]\s+/)
      .map((sentence) => sentence.trim())
      .filter(Boolean);
    if (sentences.length === 0) return text.trim();
    return sentences.map((sentence) => `- ${sentence}`).join('\n');
  };

  const formatMarkdown = (title: string | null, text: string) => {
    const header = title ? `# ${title}\n\n` : '';
    const body = formatBullets(text);
    return `${header}${body}`;
  };

  const formatCopyText = (format: 'plain' | 'markdown' | 'bullets', transcript: Transcript | null, text: string) => {
    const safeText = text.trim();
    if (format === 'bullets') return formatBullets(safeText);
    if (format === 'markdown') return formatMarkdown(transcript?.title ?? null, safeText);
    return safeText;
  };

  const resolveTitle = (transcript: Transcript) => {
    if (transcript.title && transcript.title.trim()) return transcript.title;
    const words = transcript.text.trim().split(/\s+/).slice(0, 6);
    return words.join(' ') || 'Untitled';
  };

  const resolveSummary = (transcript: Transcript) => {
    if (transcript.summary && transcript.summary.trim()) return transcript.summary;
    return transcript.text;
  };

  const formatBytes = (bytes: number) => {
    if (!Number.isFinite(bytes)) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB'];
    let value = Math.max(0, bytes);
    let unitIndex = 0;
    while (value >= 1024 && unitIndex < units.length - 1) {
      value /= 1024;
      unitIndex += 1;
    }
    return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
  };

  const sanitizeFilename = (value: string) => {
    const cleaned = value.replace(/[<>:"/\\|?*\x00-\x1F]/g, '').trim();
    return cleaned.slice(0, 80) || 'transcript';
  };

  const resolveExportExtension = (format: 'plain' | 'markdown' | 'bullets') =>
    format === 'markdown' ? 'md' : 'txt';

  const pickDirectory = async () => {
    let selection: string | string[] | null = null;
    try {
      selection = await open({
        directory: true,
        multiple: false,
      });
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to open directory picker.';
      return null;
    }
    if (!selection || Array.isArray(selection)) return null;
    return selection;
  };

  const handleSaveSettings = async (): Promise<boolean> => {
    if (!settings) return false;
    savingSettings = true;
    errorMessage = '';
    const hotkeyError = validateHotkeys(settings);
    if (hotkeyError) {
      errorMessage = hotkeyError;
      savingSettings = false;
      return false;
    }
    if (
      !(settings.automation.copy_to_clipboard || settings.automation.preserve_clipboard)
      && settings.automation.paste_method === 'clipboard_only'
    ) {
      errorMessage = 'Clipboard-only paste requires clipboard usage to be enabled.';
      savingSettings = false;
      return false;
    }
    if (settings.automation.preserve_clipboard && settings.automation.paste_method === 'clipboard_only') {
      errorMessage = 'Preserve clipboard cannot be used with "Clipboard only" paste.';
      savingSettings = false;
      return false;
    }
    try {
      settings = await saveSettings(settings);
      try {
        transcripts = await listTranscripts();
      } catch {}
      try {
        clips = await listClips();
      } catch {}
      try {
        models = await listModels();
      } catch {}
      try {
        runtimeInfo = await getRuntimeInfo();
      } catch {
        runtimeInfo = null;
      }
      try {
        performanceInfo = await getPerformanceInfo();
      } catch {}
      try {
        storageStats = await getStorageStats();
      } catch {}
      await registerHotkeysSafely();
      return true;
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to save settings.';
      return false;
    } finally {
      savingSettings = false;
    }
  };

  const handleModelAction = async (
    action: () => Promise<ModelInfo[]>,
    busyId: string | null = null
  ) => {
    errorMessage = '';
    modelBusyId = busyId;
    try {
      models = await action();
      settings = await getSettings();
      try {
        storageStats = await getStorageStats();
      } catch {}
      if (busyId && modelProgress[busyId]) {
        const { [busyId]: _ignored, ...rest } = modelProgress;
        modelProgress = rest;
      }
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Model action failed.';
    } finally {
      modelBusyId = null;
    }
  };

  const handleUpdate = async () => {
    if (!updateAvailable?.update) return;
    updateDownloading = true;
    updateProgress = 0;
    updateProgressTotal = 0;
    updateProgressDownloaded = 0;
    try {
      await updateAvailable.update.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          updateProgressTotal = Number(event.data?.contentLength ?? 0) || 0;
          updateProgressDownloaded = 0;
          updateProgress = 0;
        } else if (event.event === 'Progress') {
          updateProgressDownloaded += Number(event.data?.chunkLength ?? 0) || 0;
          if (updateProgressTotal > 0) {
            updateProgress = Math.min(100, (updateProgressDownloaded / updateProgressTotal) * 100);
          }
        } else if (event.event === 'Finished') {
          if (updateProgressTotal > 0) {
            updateProgress = 100;
          }
        }
      });
      await relaunch();
    } catch (err) {
      errorMessage = err instanceof Error ? err.message : 'Update failed.';
      updateDownloading = false;
    }
  };

  const handleExportTranscript = async (transcript: Transcript, contentOverride?: string) => {
    const payload = formatCopyText(copyFormat, transcript, contentOverride ?? transcript.text);
    const ext = resolveExportExtension(copyFormat);
    const defaultName = `${sanitizeFilename(resolveTitle(transcript))}.${ext}`;
    let path: string | null = null;
    try {
      path = await save({
        defaultPath: defaultName,
        filters: [{ name: ext === 'md' ? 'Markdown' : 'Text', extensions: [ext] }],
      });
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to open save dialog.';
      return;
    }
    if (!path) return;
    try {
      const outputPath = path.toLowerCase().endsWith(`.${ext}`)
        ? path
        : `${path}.${ext}`;
      await exportTranscript(outputPath, payload);
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to export transcript.';
    }
  };

  const handleClearTranscripts = async () => {
    clearingTranscripts = true;
    errorMessage = '';
    try {
      await clearTranscripts();
      transcripts = [];
      copiedId = null;
      search = '';
      activeTagFilters = [];
      semanticResults = [];
      expandedTranscript = null;
      try {
        storageStats = await getStorageStats();
      } catch {}
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to clear transcripts.';
    } finally {
      clearingTranscripts = false;
    }
  };

  const openTranscriptDetail = (transcript: Transcript) => {
    expandedTranscript = transcript;
    detailDraft = transcript.text;
    detailCopied = false;
    copyFormat = 'plain';
    clipSavedId = null;
    transcriptSaved = false;
  };

  const closeTranscriptDetail = () => {
    expandedTranscript = null;
    detailDraft = '';
    detailCopied = false;
    clipSavedId = null;
    transcriptSaved = false;
    if (detailCopyTimer) {
      clearTimeout(detailCopyTimer);
      detailCopyTimer = null;
    }
    if (transcriptSaveTimer) {
      clearTimeout(transcriptSaveTimer);
      transcriptSaveTimer = null;
    }
  };

  const handleSaveTranscriptEdits = async () => {
    if (!expandedTranscript || savingTranscript) return;
    const nextText = detailDraft.trim();
    if (!nextText) {
      errorMessage = 'Transcript text cannot be empty.';
      return;
    }
    if (!detailDirty) return;
    savingTranscript = true;
    errorMessage = '';
    try {
      const updated = await updateTranscript(expandedTranscript.id, { text: nextText } as TranscriptUpdate);
      applyTranscriptUpdate(updated);
      detailDraft = updated.text;
      transcriptSaved = true;
      if (transcriptSaveTimer) {
        clearTimeout(transcriptSaveTimer);
      }
      transcriptSaveTimer = window.setTimeout(() => {
        transcriptSaved = false;
      }, 1400);
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to save transcript edits.';
    } finally {
      savingTranscript = false;
    }
  };

  const handleDeleteTranscript = async (transcript: Transcript) => {
    errorMessage = '';
    try {
      await deleteTranscript(transcript.id);
      transcripts = transcripts.filter((item) => item.id !== transcript.id);
      semanticResults = semanticResults.filter((item) => item.id !== transcript.id);
      if (expandedTranscript?.id === transcript.id) {
        closeTranscriptDetail();
      }
      try {
        storageStats = await getStorageStats();
      } catch {}
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to delete transcript.';
    }
  };

  const handleDetailCopy = async () => {
    const payload = formatCopyText(copyFormat, expandedTranscript, detailDraft);
    try {
      await copyText(payload);
    } catch {
      // Ignore clipboard errors and still show feedback.
    }
    detailCopied = true;
    if (detailCopyTimer) clearTimeout(detailCopyTimer);
    detailCopyTimer = window.setTimeout(() => {
      detailCopied = false;
    }, 1200);
  };

  const applyTranscriptUpdate = (updated: Transcript) => {
    transcripts = transcripts.map((item) => (item.id === updated.id ? updated : item));
    semanticResults = semanticResults.map((item) => (item.id === updated.id ? updated : item));
    if (expandedTranscript?.id === updated.id) {
      expandedTranscript = updated;
    }
  };

  const toggleTagFilter = (tag: string) => {
    if (activeTagFilters.includes(tag)) {
      activeTagFilters = activeTagFilters.filter((item) => item !== tag);
    } else {
      activeTagFilters = [...activeTagFilters, tag];
    }
  };

  const toggleTranscriptTag = async (transcript: Transcript, tag: string) => {
    const tags = transcript.tags.includes(tag)
      ? transcript.tags.filter((item) => item !== tag)
      : [...transcript.tags, tag];
    try {
      const updated = await updateTranscript(transcript.id, { tags } as TranscriptUpdate);
      applyTranscriptUpdate(updated);
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Failed to update tags.';
    }
  };

  const handleThemeChange = (event: Event) => {
    const target = event.target as HTMLSelectElement;
    theme.setTheme(target.value as ThemePreference);
  };

  const handleCardKeydown = (event: KeyboardEvent, transcript: Transcript) => {
    if (event.target !== event.currentTarget) return;
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      handleCopyTranscript(transcript.id, transcript.text);
    }
  };
</script>

<main class="app-shell">
  <header
    class="titlebar"
    role="presentation"
    data-tauri-drag-region
    on:mousedown={handleTitlebarMouseDown}
  >
    <div class="titlebar-left" data-tauri-drag-region>
      <div class="traffic-lights" aria-label="Window controls" data-tauri-drag-region>
        <button
          class="traffic-light"
          type="button"
          aria-label="Close window"
          data-tauri-drag-region="false"
          on:mousedown|stopPropagation
          on:click={handleWindowClose}
          style="background-color: var(--traffic-close);"
        ></button>
        <button
          class="traffic-light"
          type="button"
          aria-label="Minimize window"
          data-tauri-drag-region="false"
          on:mousedown|stopPropagation
          on:click={handleWindowMinimize}
          style="background-color: var(--traffic-minimize);"
        ></button>
        <button
          class="traffic-light"
          type="button"
          aria-label="Toggle maximize"
          data-tauri-drag-region="false"
          on:mousedown|stopPropagation
          on:click={handleWindowToggleMaximize}
          style="background-color: var(--traffic-zoom);"
        ></button>
      </div>
      <div class="titlebar-nav" data-tauri-drag-region>
        <button
          class="icon-button"
          type="button"
          aria-label="Back"
          data-tauri-drag-region="false"
          on:mousedown|stopPropagation
          on:click={handleNavBack}
        >
          <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12.5 4.5L7.5 10l5 5.5" />
          </svg>
        </button>
        <button
          class="icon-button"
          type="button"
          aria-label="Forward"
          data-tauri-drag-region="false"
          on:mousedown|stopPropagation
          on:click={handleNavForward}
        >
          <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <path d="M7.5 4.5L12.5 10l-5 5.5" />
          </svg>
        </button>
      </div>
    </div>
    <div class="titlebar-right" data-tauri-drag-region>
      <button
        class="icon-button"
        type="button"
        aria-label="Settings"
        data-tauri-drag-region="false"
        on:mousedown|stopPropagation
        on:click={() => navigateTo('settings')}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01A1.65 1.65 0 0 0 10 3.09V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01A1.65 1.65 0 0 0 20.91 10H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
      </button>
    </div>
  </header>

  <div class="app-body">
    <aside class="sidebar">
      <div class="sidebar-brand">
        <div class="brand-logo">
          <div class="brand-bars" aria-hidden="true">
            <span></span>
            <span></span>
            <span></span>
          </div>
          <span class="brand-name">Whispr</span>
        </div>
      </div>
      <nav class="sidebar-nav" aria-label="Primary">
        <button
          class={`nav-item ${currentPage === 'home' ? 'active' : ''}`}
          type="button"
          on:click={() => navigateTo('home')}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 10.5L12 3l9 7.5" />
            <path d="M5 10v10h14V10" />
          </svg>
          <span>Home</span>
        </button>
        <button
          class={`nav-item ${currentPage === 'clips' ? 'active' : ''}`}
          type="button"
          on:click={() => navigateTo('clips')}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <rect x="3" y="4" width="18" height="14" rx="2" />
            <path d="M7 8h10" />
            <path d="M7 12h6" />
          </svg>
          <span>Clips</span>
        </button>
        <button
          class={`nav-item ${currentPage === 'settings' ? 'active' : ''}`}
          type="button"
          on:click={() => navigateTo('settings')}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01A1.65 1.65 0 0 0 10 3.09V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01A1.65 1.65 0 0 0 20.91 10H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
          <span>Settings</span>
        </button>
      </nav>
    </aside>

    <section class="main-content">
      {#if currentPage === 'home'}
        <div class="main-scroll">
          <div class="container container-wide">
            {#if errorMessage}
              <div class="banner error" role="alert">
                <span>{errorMessage}</span>
                <button
                  class="banner-dismiss"
                  type="button"
                  on:click={() => (errorMessage = '')}
                  aria-label="Dismiss error"
                >
                  x
                </button>
              </div>
            {/if}
            {#if gpuErrorMessage && gpuErrorMessage !== gpuErrorDismissed}
              <div class="banner warning" role="status">
                <span>{gpuErrorMessage}</span>
                <button
                  class="banner-dismiss"
                  type="button"
                  on:click={() => (gpuErrorDismissed = gpuErrorMessage)}
                  aria-label="Dismiss GPU warning"
                >
                  x
                </button>
              </div>
            {/if}
              {#if hotkeyWarning}
                <div class="banner info" role="status">
                  <span>{hotkeyWarning}</span>
                  <button
                    class="banner-dismiss"
                    type="button"
                    on:click={dismissHotkeyWarning}
                    aria-label="Dismiss hotkey warning"
                  >
                    x
                  </button>
                </div>
              {/if}
            {#if updateAvailable && !updateDismissed}
              <div class="banner info" role="status">
                <span>
                  {#if updateDownloading}
                    {#if updateProgressTotal > 0}
                      Downloading update... {Math.floor(updateProgress)}%
                    {:else}
                      Downloading update...
                    {/if}
                  {:else}
                    Update available: {updateAvailable.version} (current {currentVersion})
                  {/if}
                </span>
                <div class="banner-actions">
                  {#if !updateDownloading}
                    <button class="btn-tertiary" type="button" on:click={handleUpdate}>
                      Update now
                    </button>
                    <button class="btn-tertiary" type="button" on:click={() => (updateDismissed = true)}>
                      Dismiss
                    </button>
                  {/if}
                </div>
              </div>
            {/if}
            {#if importFailures.length > 0}
              <div class="banner warning" role="status">
                <span>Some audio files failed to import.</span>
                <div class="import-failures">
                  {#each importFailures.slice(0, 3) as failure}
                    <div class="import-failure-row">
                      <span>{failure.path.split(/[\\/]/).pop()}</span>
                      <span>{failure.error}</span>
                    </div>
                  {/each}
                  {#if importFailures.length > 3}
                    <div class="import-failure-row">+{importFailures.length - 3} more</div>
                  {/if}
                </div>
              </div>
            {/if}

            <div class="home-header">
              <div class="home-header-left">
                <h1 class="home-title">Recent</h1>
                <div class={`status-pill ${isRecording ? 'recording' : ''}`} role="status" aria-live="polite">
                  <span class="status-dot"></span>
                  <span>{statusLabel}</span>
                </div>
              </div>
              <div class="home-header-right">
                <div class="toolbar-actions">
                  <button
                    class="btn-tertiary"
                    type="button"
                    on:click={handlePasteLast}
                    disabled={pasteUnavailable || transcripts.length === 0}
                    title={pasteUnavailable ? 'Paste unavailable' : ''}
                  >
                    {pasteActionLabel}
                  </button>
                  <button
                    class="btn-secondary"
                    type="button"
                    on:click={handleImportAudio}
                    disabled={importing}
                  >
                    {importing ? 'Importing...' : 'Import Audio'}
                  </button>
                  <button
                    class="btn-primary"
                    type="button"
                    on:click={handleToggleRecording}
                    disabled={loading}
                  >
                    {isRecording ? 'Stop & Paste' : 'Start Recording'}
                  </button>
                </div>
                <button
                  class="icon-button subtle danger"
                  type="button"
                  disabled={transcripts.length === 0 || clearingTranscripts}
                  on:click={() => (clearConfirmOpen = true)}
                  aria-label="Delete all transcripts"
                  title="Delete all transcripts"
                >
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M3 6h18" />
                    <path d="M8 6V4h8v2" />
                    <path d="M6 6l1 14h10l1-14" />
                    <path d="M10 11v6" />
                    <path d="M14 11v6" />
                  </svg>
                </button>
              </div>
            </div>

            {#if isRecording && settings?.ui.live_preview_enabled}
              <div class="preview-card" aria-live="polite">
                <div class="preview-header">
                  <span>Live preview</span>
                  <span class="preview-hint">Updates every few seconds</span>
                </div>
                <p>{previewText || 'Listening...'}</p>
              </div>
            {/if}

            {#if importProgress}
              <div class="import-progress">
                <span>Importing {importProgress.index}/{importProgress.total}</span>
                <span class="import-path">{importProgress.path.split(/[\\/]/).pop()}</span>
              </div>
            {/if}

            <div class="search-row">
              <div class="search-input">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <circle cx="11" cy="11" r="7" />
                  <path d="M21 21l-4.35-4.35" />
                </svg>
                <input
                  type="text"
                  placeholder="Search transcripts"
                  bind:value={search}
                  role="searchbox"
                  aria-label="Search transcripts"
                />
              </div>
              <div class="search-filters" role="group" aria-label="Search filters">
                <button
                  class={`toggle-pill ${semanticSearchEnabled ? 'active' : ''}`}
                  type="button"
                  on:click={() => (semanticSearchEnabled = !semanticSearchEnabled)}
                  aria-pressed={semanticSearchEnabled}
                >
                  Semantic
                </button>
                <select
                  class="select-compact"
                  bind:value={dateFilter}
                  aria-label="Filter transcripts by date"
                >
                  <option value="all">All time</option>
                  <option value="7d">Last 7 days</option>
                  <option value="30d">Last 30 days</option>
                  <option value="90d">Last 90 days</option>
                </select>
              </div>
            </div>
            {#if semanticSearchEnabled}
              <div class="search-hint">
                {searchLoading ? 'Searching...' : 'Semantic search enabled'}
              </div>
            {/if}
            <div class="tag-filters">
              {#each tagOptions as tag}
                <button
                  class={`tag-chip ${activeTagFilters.includes(tag) ? 'active' : ''}`}
                  type="button"
                  on:click={() => toggleTagFilter(tag)}
                >
                  {tag}
                </button>
              {/each}
            </div>

            {#if loading}
              <div class="loading-state">Loading transcripts...</div>
            {:else if searchLoading && semanticSearchEnabled}
              <div class="loading-state">Searching transcripts...</div>
            {:else if filteredTranscripts.length === 0}
              {#if search.trim()}
                <EmptyState
                  icon="search"
                  title="No matches found"
                  description="Try adjusting your search terms"
                />
              {:else}
                <EmptyState
                  icon="microphone"
                  title="No transcripts yet"
                  description={runtimeInfo?.hotkeys_supported === false
                    ? 'Click Start Recording to begin'
                    : 'Press your hotkey or click the button below'}
                  actionLabel="Start Recording"
                  on:action={handleToggleRecording}
                />
              {/if}
            {:else}
              <div class={`transcript-grid ${settings?.ui.list_compact ? 'compact' : ''}`}>
                {#each filteredTranscripts as transcript}
                  <div
                    class="transcript-card"
                    role="button"
                    tabindex="0"
                    on:click={() => handleCopyTranscript(transcript.id, transcript.text)}
                    on:keydown={(event) => handleCardKeydown(event, transcript)}
                  >
                      <div class="transcript-rail" aria-hidden="true">
                        <div class="transcript-rail-top">
                        <span class="transcript-date">
                          <span class="transcript-date-main">{formatRailDate(transcript.created_at)}</span>
                          <span class="transcript-time">{formatRailTime(transcript.created_at)}</span>
                        </span>
                        <span class="transcript-duration">{formatDuration(transcript.duration_ms)}</span>
                      </div>
                      {#if copiedId === transcript.id}
                        <span class="badge">Copied</span>
                      {/if}
                    </div>
                    <div class="transcript-main">
                      <div class="transcript-text">
                        <h3 class="transcript-title">{resolveTitle(transcript)}</h3>
                        <p class="transcript-summary">{resolveSummary(transcript)}</p>
                        {#if transcript.tags.length > 0}
                          <div class="transcript-tags">
                            {#each transcript.tags as tag}
                              <span class="tag-chip compact">{tag}</span>
                            {/each}
                          </div>
                        {/if}
                        <div class="transcript-fade"></div>
                      </div>
                    </div>
                    <div class="transcript-actions" aria-label="Transcript actions">
                      <button
                        class="icon-button subtle"
                        type="button"
                        aria-label="Expand transcript"
                        on:click|stopPropagation={() => openTranscriptDetail(transcript)}
                      >
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
                          <path d="M4 9V4h5" />
                          <path d="M20 15v5h-5" />
                          <path d="M15 4h5v5" />
                          <path d="M9 20H4v-5" />
                        </svg>
                      </button>
                      <button
                        class="icon-button subtle danger"
                        type="button"
                        aria-label="Delete transcript"
                        on:click|stopPropagation={() => (deleteConfirmTranscript = transcript)}
                      >
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
                          <path d="M3 6h18" />
                          <path d="M8 6V4h8v2" />
                          <path d="M6 6l1 14h10l1-14" />
                          <path d="M10 11v6" />
                          <path d="M14 11v6" />
                        </svg>
                      </button>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        </div>
      {:else if currentPage === 'clips'}
        <div class="main-scroll">
          <div class="container container-narrow">
            <div class="clips-header">
              <h1 class="settings-title">Clips</h1>
              <p class="clips-subtitle">Save reusable snippets from transcripts.</p>
            </div>

            {#if errorMessage}
              <div class="banner error" role="alert">
                <span>{errorMessage}</span>
                <button
                  class="banner-dismiss"
                  type="button"
                  on:click={() => (errorMessage = '')}
                  aria-label="Dismiss error"
                >
                  x
                </button>
              </div>
            {/if}
            {#if gpuErrorMessage && gpuErrorMessage !== gpuErrorDismissed}
              <div class="banner warning" role="status">
                <span>{gpuErrorMessage}</span>
                <button
                  class="banner-dismiss"
                  type="button"
                  on:click={() => (gpuErrorDismissed = gpuErrorMessage)}
                  aria-label="Dismiss GPU warning"
                >
                  x
                </button>
              </div>
            {/if}

            {#if clips.length === 0}
              <EmptyState
                icon="bookmark"
                title="No clips yet"
                description="Save highlights from your transcripts to build a reusable library."
              />
            {:else}
              <div class="clip-grid">
                {#each clips as clip}
                  <div class="clip-card">
                    <div class="clip-meta">
                      <span>{formatTimestamp(clip.created_at)}</span>
                      {#if clipCopiedId === clip.id}
                        <span class="badge">Copied</span>
                      {/if}
                    </div>
                    <h3 class="clip-title">{clip.title}</h3>
                    <p class="clip-text">{clip.text}</p>
                    <div class="clip-actions">
                      <button class="btn-tertiary" type="button" on:click={() => handleCopyClip(clip)}>
                        Copy
                      </button>
                      <button class="btn-tertiary danger" type="button" on:click={() => handleDeleteClip(clip.id)}>
                        Delete
                      </button>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        </div>
      {:else}
        <div class="main-scroll">
          <div class="container container-narrow">
            <h1 class="settings-title">Settings</h1>

            {#if errorMessage}
              <div class="banner error" role="alert">
                <span>{errorMessage}</span>
                <button
                  class="banner-dismiss"
                  type="button"
                  on:click={() => (errorMessage = '')}
                  aria-label="Dismiss error"
                >
                  x
                </button>
              </div>
            {/if}
            {#if gpuErrorMessage && gpuErrorMessage !== gpuErrorDismissed}
              <div class="banner warning" role="status">
                <span>{gpuErrorMessage}</span>
                <button
                  class="banner-dismiss"
                  type="button"
                  on:click={() => (gpuErrorDismissed = gpuErrorMessage)}
                  aria-label="Dismiss GPU warning"
                >
                  x
                </button>
              </div>
            {/if}
              {#if hotkeyWarning}
                <div class="banner info" role="status">
                  <span>{hotkeyWarning}</span>
                  <button
                    class="banner-dismiss"
                    type="button"
                    on:click={dismissHotkeyWarning}
                    aria-label="Dismiss hotkey warning"
                  >
                    x
                  </button>
                </div>
              {/if}

              {#if settings}
                {#if runtimeInfo && runtimeInfo.session_type === 'wayland' && runtimeInfo.missing_helpers.length > 0}
                  <div class="banner info" role="status">
                    <span>
                      Wayland detected. Install <code class="code-hint">wl-clipboard</code> for clipboard copy and
                      <code class="code-hint">wtype</code> (or <code class="code-hint">ydotool</code>) for auto-paste.
                      For GNOME global hotkeys, add custom shortcuts that run
                      <code class="code-hint">whispr --toggle</code> and <code class="code-hint">whispr --paste-last</code>.
                      Missing: <code class="code-hint">{runtimeInfo.missing_helpers.join(', ')}</code>.
                    </span>
                  </div>
                {/if}

              <div class={`settings-section ${settingsFocus === 'hotkeys' ? 'focused' : ''}`} bind:this={hotkeysSectionEl}>
                <h2>Hotkeys</h2>
                <div class="settings-card">
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="record-hotkey">Record toggle</label>
                    </div>
                    <div class="settings-control">
                      <HotkeyInput
                        id="record-hotkey"
                        platform={runtimeInfo?.session_type ?? 'unknown'}
                        bind:value={settings.hotkeys.record_toggle}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="paste-hotkey">Paste last</label>
                    </div>
                    <div class="settings-control">
                      <HotkeyInput
                        id="paste-hotkey"
                        platform={runtimeInfo?.session_type ?? 'unknown'}
                        bind:value={settings.hotkeys.paste_last}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="open-hotkey">Open app</label>
                    </div>
                    <div class="settings-control">
                      <HotkeyInput
                        id="open-hotkey"
                        platform={runtimeInfo?.session_type ?? 'unknown'}
                        bind:value={settings.hotkeys.open_app}
                      />
                    </div>
                  </div>
                </div>
              </div>

              <div class={`settings-section ${settingsFocus === 'automation' ? 'focused' : ''}`} bind:this={automationSectionEl}>
                <h2>Automation</h2>
                <div class="settings-card">
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="auto-paste">Auto-paste</label>
                      <p class="settings-hint">Automatically paste after transcription</p>
                    </div>
                    <div class="settings-control">
                      <input id="auto-paste" type="checkbox" bind:checked={settings.automation.auto_paste_enabled} />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="keep-clipboard">Copy to clipboard</label>
                      <p class="settings-hint">Copy output to clipboard automatically</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="keep-clipboard"
                        type="checkbox"
                        bind:checked={settings.automation.copy_to_clipboard}
                        on:change={() => {
                          if (settings && settings.automation.copy_to_clipboard) {
                            settings.automation.preserve_clipboard = false;
                          }
                        }}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="preserve-clipboard">Preserve clipboard contents</label>
                      <p class="settings-hint">Auto-paste without leaving the transcript in your clipboard</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="preserve-clipboard"
                        type="checkbox"
                        bind:checked={settings.automation.preserve_clipboard}
                        disabled={settings.automation.copy_to_clipboard}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="paste-delay">Paste delay (ms)</label>
                      <p class="settings-hint">Delay before pasting transcription</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="paste-delay"
                        class="input-compact"
                        type="number"
                        min="0"
                        step="50"
                        bind:value={settings.automation.paste_delay_ms}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="paste-method">Paste method</label>
                      <p class="settings-hint">Override auto detection when needed.</p>
                    </div>
                    <div class="settings-control">
                      <select
                        id="paste-method"
                        class="select-compact"
                        bind:value={settings.automation.paste_method}
                      >
                        <option value="auto">Auto</option>
                        <option value="x11_ctrl_v">Keyboard paste (Ctrl/Cmd+V)</option>
                        <option value="wayland_wtype">Wayland (wtype)</option>
                        <option value="wayland_ydotool">Wayland (ydotool)</option>
                        <option value="clipboard_only">Clipboard only</option>
                      </select>
                    </div>
                  </div>
                </div>
              </div>

              <div class="settings-section">
                <h2>Models</h2>
                <div class="model-list">
                  {#each models as model}
                    <div class="model-row">
                        <div class="model-info">
                          <div class="model-meta">
                            <div class="model-name">
                              <span>{model.label}</span>
                              {#if model.active && model.installed}
                                <span class="model-badge">Active</span>
                              {/if}
                            </div>
                            <div class="model-subtext">{model.id}</div>
                          </div>
                        </div>
                        <div class="model-actions">
                          {#if model.active && model.installed}
                            <div class="model-icon-btn active-indicator" role="img" aria-label="Active" title="Active">
                              <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                                <path d="M4 10l3.5 3.5L16 6" />
                              </svg>
                            </div>
                          {:else if model.installed}
                          <button
                            class="model-action-btn"
                            type="button"
                            disabled={modelBusyId === model.id}
                            on:click={() => handleModelAction(() => activateModel(model.id), model.id)}
                          >
                            Activate
                          </button>
                          <button
                            class="model-icon-btn danger"
                            type="button"
                            disabled={modelBusyId === model.id}
                            on:click={() => (deleteConfirmModel = model)}
                            aria-label="Delete"
                          >
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
                              <path d="M3 6h18" />
                              <path d="M8 6V4h8v2" />
                              <path d="M6 6l1 14h10l1-14" />
                              <path d="M10 11v6" />
                              <path d="M14 11v6" />
                            </svg>
                          </button>
                        {:else}
                          <button
                            class="model-icon-btn"
                            type="button"
                            disabled={modelBusyId === model.id}
                            on:click={() => handleModelAction(() => downloadModel(model.id), model.id)}
                            aria-label="Download"
                          >
                            {#if modelProgress[model.id]}
                              <span class="download-progress">
                                {modelProgress[model.id].total > 0
                                  ? `${Math.round((modelProgress[model.id].downloaded / modelProgress[model.id].total) * 100)}%`
                                  : '...'}
                              </span>
                            {:else if modelBusyId === model.id}
                              <span class="spinner"></span>
                            {:else}
                              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
                                <path d="M12 3v12" />
                                <path d="M7 10l5 5 5-5" />
                                <path d="M4 20h16" />
                              </svg>
                            {/if}
                          </button>
                        {/if}
                      </div>
                    </div>
                  {/each}
                </div>
              </div>

              <div class="settings-section">
                <h2>Transcription</h2>
                <div class="settings-card">
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="model-dir">Model directory</label>
                      <p class="settings-hint">Where downloaded models are stored.</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="model-dir"
                        class="input-wide"
                        type="text"
                        bind:value={settings.transcription.model_dir}
                      />
                      <button
                        class="btn-tertiary"
                        type="button"
                        on:click={async () => {
                          const dir = await pickDirectory();
                          if (dir && settings) {
                            settings = {
                              ...settings,
                              transcription: { ...settings.transcription, model_dir: dir },
                            };
                          }
                        }}
                      >
                        Choose
                      </button>
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="thread-count">Thread count</label>
                      <p class="settings-hint">Set to 0 to auto-detect.</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="thread-count"
                        class="input-compact"
                        type="number"
                        min="0"
                        step="1"
                        bind:value={settings.transcription.threads}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="custom-vocab">Custom vocab</label>
                      <p class="settings-hint">Bias phrases, acronyms, or names to improve accuracy.</p>
                    </div>
                    <div class="settings-control">
                      <textarea
                        id="custom-vocab"
                        class="input-textarea"
                        rows="3"
                        placeholder="e.g. Dr. Rivera, Q4 OKRs, SyncCraft"
                        bind:value={settings.transcription.custom_vocab}
                      ></textarea>
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="live-preview">Live preview</label>
                      <p class="settings-hint">Show partial transcription while recording (uses extra CPU).</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="live-preview"
                        type="checkbox"
                        bind:checked={settings.ui.live_preview_enabled}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="recording-hud">Recording pill</label>
                      <p class="settings-hint">Show a floating recording control above the dock while recording.</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="recording-hud"
                        type="checkbox"
                        bind:checked={settings.ui.recording_hud_enabled}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                        <label for="use-gpu">GPU acceleration</label>
                        <p class="settings-hint">
                          {performanceInfo?.gpu_supported
                            ? 'Use GPU when available to speed up transcription.'
                            : 'GPU acceleration is not available in this build.'}
                          {#if performanceInfo?.gpu_name}
                            <br />
                            Detected: {performanceInfo.gpu_name}
                          {/if}
                        </p>
                      </div>
                      <div class="settings-control">
                      <input
                        id="use-gpu"
                        type="checkbox"
                        bind:checked={settings.transcription.use_gpu}
                        disabled={!performanceInfo?.gpu_supported}
                      />
                    </div>
                  </div>
                  {#if performanceInfo?.gpu_error}
                    <div class="settings-row">
                      <div class="settings-label">
                        <span class="settings-title">GPU error</span>
                        <p class="settings-hint">Last GPU init failure (CPU fallback active).</p>
                      </div>
                      <div class="settings-control">
                        <span class="error-text">{performanceInfo.gpu_error}</span>
                      </div>
                    </div>
                  {/if}
                  <div class="settings-row">
                    <div class="settings-label">
                      <span class="settings-title">Benchmark</span>
                      <p class="settings-hint">Measure how fast your current model runs.</p>
                    </div>
                    <div class="settings-control">
                      <button
                        class="btn-secondary"
                        type="button"
                        disabled={benchmarking}
                        on:click={handleBenchmark}
                      >
                        {benchmarking ? 'Benchmarking...' : 'Run benchmark'}
                      </button>
                    </div>
                  </div>
                  {#if benchmarkResult}
                    <div class="settings-row">
                      <div class="settings-label">
                        <span class="settings-title">Last result</span>
                      </div>
                      <div class="settings-control">
                        <div class="benchmark-result">
                          <span>{benchmarkResult.realtime_factor.toFixed(2)}x realtime</span>
                          <span>
                            {benchmarkResult.audio_seconds.toFixed(1)}s audio in
                            {(benchmarkResult.duration_ms / 1000).toFixed(2)}s
                          </span>
                        </div>
                      </div>
                    </div>
                  {/if}
                  {#if benchmarkError}
                    <div class="settings-row">
                      <div class="settings-label">
                        <span class="settings-title">Benchmark error</span>
                      </div>
                      <div class="settings-control">
                        <span class="error-text">{benchmarkError}</span>
                      </div>
                    </div>
                  {/if}
                </div>
              </div>

              <div class={`settings-section ${settingsFocus === 'audio' ? 'focused' : ''}`} bind:this={audioSectionEl}>
                <h2>Audio</h2>
                <div class="settings-card">
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="input-device">Input device</label>
                    </div>
                    <div class="settings-control">
                      <select
                        id="input-device"
                        class="select-wide"
                        bind:value={settings.audio.input_device_id}
                        on:change={handleAudioDeviceChange}
                      >
                        {#each audioDevices as device}
                          <option value={device.id}>{device.name}</option>
                        {/each}
                      </select>
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="input-gain">Input gain (dB)</label>
                      <p class="settings-hint">Boost or attenuate the recording level.</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="input-gain"
                        class="input-compact"
                        type="number"
                        min="-12"
                        max="12"
                        step="0.5"
                        bind:value={settings.audio.input_gain_db}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="sample-rate">Sample rate</label>
                      <p class="settings-hint">16 kHz recommended for speech.</p>
                    </div>
                    <div class="settings-control">
                      <select
                        id="sample-rate"
                        class="select-compact"
                        bind:value={settings.audio.sample_rate_hz}
                        aria-label="Sample rate"
                      >
                        <option value={8000}>8 kHz</option>
                        <option value={16000}>16 kHz</option>
                        <option value={22050}>22.05 kHz</option>
                        <option value={24000}>24 kHz</option>
                        <option value={44100}>44.1 kHz</option>
                        <option value={48000}>48 kHz</option>
                      </select>
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="channels">Channels</label>
                      <p class="settings-hint">Mono is recommended for transcription.</p>
                    </div>
                    <div class="settings-control">
                      <select id="channels" class="select-compact" bind:value={settings.audio.channels}>
                        <option value={1}>Mono</option>
                        <option value={2}>Stereo</option>
                      </select>
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="noise-gate">Noise gate</label>
                      <p class="settings-hint">Ignore quiet audio while recording</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="noise-gate"
                        type="checkbox"
                        bind:checked={settings.audio.noise_gate_enabled}
                      />
                    </div>
                  </div>
                  {#if settings.audio.noise_gate_enabled}
                    <div class="settings-row">
                      <div class="settings-label">
                        <label for="noise-threshold">Gate threshold</label>
                        <p class="settings-hint">Lower values are more sensitive</p>
                      </div>
                      <div class="settings-control">
                        <input
                          id="noise-threshold"
                          class="input-center"
                          type="number"
                          min="0"
                          max="0.1"
                          step="0.005"
                          bind:value={settings.audio.noise_gate_threshold}
                        />
                      </div>
                    </div>
                  {/if}
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="vad-enabled">Auto-pause on silence</label>
                      <p class="settings-hint">Pause capture when speech stops, resume automatically.</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="vad-enabled"
                        type="checkbox"
                        bind:checked={settings.audio.vad_enabled}
                      />
                    </div>
                  </div>
                  {#if settings.audio.vad_enabled}
                    <div class="settings-row">
                      <div class="settings-label">
                        <label for="vad-threshold">Speech threshold</label>
                        <p class="settings-hint">Lower values detect quieter speech.</p>
                      </div>
                      <div class="settings-control">
                        <input
                          id="vad-threshold"
                          class="input-center"
                          type="number"
                          min="0"
                          max="0.1"
                          step="0.005"
                          bind:value={settings.audio.vad_threshold}
                        />
                      </div>
                    </div>
                    <div class="settings-row">
                      <div class="settings-label">
                        <label for="vad-silence">Silence before pause (ms)</label>
                      </div>
                      <div class="settings-control">
                        <input
                          id="vad-silence"
                          class="input-center"
                          type="number"
                          min="100"
                          step="50"
                          bind:value={settings.audio.vad_silence_ms}
                        />
                      </div>
                    </div>
                    <div class="settings-row">
                      <div class="settings-label">
                        <label for="vad-resume">Speech before resume (ms)</label>
                      </div>
                      <div class="settings-control">
                        <input
                          id="vad-resume"
                          class="input-center"
                          type="number"
                          min="50"
                          step="50"
                          bind:value={settings.audio.vad_resume_ms}
                        />
                      </div>
                    </div>
                  {/if}
                </div>
              </div>

              <div class="settings-section">
                <h2>Storage</h2>
                <div class="settings-card">
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="data-dir">Data directory</label>
                      <p class="settings-hint">Transcripts, clips, and audio files.</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="data-dir"
                        class="input-wide"
                        type="text"
                        bind:value={settings.storage.data_dir}
                      />
                      <button
                        class="btn-tertiary"
                        type="button"
                        on:click={async () => {
                          const dir = await pickDirectory();
                          if (dir && settings) {
                            settings = {
                              ...settings,
                              storage: { ...settings.storage, data_dir: dir },
                            };
                          }
                        }}
                      >
                        Choose
                      </button>
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="keep-audio">Keep audio recordings</label>
                      <p class="settings-hint">Store original audio files alongside transcripts.</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="keep-audio"
                        type="checkbox"
                        bind:checked={settings.storage.keep_audio}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="retention-days">Auto-delete after days</label>
                      <p class="settings-hint">Set to 0 to keep forever.</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="retention-days"
                        class="input-center"
                        type="number"
                        min="0"
                        step="1"
                        bind:value={settings.storage.retention_days}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <span class="settings-title">Storage usage</span>
                      <p class="settings-hint">
                        {storageStats
                          ? `${formatBytes(storageStats.data_bytes)} data · ${formatBytes(storageStats.model_bytes)} models`
                          : 'Loading...'}
                      </p>
                    </div>
                    <div class="settings-control">
                      <button
                        class="btn-secondary"
                        type="button"
                        on:click={async () => {
                          try {
                            storageStats = await getStorageStats();
                          } catch {}
                        }}
                      >
                        Refresh
                      </button>
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <span class="settings-title">Cleanup</span>
                      <p class="settings-hint">Remove all transcripts from history.</p>
                    </div>
                    <div class="settings-control">
                      <button
                        class="btn-danger"
                        type="button"
                        disabled={transcripts.length === 0 || clearingTranscripts}
                        on:click={() => (clearConfirmOpen = true)}
                      >
                        Clear transcripts
                      </button>
                    </div>
                  </div>
                </div>
              </div>

              <div class={`settings-section ${settingsFocus === 'app' ? 'focused' : ''}`} bind:this={appSectionEl}>
                <h2>App</h2>
                <div class="settings-card">
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="launch-login">Launch on login</label>
                      <p class="settings-hint">Start Whispr automatically when you sign in.</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="launch-login"
                        type="checkbox"
                        bind:checked={settings.app.launch_on_login}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="start-in-tray">Start in tray</label>
                      <p class="settings-hint">Launch minimized to the tray on startup.</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="start-in-tray"
                        type="checkbox"
                        bind:checked={settings.app.start_in_tray}
                      />
                    </div>
                  </div>
                  {#if runtimeInfo?.session_type === 'windows'}
                    <div class="settings-row">
                      <div class="settings-label">
                        <label for="close-to-tray">Close to tray</label>
                        <p class="settings-hint">Closing the window hides the app instead of quitting.</p>
                      </div>
                      <div class="settings-control">
                        <input
                          id="close-to-tray"
                          type="checkbox"
                          bind:checked={settings.app.close_to_tray}
                        />
                      </div>
                    </div>
                  {/if}
                  <div class="settings-row">
                    <div class="settings-label">
                      <span class="settings-title">Onboarding</span>
                      <p class="settings-hint">Replay the quick start tour.</p>
                    </div>
                    <div class="settings-control">
                      <button
                        class="btn-secondary"
                        type="button"
                        on:click={() => {
                          if (settings) {
                            settings = {
                              ...settings,
                              ui: { ...settings.ui, onboarding_seen: false },
                            };
                          }
                          onboardingOpen = true;
                          onboardingStep = 0;
                        }}
                      >
                        Reset onboarding
                      </button>
                    </div>
                  </div>
                </div>
              </div>

              <div class="settings-section">
                <h2>Diagnostics</h2>
                <div class="settings-card">
                  <div class="settings-row">
                    <div class="settings-label">
                      <span>Session</span>
                      <p class="settings-hint">Display server and hotkey support.</p>
                    </div>
                    <div class="settings-control">
                      <span class="diag-value">
                        {runtimeInfo?.session_type ?? 'unknown'} · {runtimeInfo?.hotkeys_supported ? 'hotkeys ok' : 'hotkeys limited'}
                      </span>
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <span>Paste method</span>
                      <p class="settings-hint">
                        {runtimeInfo?.missing_helpers?.length
                          ? `Missing helpers: ${runtimeInfo.missing_helpers.join(', ')}`
                          : 'Helpers available'}
                      </p>
                    </div>
                    <div class="settings-control">
                      <span class="diag-value">{runtimeInfo?.paste_method ?? 'unknown'}</span>
                      {#if wlClipboardMissing}
                        <button
                          class="btn-tertiary"
                          type="button"
                          on:click={() => (wlClipboardInstallOpen = true)}
                        >
                          Install wl-clipboard
                        </button>
                      {/if}
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <span>Active model</span>
                    </div>
                    <div class="settings-control">
                      <span class="diag-value">{settings.transcription.model}</span>
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <span>Acceleration</span>
                    </div>
                    <div class="settings-control">
                      <span class="diag-value">
                        {performanceInfo?.gpu_supported
                          ? performanceInfo.gpu_enabled ? 'GPU enabled' : 'CPU only'
                          : 'CPU only'}
                        · {performanceInfo?.thread_count ?? 'auto'} threads
                      </span>
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <span>Input device</span>
                    </div>
                    <div class="settings-control">
                      <span class="diag-value">
                        {audioDevices.find((device) => device.id === settings.audio.input_device_id)?.name ?? settings.audio.input_device_id}
                      </span>
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <span>App version</span>
                    </div>
                    <div class="settings-control">
                      <span class="diag-value">{currentVersion || 'unknown'}</span>
                    </div>
                  </div>
                </div>
              </div>

              <div class="settings-section">
                <h2>Appearance</h2>
                <div class="settings-card">
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="compact-list">Compact transcript list</label>
                      <p class="settings-hint">Reduce spacing in the transcript list.</p>
                    </div>
                    <div class="settings-control">
                      <input
                        id="compact-list"
                        type="checkbox"
                        bind:checked={settings.ui.list_compact}
                      />
                    </div>
                  </div>
                  <div class="settings-row">
                    <div class="settings-label">
                      <label for="theme-select">Theme</label>
                    </div>
                    <div class="settings-control">
                      <select
                        id="theme-select"
                        class="select-compact"
                        value={$theme}
                        on:change={handleThemeChange}
                      >
                        <option value="system">System</option>
                        <option value="light">Light</option>
                        <option value="dark">Dark</option>
                      </select>
                    </div>
                  </div>
                </div>
              </div>

              <div class="settings-actions">
                <button class="btn-primary" type="button" on:click={handleSaveSettings} disabled={savingSettings}>
                  {savingSettings ? 'Saving...' : 'Save settings'}
                </button>
              </div>
            {:else}
              <div class="loading-state">Settings unavailable.</div>
            {/if}
          </div>
        </div>
      {/if}
    </section>
  </div>

    <Onboarding
      open={onboardingOpen}
      step={onboardingStep}
      settings={settings}
      runtimeInfo={runtimeInfo}
      audioDevices={audioDevices}
      errorMessage={onboardingErrorMessage}
      on:advance={advanceOnboardingStep}
      on:selectAudioDevice={(event) => handleOnboardingAudioDeviceSelect(event.detail.id)}
      on:back={backOnboardingStep}
      on:skipStep={skipOnboardingStep}
      on:skipAll={skipOnboarding}
      on:finish={finishOnboarding}
    />

  <ConfirmDialog
    open={clearConfirmOpen}
    title="Clear all transcripts?"
    message="This removes every transcript from Recent."
    confirmLabel={clearingTranscripts ? 'Clearing...' : 'Delete all'}
    cancelLabel="Cancel"
    destructive={true}
    on:confirm={() => {
      clearConfirmOpen = false;
      handleClearTranscripts();
    }}
    on:cancel={() => (clearConfirmOpen = false)}
  />

  <ConfirmDialog
    open={deleteConfirmModel !== null}
    title="Delete Model"
    message={`Delete "${deleteConfirmModel?.label}"? This cannot be undone.`}
    confirmLabel="Delete"
    destructive={true}
    on:confirm={() => {
      if (deleteConfirmModel) {
        handleModelAction(() => deleteModel(deleteConfirmModel.id), deleteConfirmModel.id);
      }
      deleteConfirmModel = null;
    }}
    on:cancel={() => (deleteConfirmModel = null)}
  />

    <ConfirmDialog
      open={deleteConfirmTranscript !== null}
      title="Delete transcript?"
      message={`Delete "${deleteConfirmTranscript ? resolveTitle(deleteConfirmTranscript) : ''}"?`}
      confirmLabel="Delete"
      cancelLabel="Cancel"
      destructive={true}
      on:confirm={() => {
        if (deleteConfirmTranscript) {
          handleDeleteTranscript(deleteConfirmTranscript);
        }
        deleteConfirmTranscript = null;
      }}
      on:cancel={() => (deleteConfirmTranscript = null)}
    />

    {#if wlClipboardInstallOpen}
      <div class="modal-backdrop" role="presentation">
        <button
          class="modal-dismiss"
          type="button"
          aria-label="Close wl-clipboard install"
          on:click={closeWlClipboardInstall}
        ></button>
        <div
          class="modal-card helper-install"
          role="dialog"
          aria-modal="true"
          aria-labelledby="wlcb-title"
        >
          <div class="modal-header">
            <div>
              <h2 id="wlcb-title">Install wl-clipboard</h2>
              <p class="modal-summary">
                Needed on Wayland for clipboard integration. This installs <code class="code-hint">wl-copy</code> and
                <code class="code-hint">wl-paste</code>.
              </p>
            </div>
            <button class="icon-button" type="button" aria-label="Close" on:click={closeWlClipboardInstall}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
                <path d="M18 6L6 18" />
                <path d="M6 6l12 12" />
              </svg>
            </button>
          </div>
          <div class="modal-body">
            <div class="helper-install-list">
              {#each wlClipboardInstallCommands as entry}
                <div class="helper-install-item">
                  <div class="helper-install-row">
                    <span class="helper-install-label">{entry.label}</span>
                    <button
                      class="btn-tertiary"
                      type="button"
                      on:click={() => copyInstallCommand(entry.command)}
                    >
                      Copy
                    </button>
                  </div>
                  <pre class="helper-install-code"><code>{entry.command}</code></pre>
                </div>
              {/each}
            </div>
            {#if wlClipboardCopyError}
              <p class="helper-install-error">{wlClipboardCopyError} Select the command above and copy it manually.</p>
            {/if}
            <p class="settings-hint">After installing, relaunch Whispr to re-detect helpers.</p>
          </div>
          <div class="modal-footer">
            <div class="modal-actions">
              <button class="btn-secondary" type="button" on:click={closeWlClipboardInstall}>Close</button>
              <button
                class="btn-primary"
                type="button"
                on:click={async () => {
                  try {
                    await relaunch();
                  } catch (error) {
                    wlClipboardCopyError = error instanceof Error ? error.message : 'Failed to relaunch.';
                  }
                }}
              >
                Relaunch app
              </button>
            </div>
          </div>
        </div>
      </div>
    {/if}

    {#if expandedTranscript}
      <div class="modal-backdrop" role="presentation">
        <button
          class="modal-dismiss"
        type="button"
        aria-label="Close transcript details"
        on:click={closeTranscriptDetail}
      ></button>
      <div
        class="modal-card"
        role="dialog"
        aria-modal="true"
        aria-labelledby="detail-title"
      >
        <div class="modal-header">
          <div>
            <h2 id="detail-title">{resolveTitle(expandedTranscript)}</h2>
            <div class="modal-meta">
              <span>{formatTimestamp(expandedTranscript.created_at)}</span>
              <span>&bull;</span>
              <span>{formatDuration(expandedTranscript.duration_ms)}</span>
            </div>
            {#if expandedTranscript.summary}
              <p class="modal-summary">{expandedTranscript.summary}</p>
            {/if}
          </div>
          <button class="icon-button" type="button" aria-label="Close" on:click={closeTranscriptDetail}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
              <path d="M18 6L6 18" />
              <path d="M6 6l12 12" />
            </svg>
          </button>
        </div>
        <div class="modal-body">
          <div class="modal-tags">
            {#each tagOptions as tag}
              <button
                class={`tag-chip ${expandedTranscript.tags.includes(tag) ? 'active' : ''}`}
                type="button"
                on:click={() => toggleTranscriptTag(expandedTranscript, tag)}
              >
                {tag}
              </button>
            {/each}
          </div>
          <textarea
            class="modal-textarea"
            bind:value={detailDraft}
            bind:this={detailTextareaEl}
            placeholder="Edit transcript..."
          ></textarea>
        </div>
          <div class="modal-footer">
            <div class="modal-copy">
              <p>Edit the text above before copying</p>
              <select class="select-compact" bind:value={copyFormat}>
                <option value="plain">Plain</option>
                <option value="markdown">Markdown</option>
                <option value="bullets">Bullet list</option>
              </select>
            </div>
            <div class="modal-actions">
              <button
                class="btn-tertiary"
                type="button"
                on:click={handleSaveTranscriptEdits}
                disabled={!detailDirty || savingTranscript}
              >
                {savingTranscript ? 'Saving...' : transcriptSaved && !detailDirty ? 'Saved' : 'Save changes'}
              </button>
              <button
                class="btn-tertiary"
                type="button"
                on:click={() => expandedTranscript && handleExportTranscript(expandedTranscript, detailDraft)}
              >
                Export
              </button>
              <button class="btn-tertiary" type="button" on:click={handleSaveClip}>
                {clipSavedId ? 'Clip Saved' : 'Save Clip'}
              </button>
              <button class="btn-secondary" type="button" on:click={closeTranscriptDetail}>
                Close
            </button>
            <button class="btn-primary" type="button" on:click={handleDetailCopy}>
              {detailCopied ? 'Copied!' : 'Copy to Clipboard'}
            </button>
          </div>
        </div>
      </div>
    </div>
  {/if}

</main>
