const invoke = window.__TAURI__.core.invoke;
const { t, inner, formatReset, formatTime } = window.RpcShared;

const $ = (selector) => document.querySelector(selector);
const menu = $('#menu');
const versionEl = $('#version');
const startupItem = $('#startup');
const updateItem = $('#update');
const updateLabel = $('#update-label');
const updateBadge = $('#update-badge');
const discordDot = $('#discord-dot');
const discordText = $('#discord-text');
const modelName = $('#model-name');
const modelChip = $('#model-chip');
const contextEl = $('#context');
const alertEl = $('#alert');
const alertText = $('#alert-text');
const limitsEl = $('#limits');
const pauseLabel = $('#pause-label');
const resumeButton = $('#resume');
const dndChip = $('#dnd');
const modeItems = {
  playing: $('#mode-playing'),
  watching: $('#mode-watching'),
  listening: $('#mode-listening'),
  competing: $('#mode-competing'),
};

const EFFORTS = ['low', 'medium', 'high', 'extra high', 'max', 'ultracode'];
const LIMIT_KEYS = { '5h': 'tray.5h', All: 'tray.weekly', Fable: 'tray.fable' };
const SVG_NS = 'http://www.w3.org/2000/svg';

let busy = false;
let fittedHeight = 0;
let trayState = null;
let lastStatus = null;
let updateNotice = '';
let updateNoticeTimer = null;

// Shrink the window to the menu (plus the 12px body padding on each side).
function fitWindow() {
  const height = Math.ceil(menu.getBoundingClientRect().height) + 24;
  if (height === fittedHeight) return;
  fittedHeight = height;
  invoke('tray_fit', { height }).catch(() => {
    fittedHeight = 0;
  });
}

function applyTheme() {
  const saved = localStorage.getItem('claude-rpc-theme') || 'dark';
  const resolved =
    saved === 'system'
      ? window.matchMedia('(prefers-color-scheme: light)').matches
        ? 'light'
        : 'dark'
      : saved;
  document.body.dataset.theme = ['dark', 'light'].includes(resolved) ? resolved : 'dark';
}

function render(state) {
  if (!state) return;
  trayState = state;
  window.RpcShared.setLanguage(state.language);
  versionEl.textContent = `v${state.appVersion}`;
  startupItem.classList.toggle('on', state.startOnWindows);
  Object.entries(modeItems).forEach(([mode, item]) => {
    item.classList.toggle('checked', state.rpcMode === mode);
  });

  // Pause row: "Always" is permanent DND; the durations set a timed pause.
  const pausedUntil = state.pauseUntilMs > Date.now() ? state.pauseUntilMs : 0;
  dndChip.classList.toggle('on', state.dnd);
  resumeButton.hidden = !state.dnd && !pausedUntil;
  pauseLabel.textContent = state.dnd
    ? t('tray.dndOn')
    : pausedUntil
      ? t('tray.pausedUntil', { time: formatTime(new Date(pausedUntil)) })
      : t('tray.pause');

  updateItem.title = state.updateError || '';
  updateBadge.hidden = !state.updateVersion || !!state.updateError;
  updateItem.classList.toggle('update-ready', !!state.updateVersion && !state.updateError);
  updateLabel.textContent =
    updateNotice ||
    (state.updateError
      ? t('tray.updateFailed')
      : state.updateVersion
        ? t('tray.install', { version: state.updateVersion })
        : t('tray.updates'));
  if (lastStatus) renderStatus(lastStatus);
}

function renderStatus(status) {
  lastStatus = status;
  const user = inner(status.discordLine, 'Discord:');
  const connected = (status.discordLine || '').startsWith('Discord: Connected');
  discordDot.classList.toggle('ok', connected);
  discordText.replaceChildren(connected ? t('tray.discordConnected') : t('tray.discordOff'));
  if (connected && user) {
    const name = document.createElement('b');
    name.textContent = user;
    discordText.append(' · ', name);
  }

  const running = status.claudeLine && status.claudeLine !== 'Claude: Off';
  const parts = running ? (status.modelLine || '').split(' | ') : [];
  const effort = parts.length > 1 && EFFORTS.includes(parts[parts.length - 1].toLowerCase())
    ? parts[parts.length - 1]
    : '';
  modelName.textContent = !running
    ? t('tray.notRunning')
    : parts[0] && parts[0] !== 'Auto-detect'
      ? parts[0]
      : 'Claude';
  modelChip.textContent = effort;
  modelChip.hidden = !effort;

  const client = (status.claudeLine || '').replace(/^Claude:\s*/, '');
  const app = client.startsWith('Desktop')
    ? ['Claude Desktop', inner(status.claudeLine, 'Claude: Desktop')].filter(Boolean).join(' · ')
    : client.startsWith('CLI')
      ? 'Claude Code'
      : '';
  const provider = (status.providerLine || '').replace(/^Provider:\s*/, '');
  const plan = inner(status.providerLine, 'Provider:') || provider.split(' (')[0] || '';
  const sessions = running && status.sessions > 1 ? t('tray.sessions', { count: status.sessions }) : '';
  contextEl.textContent = [app, plan, sessions]
    .filter((part) => part && part !== 'Unknown')
    .join(' · ');

  const limits = Array.isArray(status.limits) ? status.limits : [];
  const fiveHour = limits.find((limit) => limit.label === '5h');
  const fivePercent = fiveHour ? Number(fiveHour.usedPercent) || 0 : 0;
  alertEl.hidden = fivePercent < 80;
  alertText.textContent = t('tray.alert', { percent: fivePercent >= 95 ? 95 : 80 });

  limitsEl.replaceChildren(
    ...limits.map((limit) => {
      const percent = Math.max(0, Math.min(100, Number(limit.usedPercent) || 0));
      const row = document.createElement('div');
      const head = document.createElement('div');
      head.className = 'limit-head';
      const label = document.createElement('span');
      label.textContent = LIMIT_KEYS[limit.label] ? t(LIMIT_KEYS[limit.label]) : limit.label;
      const reset = formatReset(limit.reset);
      if (reset) {
        const small = document.createElement('small');
        small.textContent = reset;
        label.append(small);
      }
      const value = document.createElement('span');
      value.textContent = `${percent}%`;
      head.append(label, value);
      const bar = document.createElement('div');
      bar.className = 'bar';
      const fill = document.createElement('i');
      fill.style.width = `${percent}%`;
      fill.classList.toggle('warn', percent >= 80);
      bar.append(fill);
      row.append(head, bar);
      if (limit.label === '5h') {
        const chart = sparkline(status.history5h);
        if (chart) row.append(chart);
      }
      return row;
    }),
  );
}

// Last 24 h of the 5-hour bucket, sampled every 5 minutes by the daemon.
function sparkline(history) {
  const points = (Array.isArray(history) ? history : []).filter(
    (point) => Array.isArray(point) && Date.now() - point[0] <= 24 * 60 * 60 * 1000,
  );
  if (points.length < 2) return null;
  const width = 260;
  const height = 30;
  const start = points[0][0];
  const span = Math.max(1, points[points.length - 1][0] - start);
  const coords = points.map(([time, percent]) => [
    ((time - start) / span) * width,
    height - 2 - (Math.min(100, percent) / 100) * (height - 4),
  ]);
  const line = coords.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)} ${y.toFixed(1)}`).join(' ');
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('viewBox', `0 0 ${width} ${height}`);
  svg.setAttribute('preserveAspectRatio', 'none');
  svg.setAttribute('class', 'spark');
  svg.setAttribute('role', 'img');
  svg.setAttribute('aria-label', t('tray.today'));
  const area = document.createElementNS(SVG_NS, 'path');
  area.setAttribute('d', `${line} L${width} ${height} L0 ${height} Z`);
  area.setAttribute('class', 'spark-area');
  const stroke = document.createElementNS(SVG_NS, 'path');
  stroke.setAttribute('d', line);
  stroke.setAttribute('class', 'spark-line');
  svg.append(area, stroke);
  return svg;
}

async function refreshStatus() {
  try {
    renderStatus(await invoke('load_status'));
  } catch {
    /* status unavailable; keep last render */
  }
  fitWindow();
}

async function refresh() {
  applyTheme();
  try {
    render(await invoke('tray_state'));
  } catch {
    /* daemon state unavailable; keep last render */
  }
  await refreshStatus();
  menu.classList.remove('pop');
  void menu.offsetWidth;
  menu.classList.add('pop');
}

// Briefly replace the Updates label with a result ("Up to date", an error).
function flashUpdateLabel(text) {
  clearTimeout(updateNoticeTimer);
  updateNotice = text;
  updateLabel.textContent = text;
  updateNoticeTimer = setTimeout(() => {
    updateNotice = '';
    render(trayState);
  }, 2500);
}

// End of a timed pause, computed in local time.
function pauseEnd(value) {
  if (value === 'tomorrow') {
    const midnight = new Date();
    midnight.setHours(24, 0, 0, 0);
    return midnight.getTime();
  }
  return Date.now() + Number(value) * 60 * 1000;
}

async function act(action) {
  if (busy) return;
  busy = true;
  const checking = action === 'update' && !updateItem.classList.contains('update-ready');
  if (checking) {
    updateNotice = t('tray.checking');
    updateLabel.textContent = updateNotice;
  }
  try {
    const state = await invoke('tray_action', { action });
    if (checking) updateNotice = '';
    render(state);
    if (checking && state && !state.updateVersion && !state.updateError) {
      flashUpdateLabel(t('tray.upToDate'));
    }
  } catch (error) {
    if (checking) {
      updateItem.title = String(error);
      flashUpdateLabel(t('tray.checkFailed'));
    }
  } finally {
    busy = false;
  }
}

document.querySelectorAll('[data-action]').forEach((item) => {
  item.addEventListener('click', () => act(item.dataset.action));
});
document.querySelectorAll('[data-pause]').forEach((chip) => {
  chip.addEventListener('click', () => act(`pause:${pauseEnd(chip.dataset.pause)}`));
});

document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') act('close');
});

window.addEventListener('focus', refresh);
// Keep the card live while the menu is open.
setInterval(() => {
  if (document.hasFocus() && !document.hidden) refreshStatus();
}, 2000);
refresh();
