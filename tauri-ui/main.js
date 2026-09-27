const invoke = window.__TAURI__.core.invoke;
const appWindow = window.__TAURI__.window.getCurrentWindow();
const {
  t,
  tBold,
  describeClient,
  planOf,
  discordUser,
  resetParts,
  formatTime,
  formatElapsed,
  capitalize,
  describeCredits,
  sparkline,
  peakOf,
} = window.RpcShared;

const presets = {
  claude: ['Claude', 'https://claude.ai'],
  desktop: ['Claude Desktop', 'https://claude.ai/download'],
  repo: ['GitHub Repo', 'https://github.com/inerthel-agi/claude-rpc'],
};

// Variables accepted by the Discord text templates (presence.rs render_template).
const TEMPLATE_VARS = [
  '{model}', '{effort}', '{plan}', '{provider}', '{limits}', '{limit5h}',
  '{limitWeekly}', '{limitFable}', '{sessions}', '{project}', '{client}', '{mode}',
];
const LIMIT_KEYS = { '5h': 'tray.5h', All: 'tray.weekly', Fable: 'tray.fable' };
const PAGES = ['presence', 'usage', 'privacy', 'buttons', 'general', 'about'];
const PAGE_KEY = 'claude-rpc-page';

// Release notes are stored when an update is installed and shown once after
// the restart, as a "What's new" card.
const WHATS_NEW_KEY = 'claude-rpc-whats-new';

const $ = (selector) => document.querySelector(selector);

const fields = {
  modeButtons: [...document.querySelectorAll('.mode[data-mode]')],
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
  startupToggle: $('#startup-toggle'),
  privateProjects: $('#private-projects'),
  detailsTemplate: $('#details-template'),
  stateTemplate: $('#state-template'),
  templateVars: $('#template-vars'),
  language: $('#language'),
  refreshLimits: $('#refresh-limits'),
  labels: [$('#label0'), $('#label1')],
  urls: [$('#url0'), $('#url1')],
  errors: [$('#error0'), $('#error1')],
  clear: $('#clear'),
  presetButtons: [...document.querySelectorAll('[data-preset]')],
  watchingNotice: $('#watching-notice'),
  switchWatching: $('#switch-watching'),
  navItems: [...document.querySelectorAll('.nav-item[data-page]')],
  pages: [...document.querySelectorAll('.page[data-page]')],
  content: $('#content'),
  sideClient: $('#side-client'),
  sideClientLabel: $('#side-client-label'),
  sideDiscord: $('#side-discord'),
  sideVersion: $('#side-version'),
  navUpdateBadge: $('#nav-update-badge'),
  message: $('#message'),
  updateBanner: $('#update-banner'),
  updateText: $('#update-text'),
  updateNotes: $('#update-notes'),
  updateNow: $('#update-now'),
  versionLabel: $('#version-label'),
  updateStatus: $('#update-status'),
  checkUpdate: $('#check-update'),
  whatsNew: $('#whats-new'),
  whatsNewTitle: $('#whats-new-title'),
  whatsNewNotes: $('#whats-new-notes'),
  whatsNewDismiss: $('#whats-new-dismiss'),
  copyDiagnostic: $('#copy-diagnostic'),
  diagnosticMessage: $('#diagnostic-message'),
  diagnosticText: $('#diagnostic-text'),
  preview: $('#preview'),
  previewActivity: $('#preview-activity'),
  previewPrimary: $('#preview-primary'),
  previewSecondary: $('#preview-secondary'),
  previewTertiary: $('#preview-tertiary'),
  previewElapsed: $('#preview-elapsed'),
  previewButtons: $('#preview-buttons'),
  usagePlan: $('#usage-plan'),
  usageLimits: $('#usage-limits'),
  usageEmpty: $('#usage-empty'),
  themeButtons: [...document.querySelectorAll('[data-theme-option]')],
};

// Two-state controls (switches and check chips) keep their state in
// data-enabled; the look comes from the .active class.
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
let saving = false;
// Keys the tray menu can also change; see save().
const TRAY_KEYS = ['dnd', 'rpcMode'];
const touchedTrayKeys = new Set();
let refreshing = false;
// Text under "Copy diagnostic": a string key or a raw error.
let diagnosticMessage = { key: 'app.diagnosticHint', text: '' };
// '' | 'downloading' | 'failed:<error>' for the update banner.
let updateInstall = '';
let lastStatusJson = '';
let polling = false;
let currentMode = 'playing';
let currentConfig = {};
// Last config seen on disk; the tray menu can change DND, the pause or the
// activity type while this window is open, so the form re-reads it instead of
// overwriting it.
let lastConfigJson = '';
let messageState = { kind: '', key: 'footer.auto', time: null };
let appVersion = '';
let lastTemplateInput = fields.stateTemplate;
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
    button.setAttribute('aria-checked', String(button.dataset.mode === currentMode));
  });
}

function showPage(page) {
  const current = PAGES.includes(page) ? page : 'presence';
  fields.navItems.forEach((item) => {
    if (item.dataset.page === current) item.setAttribute('aria-current', 'page');
    else item.removeAttribute('aria-current');
  });
  fields.pages.forEach((section) => {
    section.hidden = section.dataset.page !== current;
  });
  fields.content.scrollTop = 0;
  try {
    localStorage.setItem(PAGE_KEY, current);
  } catch {
    /* storage unavailable: start on Presence next time */
  }
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

// texts: also fill the text fields. Only on load: afterwards they are owned by
// this window (the tray never changes them), and rewriting them from disk
// would erase a half-typed button or move the caret.
function writeForm(config, texts = true) {
  currentConfig = config || {};
  setMode(currentConfig.rpcMode || 'playing');
  for (const [button, key, fallback] of toggleBindings) {
    setToggle(button, key in currentConfig ? !!currentConfig[key] : fallback);
  }
  if (texts) {
    for (let i = 0; i < 2; i += 1) {
      fields.labels[i].value = currentConfig.buttons?.[i]?.label || '';
      fields.urls[i].value = currentConfig.buttons?.[i]?.url || '';
    }
    fields.privateProjects.value = (currentConfig.privateProjects || []).join('\n');
    fields.detailsTemplate.value = currentConfig.detailsTemplate || '';
    fields.stateTemplate.value = currentConfig.stateTemplate || '';
  }
  fields.language.value = currentConfig.language || 'auto';
  setLanguage(fields.language.value);
  syncDependents();
  validateButtons();
  renderStatus(currentStatus);
}

// Re-applies every piece of text that is not a static [data-i18n] string.
function setLanguage(setting) {
  window.RpcShared.setLanguage(setting);
  renderMessage();
  renderUpdateBanner();
  renderUpdateStatus();
  renderRefreshButton();
  renderDiagnosticMessage();
  renderStatus(currentStatus);
}

// Controls that only make sense when another one is on.
function syncDependents() {
  fields.planToggle.disabled = !isOn(fields.providerToggle);
  const limits = isOn(fields.limitsToggle);
  [fields.limit5hToggle, fields.limitAllToggle, fields.limitFableToggle].forEach((button) => {
    button.disabled = !limits;
  });
  const watching = currentMode === 'watching';
  fields.watchingNotice.hidden = watching;
  for (const input of [...fields.labels, ...fields.urls]) input.disabled = !watching;
  [...fields.presetButtons, fields.clear].forEach((button) => {
    button.disabled = !watching;
  });
}

// Mirror config.rs::clean_url: only complete http(s) buttons reach Discord.
function validateButtons() {
  for (let i = 0; i < 2; i += 1) {
    const label = fields.labels[i].value.trim();
    const url = fields.urls[i].value.trim();
    const badUrl = !!url && !/^https?:\/\//.test(url);
    const incomplete = !badUrl && !!label !== !!url;
    fields.urls[i].classList.toggle('invalid', badUrl);
    fields.errors[i].hidden = !badUrl && !incomplete;
    fields.errors[i].textContent = badUrl ? t('buttons.badUrl') : incomplete ? t('buttons.incomplete') : '';
  }
}

// kind: 'ok' (check mark), 'error', or '' for progress messages.
function showMessage(key, kind = '', vars = {}) {
  messageState = { key, kind, vars, time: kind === 'ok' ? new Date() : null };
  renderMessage();
}

function showError(error) {
  messageState = { key: null, kind: 'error', text: String(error) };
  renderMessage();
}

function renderMessage() {
  const { key, kind, vars, time, text } = messageState;
  fields.message.textContent = key
    ? t(key, time ? { ...vars, time: formatTime(time) } : vars)
    : text || '';
  fields.message.classList.toggle('error', kind === 'error');
  fields.message.classList.toggle('ok', kind === 'ok');
}

// Picks up what the tray changed (DND, pause, activity type) while this
// window is open. Never while an edit is pending or being saved.
async function syncConfigFromDisk() {
  if (loading || saving || saveTimer) return;
  let config;
  try {
    config = await invoke('load_config');
  } catch {
    return;
  }
  // An edit may have started while the file was being read.
  if (loading || saving || saveTimer) return;
  const json = JSON.stringify(config);
  if (json !== lastConfigJson) {
    lastConfigJson = json;
    writeForm(config, false);
  }
}

async function refreshLimits() {
  try {
    if (saveTimer) {
      clearTimeout(saveTimer);
      saveTimer = null;
      await save();
    }
    await invoke('refresh_limits');
    refreshing = true;
    renderRefreshButton();
    setTimeout(() => {
      refreshStatus();
      refreshing = false;
      renderRefreshButton();
    }, 1500);
  } catch (error) {
    showError(error);
  }
}

function renderRefreshButton() {
  fields.refreshLimits.disabled = refreshing;
  fields.refreshLimits.textContent = t(refreshing ? 'usage.refreshing' : 'usage.refresh');
}

function renderDiagnosticMessage() {
  fields.diagnosticMessage.textContent = diagnosticMessage.key
    ? t(diagnosticMessage.key)
    : diagnosticMessage.text;
}

async function refreshStartup() {
  try {
    const state = await invoke('tray_state');
    appVersion = state.appVersion || appVersion;
    setToggle(fields.startupToggle, !!state.startOnWindows);
  } catch {
    /* tray state unavailable; keep the last value */
  }
  fields.sideVersion.textContent = appVersion ? t('side.version', { version: appVersion }) : '';
  $('#about-version').textContent = appVersion ? t('general.version', { version: appVersion }) : '';
  renderUpdateStatus();
}

async function load() {
  let page = 'presence';
  try {
    page = localStorage.getItem(PAGE_KEY) || page;
  } catch {
    /* storage unavailable */
  }
  showPage(page);
  renderTemplateVars();
  try {
    let theme = 'dark';
    try {
      theme = localStorage.getItem('claude-rpc-theme') || 'dark';
    } catch {
      /* storage unavailable */
    }
    applyTheme(theme);
    await invoke('start_daemon');
    const config = await invoke('load_config');
    lastConfigJson = JSON.stringify(config);
    writeForm(config);
    await refreshStatus();
    await refreshStartup();
    loading = false;
  } catch (error) {
    showError(error);
    loading = false;
  }
  showWhatsNew();
  checkForUpdate();
}

let pendingUpdate = null;
// '' | 'checking' | 'current' | 'failed:<error>'
let updateCheck = '';

async function checkForUpdate(manual = false) {
  let info = null;
  if (!manual) {
    try {
      info = await invoke('pending_update');
    } catch {
      info = null;
    }
  }
  if (!info) {
    if (manual) {
      updateCheck = 'checking';
      renderUpdateStatus();
    }
    try {
      info = await invoke('check_update');
      updateCheck = 'current';
    } catch (error) {
      info = null;
      updateCheck = manual ? `failed:${error}` : '';
    }
  }
  pendingUpdate = info && info.version ? info : null;
  renderUpdateBanner();
  renderUpdateStatus();
}

function renderUpdateBanner() {
  fields.updateBanner.hidden = !pendingUpdate;
  fields.navUpdateBadge.hidden = !pendingUpdate;
  if (!pendingUpdate) return;
  fields.updateText.textContent =
    updateInstall === 'downloading'
      ? t('update.downloading')
      : updateInstall.startsWith('failed:')
        ? t('update.failed', { error: updateInstall.slice(7) })
        : t('update.available', { version: pendingUpdate.version });
  const notes = (pendingUpdate.notes || '').trim();
  fields.updateNotes.textContent = notes;
  fields.updateNotes.hidden = !notes;
}

function renderUpdateStatus() {
  fields.versionLabel.textContent = appVersion ? t('general.version', { version: appVersion }) : '';
  fields.checkUpdate.disabled = updateCheck === 'checking';
  if (pendingUpdate) {
    fields.updateStatus.textContent = t('update.available', { version: pendingUpdate.version });
  } else if (updateCheck === 'checking') {
    fields.updateStatus.textContent = t('general.checking');
  } else if (updateCheck.startsWith('failed:')) {
    fields.updateStatus.textContent = t('general.checkFailed', { error: updateCheck.slice(7) });
  } else if (updateCheck === 'current') {
    fields.updateStatus.textContent = t('general.upToDate');
  } else {
    fields.updateStatus.textContent = '';
  }
}

fields.updateNow.addEventListener('click', async () => {
  updateInstall = 'downloading';
  renderUpdateBanner();
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
    updateInstall = `failed:${error}`;
    renderUpdateBanner();
    fields.updateNow.disabled = false;
  }
});

fields.checkUpdate.addEventListener('click', () => checkForUpdate(true));

async function showWhatsNew() {
  let stored = null;
  try {
    stored = JSON.parse(localStorage.getItem(WHATS_NEW_KEY) || 'null');
  } catch {
    stored = null;
  }
  if (!stored) return;
  // Only once the installed version matches the update that was downloaded.
  if (!appVersion || stored.version !== appVersion) return;
  fields.whatsNewTitle.textContent = t('whatsnew.title', { version: appVersion });
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
  try {
    const report = await invoke('diagnostic');
    fields.diagnosticText.value = report;
    try {
      // A clipboard that never answers must not leave the button disabled.
      await Promise.race([
        navigator.clipboard.writeText(report),
        new Promise((_, reject) => {
          setTimeout(() => reject(new Error('clipboard timeout')), 3000);
        }),
      ]);
      fields.diagnosticText.hidden = true;
      diagnosticMessage = { key: 'app.copied', text: '' };
    } catch {
      fields.diagnosticText.hidden = false;
      fields.diagnosticText.focus();
      fields.diagnosticText.select();
      diagnosticMessage = { key: 'app.copyFailed', text: '' };
    }
  } catch (error) {
    diagnosticMessage = { key: '', text: String(error) };
  } finally {
    renderDiagnosticMessage();
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
  // Rebuild the usage bars and sidebar only when the status changed. The
  // preview still updates: its timer ticks and a pause can end meanwhile.
  const json = JSON.stringify(currentStatus);
  if (json === lastStatusJson) {
    updatePreview();
    return;
  }
  lastStatusJson = json;
  renderStatus(currentStatus);
}

function renderStatus(status) {
  renderSidebar(status);
  renderUsage(status);
  renderCredits(status.credits);
  updatePreview();
}

function renderSidebar(status) {
  const client = describeClient(status);
  fields.sideClient.classList.toggle('on', client.running);
  fields.sideClientLabel.textContent = client.running ? client.label : t('client.notRunning');
  const user = discordUser(status);
  fields.sideDiscord.classList.toggle('on', !!user);
  const dot = document.createElement('i');
  const text = document.createElement('span');
  text.replaceChildren(...(user ? ['Discord · ', ...tBold('{user}', { user })] : [t('side.discordOff')]));
  fields.sideDiscord.replaceChildren(dot, text);
}

const clampPercent = (value) => Math.max(0, Math.min(100, Number(value) || 0));

function renderUsage(status) {
  const plan = planOf(status);
  fields.usagePlan.textContent = plan;
  fields.usagePlan.hidden = !plan;
  fields.usagePlan.parentElement.hidden = !plan;

  const limits = Array.isArray(status.limits) ? status.limits : [];
  fields.usageEmpty.hidden = limits.length > 0;
  fields.usageLimits.replaceChildren(
    ...limits.map((limit) => {
      const percent = clampPercent(limit.usedPercent);
      const block = document.createElement('div');
      const head = document.createElement('div');
      head.className = 'limit-head';
      const label = document.createElement('span');
      label.textContent = LIMIT_KEYS[limit.label] ? t(LIMIT_KEYS[limit.label]) : limit.label;
      const value = document.createElement('b');
      const used = document.createElement('small');
      used.textContent = t('usage.used');
      value.append(`${percent}%`, used);
      head.append(label, value);
      const bar = document.createElement('div');
      bar.className = 'bar';
      const fill = document.createElement('i');
      fill.style.width = `${percent}%`;
      fill.classList.toggle('warn', percent >= 80 && percent < 95);
      fill.classList.toggle('danger', percent >= 95);
      bar.append(fill);
      block.append(head, bar);
      const reset = resetParts(limit.reset);
      if (reset) {
        const meta = document.createElement('div');
        meta.className = 'limit-meta';
        const relative = document.createElement('span');
        relative.textContent = capitalize(reset.relative);
        const absolute = document.createElement('span');
        absolute.textContent = reset.absolute;
        meta.append(relative, absolute);
        block.append(meta);
      }
      if (limit.label === '5h') {
        const svg = sparkline(status.history5h, 64);
        if (svg) {
          const chart = document.createElement('div');
          chart.className = 'chart';
          const chartHead = document.createElement('div');
          chartHead.className = 'chart-head';
          const title = document.createElement('span');
          title.textContent = t('tray.last24h');
          const peak = document.createElement('span');
          peak.textContent = t('tray.peak', { percent: peakOf(status.history5h) });
          chartHead.append(title, peak);
          chart.append(chartHead, svg);
          block.append(chart);
        }
      }
      return block;
    }),
  );
}

// Usage credits (extra usage) on the Usage page. Private: never sent to Discord.
function renderCredits(raw) {
  const credits = describeCredits(raw);
  $('#credits-section').hidden = !credits;
  if (!credits) return;
  const on = credits.enabled;
  $('#credits-used').textContent = on ? credits.used || '—' : t('credits.off');
  $('#credits-of').textContent = !on
    ? ''
    : credits.limit
      ? t('credits.of', { limit: credits.limit })
      : t('credits.month');
  $('#credits-bar').hidden = !on || credits.percent === null;
  const fill = $('#credits-fill');
  fill.style.width = `${credits.percent || 0}%`;
  fill.classList.toggle('warn', credits.percent >= 80 && credits.percent < 95);
  fill.classList.toggle('danger', credits.percent >= 95);
  $('#credits-note').textContent = on
    ? [
        t(credits.limit ? 'credits.hint' : 'credits.hintNoCap'),
        credits.currency ? t('credits.billed', { currency: credits.currency }) : '',
      ]
        .filter(Boolean)
        .join(' ')
    : t('credits.offHint');
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
  fields.preview.classList.toggle('off', !!hidden);
  if (hidden) {
    fields.previewActivity.textContent = t('preview.notShown');
    fields.previewPrimary.textContent = hidden;
    fields.previewSecondary.textContent = t('preview.nothing');
    fields.previewTertiary.textContent = '';
    fields.previewElapsed.textContent = '';
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
  renderElapsed();
  // The limits tooltip belongs on the line that actually carries the state text:
  // tertiary in playing mode, secondary otherwise (tertiary is then the static
  // "Powered by Anthropic" line).
  const limitsTip = currentStatus.limitsLine || '';
  fields.previewSecondary.title = playing ? '' : limitsTip;
  fields.previewTertiary.title = playing ? limitsTip : '';
  renderPreviewButtons(daemonMode, config.buttons || []);
}

// Discord shows the session timer as "12:34:56 elapsed".
function renderElapsed() {
  const shown = !fields.preview.classList.contains('off') && currentStatus.startedAtMs;
  const text = shown
    ? t('preview.elapsed', { time: formatElapsed(currentStatus.startedAtMs, true) })
    : '';
  if (fields.previewElapsed.textContent !== text) fields.previewElapsed.textContent = text;
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

function renderTemplateVars() {
  fields.templateVars.replaceChildren(
    ...TEMPLATE_VARS.map((name) => {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'var';
      button.textContent = name;
      button.addEventListener('click', () => insertVariable(name));
      return button;
    }),
  );
}

// Inserts a variable at the caret of the template field used last.
function insertVariable(name) {
  const input = lastTemplateInput;
  const start = input.selectionStart ?? input.value.length;
  const end = input.selectionEnd ?? input.value.length;
  const value = `${input.value.slice(0, start)}${name}${input.value.slice(end)}`;
  if (value.length > input.maxLength) return;
  input.value = value;
  input.focus();
  input.setSelectionRange(start + name.length, start + name.length);
  scheduleSave();
}

// Saves on top of the config on disk: the tray may have changed DND, the pause
// or the activity type since this form was filled. Those keep the disk value
// unless they were changed in this window (the pause never is).
async function save() {
  saving = true;
  const changed = [...touchedTrayKeys];
  touchedTrayKeys.clear();
  try {
    const form = readForm();
    let disk = currentConfig;
    try {
      disk = await invoke('load_config');
    } catch {
      /* keep the last known values */
    }
    const config = { ...form, pauseUntilMs: disk.pauseUntilMs ?? 0 };
    for (const key of TRAY_KEYS) {
      if (!changed.includes(key) && key in disk) config[key] = disk[key];
    }
    await invoke('save_config', { config });
    const saved = await invoke('load_config');
    lastConfigJson = JSON.stringify(saved);
    // A new edit may have started during the save: do not revert it.
    if (saveTimer) currentConfig = saved;
    else writeForm(saved, false);
    showMessage('footer.saved', 'ok');
  } catch (error) {
    changed.forEach((key) => touchedTrayKeys.add(key));
    showError(error);
  } finally {
    saving = false;
  }
}

function scheduleSave() {
  if (loading) return;
  clearTimeout(saveTimer);
  showMessage('footer.saving');
  syncDependents();
  validateButtons();
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
// Close button mirrors the footer Done / native X: hide to tray, never kill the daemon.
$('#titlebar-close').addEventListener('click', closeSettings);
fields.refreshLimits.addEventListener('click', refreshLimits);
// About page: the backend only opens its own fixed list of links.
document.querySelectorAll('[data-link]').forEach((row) => {
  row.addEventListener('click', () => {
    invoke('open_link', { name: row.dataset.link }).catch(showError);
  });
});
fields.navItems.forEach((item) => {
  item.addEventListener('click', () => showPage(item.dataset.page));
});
fields.clear.addEventListener('click', () => {
  for (const input of [...fields.labels, ...fields.urls]) input.value = '';
  scheduleSave();
});
fields.modeButtons.forEach((button) => {
  button.addEventListener('click', () => {
    setMode(button.dataset.mode);
    touchedTrayKeys.add('rpcMode');
    scheduleSave();
  });
});
fields.switchWatching.addEventListener('click', () => {
  setMode('watching');
  touchedTrayKeys.add('rpcMode');
  scheduleSave();
});
for (const [button, key] of toggleBindings) {
  button.addEventListener('click', () => {
    setToggle(button, !isOn(button));
    if (TRAY_KEYS.includes(key)) touchedTrayKeys.add(key);
    scheduleSave();
  });
}
fields.startupToggle.addEventListener('click', async () => {
  fields.startupToggle.disabled = true;
  try {
    const state = await invoke('tray_action', { action: 'startup' });
    setToggle(fields.startupToggle, !!state.startOnWindows);
  } catch (error) {
    showError(error);
  } finally {
    fields.startupToggle.disabled = false;
  }
});
for (const input of textInputs) input.addEventListener('input', scheduleSave);
for (const input of [fields.detailsTemplate, fields.stateTemplate]) {
  input.addEventListener('focus', () => {
    lastTemplateInput = input;
  });
}
fields.language.addEventListener('change', () => {
  setLanguage(fields.language.value);
  validateButtons();
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
  let saved = null;
  try {
    saved = localStorage.getItem('claude-rpc-theme');
  } catch {
    /* storage unavailable */
  }
  if (saved === 'system') applyTheme('system');
});

function applyTheme(theme) {
  const safeTheme = ['dark', 'system', 'light'].includes(theme) ? theme : 'dark';
  const resolved = safeTheme === 'system' ? (systemTheme.matches ? 'light' : 'dark') : safeTheme;
  document.body.dataset.theme = resolved;
  fields.themeButtons.forEach((button) => {
    const active = button.dataset.themeOption === safeTheme;
    button.classList.toggle('active', active);
    button.setAttribute('aria-checked', String(active));
  });
  try {
    localStorage.setItem('claude-rpc-theme', safeTheme);
  } catch {
    /* storage unavailable: the theme applies until the window closes */
  }
}

window.addEventListener('DOMContentLoaded', load);
// Skip polling while the window is hidden in the tray.
// One poll at a time, so slow answers never arrive out of order.
setInterval(async () => {
  if (document.hidden || polling) return;
  polling = true;
  try {
    await refreshStatus();
    await syncConfigFromDisk();
  } finally {
    polling = false;
  }
}, 1000);
