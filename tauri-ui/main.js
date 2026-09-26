const invoke = window.__TAURI__.core.invoke;
const appWindow = window.__TAURI__.window.getCurrentWindow();
const { t, inner, formatTime } = window.RpcShared;

const presets = {
  claude: ['Claude', 'https://claude.ai'],
  desktop: ['Claude Desktop', 'https://claude.ai/download'],
  repo: ['GitHub Repo', 'https://github.com/inerthel-agi/claude-rpc'],
};

// Release notes are stored when an update is installed and shown once after
// the restart, as a "What's new" card.
const WHATS_NEW_KEY = 'claude-rpc-whats-new';

const $ = (selector) => document.querySelector(selector);

const fields = {
  modeButtons: [...document.querySelectorAll('.seg[data-mode]')],
  dndToggle: $('#dnd-toggle'),
  providerToggle: $('#provider-toggle'),
  planToggle: $('#plan-toggle'),
  effortToggle: $('#effort-toggle'),
  sessionsToggle: $('#sessions-toggle'),
  modelIconToggle: $('#model-icon-toggle'),
  limitsToggle: $('#limits-toggle'),
  limit5hToggle: $('#limit-5h-toggle'),
  limitAllToggle: $('#limit-all-toggle'),
  limitFableToggle: $('#limit-fable-toggle'),
  alert80Toggle: $('#alert-80-toggle'),
  alert95Toggle: $('#alert-95-toggle'),
  alertResetToggle: $('#alert-reset-toggle'),
  hideChatToggle: $('#hide-chat-toggle'),
  privateProjects: $('#private-projects'),
  detailsTemplate: $('#details-template'),
  stateTemplate: $('#state-template'),
  language: $('#language'),
  refreshLimits: $('#refresh-limits'),
  labels: [$('#label0'), $('#label1')],
  urls: [$('#url0'), $('#url1')],
  clear: $('#clear'),
  presetButtons: [...document.querySelectorAll('[data-preset]')],
  tileClaude: $('#tile-claude'),
  tileModel: $('#tile-model'),
  tilePlan: $('#tile-plan'),
  tileDiscord: $('#tile-discord'),
  tileDiscordDot: $('#tile-discord-dot'),
  message: $('#message'),
  updateBanner: $('#update-banner'),
  updateText: $('#update-text'),
  updateNotes: $('#update-notes'),
  updateNow: $('#update-now'),
  whatsNew: $('#whats-new'),
  whatsNewTitle: $('#whats-new-title'),
  whatsNewNotes: $('#whats-new-notes'),
  whatsNewDismiss: $('#whats-new-dismiss'),
  copyDiagnostic: $('#copy-diagnostic'),
  diagnosticMessage: $('#diagnostic-message'),
  diagnosticText: $('#diagnostic-text'),
  previewActivity: $('#preview-activity'),
  previewPrimary: $('#preview-primary'),
  previewSecondary: $('#preview-secondary'),
  previewTertiary: $('#preview-tertiary'),
  previewButtons: $('#preview-buttons'),
  themeButtons: [...document.querySelectorAll('[data-theme-option]')],
};

// Two-state controls (switches and check chips) keep their state in
// data-enabled; the checkbox/switch look comes from the .active class.
// [control, config key, default when the key is missing]
const toggleBindings = [
  [fields.dndToggle, 'dnd', false],
  [fields.providerToggle, 'showProvider', true],
  [fields.planToggle, 'showPlan', true],
  [fields.effortToggle, 'showEffort', true],
  [fields.sessionsToggle, 'showSessions', false],
  [fields.modelIconToggle, 'modelIcon', false],
  [fields.limitsToggle, 'showLimits', true],
  [fields.limit5hToggle, 'showLimit5h', true],
  [fields.limitAllToggle, 'showLimitAll', true],
  [fields.limitFableToggle, 'showLimitFable', true],
  [fields.alert80Toggle, 'alert80', true],
  [fields.alert95Toggle, 'alert95', true],
  [fields.alertResetToggle, 'alertReset', true],
  [fields.hideChatToggle, 'hideInChat', false],
];
const textInputs = [
  ...fields.labels,
  ...fields.urls,
  fields.privateProjects,
  fields.detailsTemplate,
  fields.stateTemplate,
];

let loading = true;
let saveTimer = null;
let currentMode = 'playing';
let currentConfig = {};
// Last config seen on disk; the tray menu can change DND, the pause or the
// activity type while this window is open, so the form re-reads it instead of
// overwriting it.
let lastConfigJson = '';
let messageKind = 'ok';
let currentStatus = {
  claudeLine: 'Claude: Off',
  modelLine: 'Auto-detect',
  providerLine: 'Provider: Unknown',
  discordLine: 'Discord: RPC disabled',
};

const isOn = (button) => button.dataset.enabled === 'true';

function setToggle(button, enabled) {
  button.dataset.enabled = String(enabled);
  button.classList.toggle('active', enabled);
  button.setAttribute('aria-checked', String(enabled));
}

function setMode(mode) {
  currentMode = ['playing', 'watching', 'listening', 'competing'].includes(mode) ? mode : 'playing';
  fields.modeButtons.forEach((button) => {
    const active = button.dataset.mode === currentMode;
    button.classList.toggle('active', active);
    button.setAttribute('aria-checked', String(active));
  });
}

function readForm() {
  const buttons = [];
  for (let i = 0; i < 2; i += 1) {
    const label = fields.labels[i].value.trim();
    const url = fields.urls[i].value.trim();
    if (label && url) buttons.push({ label, url });
  }
  const config = {
    ...currentConfig,
    rpcMode: currentMode,
    buttons,
    privateProjects: fields.privateProjects.value
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean),
    detailsTemplate: fields.detailsTemplate.value.trim(),
    stateTemplate: fields.stateTemplate.value.trim(),
    language: fields.language.value,
  };
  for (const [button, key] of toggleBindings) config[key] = isOn(button);
  return config;
}

function writeForm(config) {
  currentConfig = config || {};
  setMode(currentConfig.rpcMode || 'playing');
  for (const [button, key, fallback] of toggleBindings) {
    setToggle(button, key in currentConfig ? !!currentConfig[key] : fallback);
  }
  for (let i = 0; i < 2; i += 1) {
    fields.labels[i].value = currentConfig.buttons?.[i]?.label || '';
    fields.urls[i].value = currentConfig.buttons?.[i]?.url || '';
  }
  fields.privateProjects.value = (currentConfig.privateProjects || []).join('\n');
  fields.detailsTemplate.value = currentConfig.detailsTemplate || '';
  fields.stateTemplate.value = currentConfig.stateTemplate || '';
  fields.language.value = currentConfig.language || 'auto';
  window.RpcShared.setLanguage(fields.language.value);
  syncDependents();
  renderTiles(currentStatus);
  updatePreview();
}

// Controls that only make sense when another one is on.
function syncDependents() {
  fields.planToggle.disabled = !isOn(fields.providerToggle);
  const limits = isOn(fields.limitsToggle);
  [fields.limit5hToggle, fields.limitAllToggle, fields.limitFableToggle].forEach((button) => {
    button.disabled = !limits;
  });
  const watching = currentMode === 'watching';
  for (const input of [...fields.labels, ...fields.urls]) input.disabled = !watching;
  [...fields.presetButtons, fields.clear].forEach((button) => {
    button.disabled = !watching;
  });
}

// kind: 'ok' (shows the check mark), 'error', or '' for progress messages.
function showMessage(text, kind = '') {
  messageKind = kind;
  fields.message.textContent = text;
  fields.message.classList.toggle('error', kind === 'error');
  fields.message.classList.toggle('ok', kind === 'ok');
}

async function syncConfigFromDisk() {
  const tag = document.activeElement && document.activeElement.tagName;
  const typing = tag === 'INPUT' || tag === 'TEXTAREA';
  if (loading || saveTimer || typing) return;
  let config;
  try {
    config = await invoke('load_config');
  } catch {
    return;
  }
  const json = JSON.stringify(config);
  if (json !== lastConfigJson) {
    lastConfigJson = json;
    writeForm(config);
  }
}

async function refreshLimits() {
  try {
    const config = readForm();
    await invoke('save_config', { config });
    currentConfig = config;
    await invoke('refresh_limits');
    showMessage(t('footer.refreshing'));
    setTimeout(() => {
      refreshStatus();
      showMessage(t('footer.saved'), 'ok');
    }, 1500);
  } catch (error) {
    showMessage(String(error), 'error');
  }
}

async function load() {
  try {
    applyTheme(localStorage.getItem('claude-rpc-theme') || 'dark');
    await invoke('start_daemon');
    const config = await invoke('load_config');
    lastConfigJson = JSON.stringify(config);
    writeForm(config);
    await refreshStatus();
    loading = false;
  } catch (error) {
    showMessage(String(error), 'error');
    loading = false;
  }
  showWhatsNew();
  checkForUpdate();
}

let pendingUpdate = null;

async function checkForUpdate() {
  let info = null;
  try {
    info = await invoke('pending_update');
  } catch {
    info = null;
  }
  if (!info) {
    try {
      info = await invoke('check_update');
    } catch {
      info = null;
    }
  }
  pendingUpdate = info && info.version ? info : null;
  renderUpdateBanner();
}

function renderUpdateBanner() {
  fields.updateBanner.hidden = !pendingUpdate;
  if (!pendingUpdate) return;
  fields.updateText.textContent = t('update.available', { version: pendingUpdate.version });
  const notes = (pendingUpdate.notes || '').trim();
  fields.updateNotes.textContent = notes;
  fields.updateNotes.hidden = !notes;
}

fields.updateNow.addEventListener('click', async () => {
  fields.updateText.textContent = t('update.downloading');
  fields.updateNow.disabled = true;
  try {
    localStorage.setItem(
      WHATS_NEW_KEY,
      JSON.stringify({ version: pendingUpdate.version, notes: pendingUpdate.notes || '' }),
    );
  } catch {
    /* storage unavailable: skip the card */
  }
  try {
    await invoke('install_update');
  } catch (error) {
    fields.updateText.textContent = t('update.failed', { error });
    fields.updateNow.disabled = false;
  }
});

async function showWhatsNew() {
  let stored = null;
  try {
    stored = JSON.parse(localStorage.getItem(WHATS_NEW_KEY) || 'null');
  } catch {
    stored = null;
  }
  if (!stored) return;
  let version = '';
  try {
    version = (await invoke('tray_state')).appVersion;
  } catch {
    return;
  }
  // Only once the installed version matches the update that was downloaded.
  if (stored.version !== version) return;
  fields.whatsNewTitle.textContent = t('whatsnew.title', { version });
  fields.whatsNewNotes.textContent = stored.notes || '';
  fields.whatsNewNotes.hidden = !stored.notes;
  fields.whatsNew.hidden = false;
}

fields.whatsNewDismiss.addEventListener('click', () => {
  fields.whatsNew.hidden = true;
  try {
    localStorage.removeItem(WHATS_NEW_KEY);
  } catch {
    /* ignore */
  }
});

fields.copyDiagnostic.addEventListener('click', async () => {
  fields.copyDiagnostic.disabled = true;
  fields.diagnosticMessage.textContent = '';
  try {
    const report = await invoke('diagnostic');
    fields.diagnosticText.value = report;
    try {
      await navigator.clipboard.writeText(report);
      fields.diagnosticText.hidden = true;
      fields.diagnosticMessage.textContent = t('app.copied');
    } catch {
      fields.diagnosticText.hidden = false;
      fields.diagnosticText.focus();
      fields.diagnosticText.select();
      fields.diagnosticMessage.textContent = t('app.copyFailed');
    }
  } catch (error) {
    fields.diagnosticMessage.textContent = String(error);
  } finally {
    fields.copyDiagnostic.disabled = false;
  }
});

async function refreshStatus() {
  try {
    currentStatus = await invoke('load_status');
  } catch {
    currentStatus = {
      claudeLine: 'Claude: Off',
      modelLine: 'Auto-detect',
      providerLine: 'Provider: Unknown',
      discordLine: 'Discord: RPC disabled',
    };
  }
  renderTiles(currentStatus);
  updatePreview();
}

function renderTiles(status) {
  const client = (status.claudeLine || '').replace(/^Claude:\s*/, '');
  const running = client && client !== 'Off';
  fields.tileClaude.textContent = !running
    ? t('tile.notRunning')
    : client.startsWith('Desktop')
      ? ['Desktop', inner(status.claudeLine, 'Claude: Desktop')].filter(Boolean).join(' · ')
      : client.startsWith('CLI')
        ? 'Claude Code'
        : client;

  const model = running ? (status.modelLine || '').split(' | ')[0] : '';
  fields.tileModel.textContent =
    model && model !== 'Auto-detect' ? model.replace(/^Claude\s+/, '') : '—';

  const provider = (status.providerLine || '').replace(/^Provider:\s*/, '');
  const plan = inner(status.providerLine, 'Provider:') || provider.split(' (')[0];
  fields.tilePlan.textContent = plan && plan !== 'Unknown' ? plan : '—';

  const connected = (status.discordLine || '').startsWith('Discord: Connected');
  fields.tileDiscordDot.classList.toggle('ok', connected);
  fields.tileDiscord.textContent = connected
    ? inner(status.discordLine, 'Discord:') || t('tile.connected')
    : t('tile.notConnected');
}

// Why the card is not on Discord right now, or '' when it is.
function hiddenReason(config) {
  if (config.dnd) return t('preview.dnd');
  if ((config.pauseUntilMs || 0) > Date.now()) {
    return t('preview.paused', { time: formatTime(new Date(config.pauseUntilMs)) });
  }
  if (currentStatus.hiddenReason === 'private') return t('preview.private');
  if (currentStatus.hiddenReason === 'chat') return t('preview.chat');
  if (!currentStatus.previewHeader) return t('preview.notRunning');
  return '';
}

function updatePreview() {
  const config = readForm();
  // Nothing reaches Discord while paused, hidden or with no Claude client
  // running; say so instead of showing a card that is not actually published.
  const hidden = hiddenReason(config);
  if (hidden) {
    fields.previewActivity.textContent = t('preview.notShown');
    fields.previewPrimary.textContent = hidden;
    fields.previewSecondary.textContent = t('preview.nothing');
    fields.previewTertiary.textContent = '';
    fields.previewSecondary.title = '';
    fields.previewTertiary.title = '';
    renderPreviewButtons('playing', []);
    return;
  }
  // Drive the whole card from the daemon's authoritative layout so header, body,
  // buttons and tooltip never disagree during the save lag.
  const daemonMode = effectivePreviewMode(currentStatus.previewHeader, currentMode);
  const playing = daemonMode === 'playing';
  fields.previewActivity.textContent = currentStatus.previewHeader;
  fields.previewPrimary.textContent = currentStatus.previewPrimary || '';
  fields.previewSecondary.textContent = currentStatus.previewSecondary || '';
  fields.previewTertiary.textContent = currentStatus.previewTertiary || '';
  // The limits tooltip belongs on the line that actually carries the state text:
  // tertiary in playing mode, secondary otherwise (tertiary is then the static
  // "Powered by Anthropic" line).
  const limitsTip = currentStatus.limitsLine || '';
  fields.previewSecondary.title = playing ? '' : limitsTip;
  fields.previewTertiary.title = playing ? limitsTip : '';
  renderPreviewButtons(daemonMode, config.buttons || []);
}

// Mirror the mode the daemon actually rendered (from its preview header) so the
// preview's button visibility and tooltip placement track the daemon-sourced body
// instead of the unsaved form.
function effectivePreviewMode(previewHeader, fallbackMode) {
  if (!previewHeader) return fallbackMode;
  if (previewHeader.startsWith('Watching')) return 'watching';
  if (previewHeader.startsWith('Listening')) return 'listening';
  if (previewHeader.startsWith('Competing')) return 'competing';
  return 'playing';
}

function renderPreviewButtons(mode, buttons) {
  // Mirror config.rs::clean_url: only http(s) buttons reach Discord.
  const valid = buttons.filter((button) => /^https?:\/\//.test(button.url));
  fields.previewButtons.replaceChildren();
  fields.previewButtons.hidden = mode !== 'watching' || valid.length === 0;
  if (fields.previewButtons.hidden) return;
  for (const button of valid.slice(0, 2)) {
    const item = document.createElement('span');
    item.textContent = button.label;
    fields.previewButtons.appendChild(item);
  }
}

async function save() {
  try {
    const config = readForm();
    await invoke('save_config', { config });
    currentConfig = config;
    lastConfigJson = JSON.stringify(await invoke('load_config'));
    showMessage(t('footer.saved'), 'ok');
    updatePreview();
  } catch (error) {
    showMessage(String(error), 'error');
  }
}

function scheduleSave() {
  if (loading) return;
  clearTimeout(saveTimer);
  showMessage(t('footer.saving'));
  syncDependents();
  updatePreview();
  saveTimer = setTimeout(() => {
    saveTimer = null;
    save();
  }, 300);
}

// Settings save automatically; flush a pending edit before hiding the window.
async function closeSettings() {
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = null;
    await save();
  }
  invoke('close_settings');
}

$('#close').addEventListener('click', closeSettings);
$('#titlebar-minimize').addEventListener('click', () => appWindow.minimize());
$('#titlebar-maximize').addEventListener('click', () => appWindow.toggleMaximize());
// Close button mirrors the footer Close / native X: hide to tray, never kill the daemon.
$('#titlebar-close').addEventListener('click', closeSettings);
fields.refreshLimits.addEventListener('click', refreshLimits);
fields.clear.addEventListener('click', () => {
  for (const input of [...fields.labels, ...fields.urls]) input.value = '';
  scheduleSave();
});
fields.modeButtons.forEach((button) => {
  button.addEventListener('click', () => {
    setMode(button.dataset.mode);
    scheduleSave();
  });
});
for (const [button] of toggleBindings) {
  button.addEventListener('click', () => {
    setToggle(button, !isOn(button));
    scheduleSave();
  });
}
for (const input of textInputs) input.addEventListener('input', scheduleSave);
fields.language.addEventListener('change', () => {
  window.RpcShared.setLanguage(fields.language.value);
  renderTiles(currentStatus);
  renderUpdateBanner();
  showMessage(t(messageKind === 'ok' ? 'footer.saved' : 'footer.saving'), messageKind);
  scheduleSave();
});
fields.themeButtons.forEach((button) => {
  button.addEventListener('click', () => applyTheme(button.dataset.themeOption));
});
fields.presetButtons.forEach((button) => {
  button.addEventListener('click', () => {
    const [label, url] = presets[button.dataset.preset];
    const slot = fields.labels[0].value.trim() ? 1 : 0;
    fields.labels[slot].value = label;
    fields.urls[slot].value = url;
    scheduleSave();
  });
});

const systemTheme = window.matchMedia('(prefers-color-scheme: light)');
// "System" follows Windows when its theme changes while the window is open.
systemTheme.addEventListener('change', () => {
  if (localStorage.getItem('claude-rpc-theme') === 'system') applyTheme('system');
});

function applyTheme(theme) {
  const safeTheme = ['dark', 'system', 'light'].includes(theme) ? theme : 'dark';
  const resolved = safeTheme === 'system' ? (systemTheme.matches ? 'light' : 'dark') : safeTheme;
  document.body.dataset.theme = resolved;
  fields.themeButtons.forEach((button) => {
    const active = button.dataset.themeOption === safeTheme;
    button.classList.toggle('active', active);
    button.setAttribute('aria-pressed', String(active));
  });
  localStorage.setItem('claude-rpc-theme', safeTheme);
}

window.addEventListener('DOMContentLoaded', load);
// Skip polling while the window is hidden in the tray.
setInterval(() => {
  if (document.hidden) return;
  refreshStatus();
  syncConfigFromDisk();
}, 1000);
