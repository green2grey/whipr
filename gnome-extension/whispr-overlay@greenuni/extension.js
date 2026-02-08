import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

const STATE_DIR = 'whispr';
const CONFIG_DIR = 'whispr';
const STATE_FILE = 'overlay.json';
const TRAY_FILE = 'tray.json';
const STATE_TMP_FILE = STATE_FILE.replace(/\.json$/i, '.tmp');
const TRAY_TMP_FILE = TRAY_FILE.replace(/\.json$/i, '.tmp');
const CONFIG_FILE = 'overlay-config.json';
const DEFAULT_COMMAND = 'whispr';
const MARGIN_BOTTOM = 32;
const BAR_COUNT = 10;
const BAR_MAX_HEIGHT = 26;
const BAR_MIN_HEIGHT = 10;
const BAR_WEIGHTS = [0.35, 0.55, 0.8, 1.0, 0.9, 0.9, 1.0, 0.8, 0.55, 0.35];
const DOUBLE_CLICK_MS = 220;
const SUCCESS_FLASH_MS = 3000;
const ERROR_FLASH_MS = 4000;
const STALE_STATE_MS = 5000;

export default class WhisprOverlayExtension extends Extension {
  enable() {
    this._bars = [];
    this._recording = false;
    this._startedAtMs = null;
    this._level = 0;
    this._levelTarget = 0;
    this._lastTranscriptAtMs = null;
    this._lastErrorAtMs = null;
    this._lastError = null;
    this._trayRecent = [];
    this._hotkeys = {};
    this._meterTimerId = null;
    this._clockTimerId = null;
    this._pollTimerId = null;
    this._clickTimerId = null;
    this._statusTimerId = null;
    this._loadOverlayTimerId = null;
    this._loadTrayTimerId = null;

    this._overlay = this._buildOverlay();
    this._overlay.hide();
    Main.layoutManager.addChrome(this._overlay);

    this._indicator = this._buildIndicator();
    this._buildMenu();
    Main.panel.addToStatusArea('whispr-indicator', this._indicator, 0, 'right');

    this._stylesheet = Gio.File.new_for_path(`${this.path}/stylesheet.css`);
    this._applyStylesheet();

    this._stateDirPath = GLib.build_filenamev([GLib.get_user_state_dir(), STATE_DIR]);
    this._statePath = GLib.build_filenamev([this._stateDirPath, STATE_FILE]);
    this._stateDir = Gio.File.new_for_path(this._stateDirPath);
    this._stateFile = Gio.File.new_for_path(this._statePath);
    this._trayPath = GLib.build_filenamev([this._stateDirPath, TRAY_FILE]);
    this._trayFile = Gio.File.new_for_path(this._trayPath);
    this._configPath = GLib.build_filenamev([GLib.get_user_config_dir(), CONFIG_DIR, CONFIG_FILE]);
    this._configFile = Gio.File.new_for_path(this._configPath);

    this._ensureStateDir();
    this._watchState();

    this._layoutSignals = [];
    this._layoutSignals.push(Main.layoutManager.connect('monitors-changed', () => this._positionOverlay()));

    this._loadOverlayState();
    this._loadTrayState();
  }

  disable() {
    this._stopMeter();
    this._stopClock();
    this._clearClickTimer();
    this._clearStatusTimer();
    this._clearLoadTimers();

    if (this._monitor) {
      if (this._monitorId) {
        this._monitor.disconnect(this._monitorId);
      }
      this._monitor.cancel();
      this._monitor = null;
    }

    if (this._pollTimerId) {
      GLib.source_remove(this._pollTimerId);
      this._pollTimerId = null;
    }

    if (this._layoutSignals) {
      this._layoutSignals.forEach((id) => Main.layoutManager.disconnect(id));
      this._layoutSignals = null;
    }

    if (this._overlay) {
      this._overlay.destroy();
      this._overlay = null;
    }

    if (this._indicator) {
      this._indicator.destroy();
      this._indicator = null;
      this._indicatorIcon = null;
      this._toggleItem = null;
      this._recentMenu = null;
      this._recentItems = null;
    }

    if (this._stylesheet) {
      this._removeStylesheet();
      this._stylesheet = null;
    }
  }

  _buildIndicator() {
    const button = new PanelMenu.Button(0.0, 'Whispr', false);
    button.add_style_class_name('whispr-indicator');

    this._indicatorIcon = new St.Icon({
      icon_name: 'audio-input-microphone-symbolic',
      style_class: 'system-status-icon whispr-indicator-icon',
    });

    button.add_child(this._indicatorIcon);
    button.connect('button-press-event', (_actor, event) => {
      const buttonId = event.get_button();
      if (buttonId === Clutter.BUTTON_PRIMARY) {
        this._handlePrimaryClick();
        if (button.menu) {
          button.menu.close();
        }
        return Clutter.EVENT_STOP;
      }
      if (buttonId === Clutter.BUTTON_SECONDARY) {
        if (button.menu) {
          button.menu.toggle();
        }
        return Clutter.EVENT_STOP;
      }
      return Clutter.EVENT_PROPAGATE;
    });

    return button;
  }

  _buildMenu() {
    if (!this._indicator) return;
    const menu = this._indicator.menu;
    menu.removeAll();

    this._toggleItem = new PopupMenu.PopupMenuItem('Start Recording');
    this._toggleItem.connect('activate', () => this._toggleRecording());
    menu.addMenuItem(this._toggleItem);

    this._recentMenu = new PopupMenu.PopupSubMenuMenuItem('Recent Transcriptions');
    menu.addMenuItem(this._recentMenu);

    menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

    this._settingsItem = new PopupMenu.PopupMenuItem('Settings');
    this._settingsItem.connect('activate', () => this._openSettings());
    menu.addMenuItem(this._settingsItem);

    this._openItem = new PopupMenu.PopupMenuItem('Open App');
    this._openItem.connect('activate', () => this._openApp());
    menu.addMenuItem(this._openItem);

    menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

    this._quitItem = new PopupMenu.PopupMenuItem('Quit');
    this._quitItem.connect('activate', () => this._quitApp());
    menu.addMenuItem(this._quitItem);

    this._refreshMenu();
  }

  _buildOverlay() {
    const container = new St.BoxLayout({
      style_class: 'whispr-overlay',
      reactive: true,
      x_align: Clutter.ActorAlign.CENTER,
    });

    const closeButton = new St.Button({
      style_class: 'whispr-overlay-button whispr-overlay-close',
      reactive: true,
      can_focus: true,
      accessible_name: 'Stop recording',
    });
    closeButton.set_child(new St.Icon({
      icon_name: 'window-close-symbolic',
      style_class: 'whispr-overlay-icon',
    }));
    closeButton.connect('clicked', () => this._toggleRecording());

    const meter = new St.BoxLayout({ style_class: 'whispr-overlay-meter' });
    for (let i = 0; i < BAR_COUNT; i += 1) {
      const bar = new St.Widget({ style_class: 'whispr-overlay-bar' });
      bar.set_height(BAR_MIN_HEIGHT);
      meter.add_child(bar);
      this._bars.push(bar);
    }

    this._statusLabel = new St.Label({
      style_class: 'whispr-overlay-label',
      text: 'Recording',
    });
    this._timeLabel = new St.Label({
      style_class: 'whispr-overlay-time',
      text: '0:00',
    });

    const textBox = new St.BoxLayout({
      style_class: 'whispr-overlay-text',
      vertical: true,
    });
    textBox.add_child(this._statusLabel);
    textBox.add_child(this._timeLabel);

    const stopButton = new St.Button({
      style_class: 'whispr-overlay-button whispr-overlay-stop',
      reactive: true,
      can_focus: true,
      accessible_name: 'Stop recording',
    });
    stopButton.set_child(new St.Icon({
      icon_name: 'media-playback-stop-symbolic',
      style_class: 'whispr-overlay-icon',
    }));
    stopButton.connect('clicked', () => this._toggleRecording());

    container.add_child(closeButton);
    container.add_child(meter);
    container.add_child(textBox);
    container.add_child(stopButton);

    return container;
  }

  _ensureStateDir() {
    try {
      this._stateDir.make_directory_with_parents(null);
    } catch (error) {
      // Directory already exists or is not writable.
    }
  }

  _watchState() {
    try {
      this._monitor = this._stateDir.monitor_directory(Gio.FileMonitorFlags.NONE, null);
      this._monitorId = this._monitor.connect('changed', (_monitor, file, otherFile, _eventType) => {
        const basenames = [];
        if (file) basenames.push(file.get_basename());
        if (otherFile) basenames.push(otherFile.get_basename());

        // The app writes atomically using a tmp file and rename:
        // overlay.json -> overlay.tmp -> overlay.json (same for tray.json).
        // Gio can report only the tmp file in 'file' and the final file in 'otherFile'.
        if (basenames.some((name) => name === STATE_FILE || name === STATE_TMP_FILE)) {
          this._scheduleLoadOverlay();
        }
        if (basenames.some((name) => name === TRAY_FILE || name === TRAY_TMP_FILE)) {
          this._scheduleLoadTray();
        }
      });
    } catch (error) {
      // If monitoring fails, fall back to a simple poll.
      this._monitor = null;
      this._monitorId = null;
      this._pollTimerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 1000, () => {
        this._loadOverlayState();
        this._loadTrayState();
        return true;
      });
    }
  }

  _scheduleLoadOverlay() {
    if (this._loadOverlayTimerId) return;
    this._loadOverlayTimerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 60, () => {
      this._loadOverlayTimerId = null;
      this._loadOverlayState();
      return GLib.SOURCE_REMOVE;
    });
  }

  _scheduleLoadTray() {
    if (this._loadTrayTimerId) return;
    this._loadTrayTimerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 120, () => {
      this._loadTrayTimerId = null;
      this._loadTrayState();
      return GLib.SOURCE_REMOVE;
    });
  }

  _clearLoadTimers() {
    if (this._loadOverlayTimerId) {
      GLib.source_remove(this._loadOverlayTimerId);
      this._loadOverlayTimerId = null;
    }
    if (this._loadTrayTimerId) {
      GLib.source_remove(this._loadTrayTimerId);
      this._loadTrayTimerId = null;
    }
  }

  _loadOverlayState() {
    let recording = false;
    let startedAtMs = null;
    let level = 0;
    let updatedAtMs = null;

    try {
      const [ok, contents] = this._stateFile.load_contents(null);
      if (ok) {
        const text = new TextDecoder('utf-8').decode(contents);
        const data = JSON.parse(text);
        recording = Boolean(data.recording);
        if (Number.isFinite(data.started_at_ms)) {
          startedAtMs = data.started_at_ms;
        }
        if (Number.isFinite(data.level)) {
          level = Math.max(0, Math.min(1, Number(data.level)));
        }
        if (Number.isFinite(data.updated_at_ms)) {
          updatedAtMs = data.updated_at_ms;
        }
      }
    } catch (error) {
      recording = false;
      startedAtMs = null;
      level = 0;
      updatedAtMs = null;
    }

    // If the app crashed or isn't running, stale state can leave the overlay "stuck".
    // Treat stale recording state as inactive.
    if (recording && updatedAtMs && Date.now() - updatedAtMs > STALE_STATE_MS) {
      recording = false;
      startedAtMs = null;
      level = 0;
    }

    this._levelTarget = level;

    if (recording) {
      this._showOverlay(startedAtMs);
    } else {
      this._hideOverlay();
    }
  }

  _showOverlay(startedAtMs) {
    this._recording = true;
    this._startedAtMs = startedAtMs;
    this._timeLabel.visible = Boolean(startedAtMs);
    this._overlay.show();
    this._positionOverlay();
    this._startMeter();
    this._startClock();
    this._updateIndicator();
    this._refreshMenu();
  }

  _hideOverlay() {
    this._recording = false;
    this._startedAtMs = null;
    this._timeLabel.visible = false;
    this._level = 0;
    this._levelTarget = 0;
    this._overlay.hide();
    this._stopMeter();
    this._stopClock();
    this._updateIndicator();
    this._refreshMenu();
  }

  _positionOverlay() {
    if (!this._overlay) return;
    const monitor = Main.layoutManager.primaryMonitor;
    if (!monitor) return;

    const [, naturalWidth] = this._overlay.get_preferred_width(-1);
    const [, naturalHeight] = this._overlay.get_preferred_height(-1);

    const x = Math.round(monitor.x + (monitor.width - naturalWidth) / 2);
    const y = Math.round(monitor.y + monitor.height - naturalHeight - MARGIN_BOTTOM);

    this._overlay.set_position(x, y);
  }

  _startMeter() {
    if (this._meterTimerId) return;
    this._meterTimerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 150, () => {
      const delta = this._levelTarget - this._level;
      const smoothing = delta > 0 ? 0.45 : 0.2;
      this._level = this._level + delta * smoothing;
      const clamped = Math.max(0, Math.min(1, this._level));

      this._bars.forEach((bar, index) => {
        const weight = BAR_WEIGHTS[index % BAR_WEIGHTS.length];
        const height = Math.round(BAR_MIN_HEIGHT + (BAR_MAX_HEIGHT - BAR_MIN_HEIGHT) * clamped * weight);
        bar.set_height(height);
      });
      return true;
    });
  }

  _stopMeter() {
    if (this._meterTimerId) {
      GLib.source_remove(this._meterTimerId);
      this._meterTimerId = null;
    }
  }

  _startClock() {
    if (!this._startedAtMs) {
      this._timeLabel.text = '';
      return;
    }
    this._updateClock();
    if (this._clockTimerId) return;
    this._clockTimerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 1000, () => {
      this._updateClock();
      return true;
    });
  }

  _stopClock() {
    if (this._clockTimerId) {
      GLib.source_remove(this._clockTimerId);
      this._clockTimerId = null;
    }
  }

  _updateClock() {
    if (!this._startedAtMs) return;
    const elapsedSec = Math.max(0, Math.floor((Date.now() - this._startedAtMs) / 1000));
    const minutes = Math.floor(elapsedSec / 60);
    const seconds = elapsedSec % 60;
    this._timeLabel.text = `${minutes}:${seconds.toString().padStart(2, '0')}`;
  }

  _updateIndicator() {
    if (!this._indicatorIcon) return;
    this._indicatorIcon.remove_style_class_name('whispr-indicator-recording');
    this._indicatorIcon.remove_style_class_name('whispr-indicator-success');
    this._indicatorIcon.remove_style_class_name('whispr-indicator-error');

    if (this._recording) {
      this._indicatorIcon.icon_name = 'media-record-symbolic';
      this._indicatorIcon.add_style_class_name('whispr-indicator-recording');
      return;
    }

    const now = Date.now();
    if (this._lastErrorAtMs && now - this._lastErrorAtMs < ERROR_FLASH_MS) {
      this._indicatorIcon.icon_name = 'dialog-warning-symbolic';
      this._indicatorIcon.add_style_class_name('whispr-indicator-error');
      this._scheduleStatusRefresh();
      return;
    }

    if (this._lastTranscriptAtMs && now - this._lastTranscriptAtMs < SUCCESS_FLASH_MS) {
      this._indicatorIcon.icon_name = 'emblem-ok-symbolic';
      this._indicatorIcon.add_style_class_name('whispr-indicator-success');
      this._scheduleStatusRefresh();
      return;
    }

    this._indicatorIcon.icon_name = 'audio-input-microphone-symbolic';
    this._clearStatusTimer();
  }

  _toggleRecording() {
    const command = this._resolveCommand();
    GLib.spawn_command_line_async(`${GLib.shell_quote(command)} --toggle`);
  }

  _openApp() {
    const command = this._resolveCommand();
    GLib.spawn_command_line_async(`${GLib.shell_quote(command)} --show`);
  }

  _openSettings() {
    const command = this._resolveCommand();
    GLib.spawn_command_line_async(`${GLib.shell_quote(command)} --show-settings`);
  }

  _quitApp() {
    const command = this._resolveCommand();
    GLib.spawn_command_line_async(`${GLib.shell_quote(command)} --quit`);
  }

  _handlePrimaryClick() {
    if (this._clickTimerId) {
      this._clearClickTimer();
      this._openApp();
      return;
    }

    this._clickTimerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, DOUBLE_CLICK_MS, () => {
      this._clickTimerId = null;
      this._toggleRecording();
      return GLib.SOURCE_REMOVE;
    });
  }

  _clearClickTimer() {
    if (this._clickTimerId) {
      GLib.source_remove(this._clickTimerId);
      this._clickTimerId = null;
    }
  }

  _scheduleStatusRefresh() {
    this._clearStatusTimer();
    const now = Date.now();
    let nextDelay = null;
    if (this._lastErrorAtMs) {
      nextDelay = ERROR_FLASH_MS - (now - this._lastErrorAtMs);
    } else if (this._lastTranscriptAtMs) {
      nextDelay = SUCCESS_FLASH_MS - (now - this._lastTranscriptAtMs);
    }
    if (nextDelay === null || nextDelay <= 0) return;
    this._statusTimerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, nextDelay, () => {
      this._statusTimerId = null;
      this._updateIndicator();
      return GLib.SOURCE_REMOVE;
    });
  }

  _clearStatusTimer() {
    if (this._statusTimerId) {
      GLib.source_remove(this._statusTimerId);
      this._statusTimerId = null;
    }
  }

  _loadTrayState() {
    let trayRecent = [];
    let lastTranscriptAtMs = null;
    let lastErrorAtMs = null;
    let lastError = null;
    let hotkeys = {};

    try {
      const [ok, contents] = this._trayFile.load_contents(null);
      if (ok) {
        const text = new TextDecoder('utf-8').decode(contents);
        const data = JSON.parse(text);
        if (Array.isArray(data.recent)) {
          trayRecent = data.recent;
        }
        if (Number.isFinite(data.last_transcript_at_ms)) {
          lastTranscriptAtMs = data.last_transcript_at_ms;
        }
        if (Number.isFinite(data.last_error_at_ms)) {
          lastErrorAtMs = data.last_error_at_ms;
        }
        if (typeof data.last_error === 'string') {
          lastError = data.last_error;
        }
        if (data.hotkeys && typeof data.hotkeys === 'object') {
          hotkeys = data.hotkeys;
        }
      }
    } catch (error) {
      trayRecent = [];
      lastTranscriptAtMs = null;
      lastErrorAtMs = null;
      lastError = null;
      hotkeys = {};
    }

    this._trayRecent = trayRecent;
    this._lastTranscriptAtMs = lastTranscriptAtMs;
    this._lastErrorAtMs = lastErrorAtMs;
    this._lastError = lastError;
    this._hotkeys = hotkeys;
    this._refreshMenu();
    this._updateIndicator();
  }

  _refreshMenu() {
    if (this._toggleItem) {
      const label = this._recording ? 'Stop Recording' : 'Start Recording';
      const hotkey = this._hotkeys?.record_toggle;
      this._toggleItem.label.text = hotkey ? `${label}  (${hotkey})` : label;
    }

    if (this._openItem) {
      const hotkey = this._hotkeys?.open_app;
      this._openItem.label.text = hotkey ? `Open App  (${hotkey})` : 'Open App';
    }

    if (this._recentMenu) {
      this._recentMenu.menu.removeAll();
      if (!this._trayRecent || this._trayRecent.length === 0) {
        const emptyItem = new PopupMenu.PopupMenuItem('No recent transcripts', false);
        this._recentMenu.menu.addMenuItem(emptyItem);
      } else {
        this._trayRecent.forEach((item) => {
          const preview = typeof item.preview === 'string' && item.preview.length > 0
            ? item.preview
            : (typeof item.text === 'string' ? item.text : '');
          const label = preview.length > 0 ? preview : 'Untitled transcript';
          const menuItem = new PopupMenu.PopupMenuItem(label);
          menuItem.connect('activate', () => this._copyTranscript(item));
          this._recentMenu.menu.addMenuItem(menuItem);
        });
        this._recentMenu.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        const showAll = new PopupMenu.PopupMenuItem('Show All Transcripts');
        showAll.connect('activate', () => this._openApp());
        this._recentMenu.menu.addMenuItem(showAll);
      }
    }
  }

  _copyTranscript(item) {
    const text = typeof item.text === 'string' ? item.text : '';
    if (!text) return;
    const clipboard = St.Clipboard.get_default();
    clipboard.set_text(St.ClipboardType.CLIPBOARD, text);
  }

  _applyStylesheet() {
    const theme = St.ThemeContext.get_for_stage(global.stage).get_theme();
    if (typeof theme.load_stylesheet === 'function') {
      theme.load_stylesheet(this._stylesheet);
      return;
    }
    if (typeof theme.add_stylesheet === 'function') {
      theme.add_stylesheet(this._stylesheet);
    }
  }

  _removeStylesheet() {
    const theme = St.ThemeContext.get_for_stage(global.stage).get_theme();
    if (typeof theme.unload_stylesheet === 'function') {
      theme.unload_stylesheet(this._stylesheet);
      return;
    }
    if (typeof theme.remove_stylesheet === 'function') {
      theme.remove_stylesheet(this._stylesheet);
    }
  }

  _resolveCommand() {
    const envCommand = GLib.getenv('WHISPR_BIN');
    if (envCommand && envCommand.trim().length > 0) {
      return envCommand.trim();
    }

    const configCommand = this._readConfigCommand();
    if (configCommand) {
      return configCommand;
    }

    return DEFAULT_COMMAND;
  }

  _readConfigCommand() {
    try {
      const [ok, contents] = this._configFile.load_contents(null);
      if (!ok) return null;
      const text = new TextDecoder('utf-8').decode(contents);
      const data = JSON.parse(text);
      if (typeof data.binary === 'string' && data.binary.trim().length > 0) {
        return data.binary.trim();
      }
    } catch (error) {
      return null;
    }
    return null;
  }
}
