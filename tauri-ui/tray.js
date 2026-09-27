const invoke = window.__TAURI__.core.invoke;
const {
  t,
  tBold,
  describeClient,
  describeModel,
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

const $ = (selector) => document.querySelector(selector);
const menu = $('#menu');
const modelLine = $('#model-line');
const clientPill = $('#client-pill');
const clientLabel = $('#client-label');
const hero = $('#hero');
const heroValue = $('#hero-value');
const heroName = $('#hero-name');
const heroFill = $('#hero-fill');
const heroReset = $('#hero-reset');
const heroRelative = $('#hero-relative');
const heroAbsolute = $('#hero-absolute');
const planPill = $('#plan-pill');
const alertEl = $('#alert');
const alertText = $('#alert-text');
const chart = $('#chart');
const chartPeak = $('#chart-peak');
const chartSlot = $('#chart-slot');
const usageEmpty = $('#usage-empty');
const limitsEl = $('#limits');
const creditsEl = $('#credits');
const creditsValue = $('#credits-value');
const creditsBar = $('#credits-bar');
const creditsFill = $('#credits-fill');
const pauseSwitch = $('#pause-switch');
const pauseState = $('#pause-state');
const modeLabel = $('#mode-label');
const modeButtons = [...document.querySelectorAll('.mode[data-mode]')];
const desktopLabel = $('#desktop-label');
const desktopExternal = $('#desktop-external');
const codeLabel = $('#code-label');
const codeExternal = $('#code-external');
const updateItem = $('#update');
const updateLabel = $('#update-label');
const updateBadge = $('#update-badge');
const foot = $('#foot');
const footText = $('#foot-text');

const LIMIT_KEYS = { '5h': 'tray.5h', All: 'tray.weekly', Fable: 'tray.fable' };

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

const pausedUntil = (state) => (state && state.pauseUntilMs > Date.now() ? state.pauseUntilMs : 0);

function render(state) {
  if (!state) return;
  trayState = state;
  window.RpcShared.setLanguage(state.language);

  // The switch pauses until resumed (Do Not Disturb); the chips set a timed pause.
  const until = pausedUntil(state);
  const paused = state.dnd || !!until;
  pauseSwitch.classList.toggle('on', paused);
  pauseSwitch.setAttribute('aria-checked', String(paused));
  pauseState.hidden = !paused;
  pauseState.classList.toggle('on', paused);
  pauseState.textContent = state.dnd
    ? t('tray.pausedResume')
    : until
      ? t('tray.pausedUntil', { time: untilLabel(new Date(until)) })
      : '';

  modeButtons.forEach((button) => {
    const checked = button.dataset.mode === state.rpcMode;
    button.classList.toggle('checked', checked);
    button.setAttribute('aria-checked', String(checked));
  });
  modeLabel.textContent = t(`mode.${state.rpcMode}`);

  // Installed: open the app. Missing: its download / install page (arrow icon).
  desktopLabel.textContent = t(state.desktopInstalled ? 'tray.openDesktop' : 'tray.getDesktop');
  // SVG elements have no `hidden` property: toggle the attribute itself.
  desktopExternal.toggleAttribute('hidden', !!state.desktopInstalled);
  codeLabel.textContent = t(state.codeInstalled ? 'tray.launchCode' : 'tray.installCode');
  codeExternal.toggleAttribute('hidden', !!state.codeInstalled);

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

// "14:30" today, "Mon 00:00" on another day (a "Tomorrow" pause).
function untilLabel(date) {
  if (date.toDateString() === new Date().toDateString()) return formatTime(date);
  const lang = document.documentElement.lang === 'fr' ? 'fr-FR' : 'en-GB';
  return `${date.toLocaleDateString(lang, { weekday: 'short' })} ${formatTime(date)}`;
}

const clampPercent = (value) => Math.max(0, Math.min(100, Number(value) || 0));

function levelClass(fill, percent) {
  fill.classList.toggle('warn', percent >= 80 && percent < 95);
  fill.classList.toggle('danger', percent >= 95);
}

function renderStatus(status) {
  lastStatus = status;
  const client = describeClient(status);
  const { model, effort } = describeModel(status);
  modelLine.textContent = client.running
    ? [model || 'Claude', effort].filter(Boolean).join(' · ')
    : t('tray.notRunning');
  clientPill.classList.toggle('on', client.running);
  clientLabel.textContent = client.label;

  const plan = planOf(status).replace(/^Claude\s+/, '').replace(/\s*\((.*)\)/, ' $1');
  planPill.textContent = plan;
  planPill.hidden = !plan;

  // The 5-hour session leads; the other buckets follow as compact rows.
  const limits = Array.isArray(status.limits) ? status.limits : [];
  const main = limits.find((limit) => limit.label === '5h') || limits[0];
  hero.hidden = !main;
  usageEmpty.hidden = !!main;
  if (main) {
    const percent = clampPercent(main.usedPercent);
    heroValue.textContent = `${percent}%`;
    heroName.textContent = LIMIT_KEYS[main.label] ? t(LIMIT_KEYS[main.label]) : main.label;
    heroFill.style.width = `${percent}%`;
    levelClass(heroFill, percent);
    const reset = resetParts(main.reset);
    heroRelative.textContent = reset ? capitalize(reset.relative) : '';
    heroAbsolute.textContent = reset ? reset.absolute : '';
    heroReset.hidden = !reset;

    const fiveHour = main.label === '5h';
    alertEl.hidden = !fiveHour || percent < 80;
    alertText.textContent = t('tray.alert', { percent: percent >= 95 ? 95 : 80 });

    const svg = fiveHour ? sparkline(status.history5h) : null;
    chart.hidden = !svg;
    chartSlot.replaceChildren(...(svg ? [svg] : []));
    chartPeak.textContent = svg ? t('tray.peak', { percent: peakOf(status.history5h) }) : '';
  }

  limitsEl.replaceChildren(
    ...limits
      .filter((limit) => limit !== main)
      .map((limit) => {
        const percent = clampPercent(limit.usedPercent);
        const row = document.createElement('div');
        const head = document.createElement('div');
        head.className = 'limit-head';
        const label = document.createElement('span');
        label.textContent = LIMIT_KEYS[limit.label] ? t(LIMIT_KEYS[limit.label]) : limit.label;
        const value = document.createElement('b');
        value.textContent = `${percent}%`;
        head.append(label, value);
        const bar = document.createElement('div');
        bar.className = 'bar';
        const fill = document.createElement('i');
        fill.style.width = `${percent}%`;
        levelClass(fill, percent);
        bar.append(fill);
        row.append(head, bar);
        const parts = resetParts(limit.reset);
        if (parts) {
          const reset = document.createElement('small');
          reset.className = 'limit-reset';
          reset.textContent = parts.absolute || parts.relative;
          row.append(reset);
        }
        return row;
      }),
  );

  renderCredits(status.credits);
  renderFooter(status, client);
}

// Extra usage spend, only while usage credits are turned on for the account.
function renderCredits(raw) {
  const credits = describeCredits(raw);
  creditsEl.hidden = !credits || !credits.enabled || !credits.used;
  if (creditsEl.hidden) return;
  creditsValue.textContent = credits.limit ? `${credits.used} / ${credits.limit}` : credits.used;
  creditsBar.hidden = credits.percent === null;
  creditsFill.style.width = `${credits.percent || 0}%`;
  levelClass(creditsFill, credits.percent || 0);
}

// Why nothing reaches Discord right now, or '' when the activity is shown.
function hiddenReason(status, client) {
  if (trayState && trayState.dnd) return t('reason.dnd');
  if (pausedUntil(trayState)) return t('reason.paused');
  if (status.hiddenReason === 'private') return t('reason.private');
  if (status.hiddenReason === 'chat') return t('reason.chat');
  if (!client.running) return t('reason.notRunning');
  return '';
}

function renderFooter(status, client) {
  const user = discordUser(status);
  foot.classList.toggle('on', !!user);
  if (!user) {
    footText.replaceChildren(t('tray.discordOff'));
    return;
  }
  const reason = hiddenReason(status, client);
  const nodes = tBold(reason ? 'tray.hiddenOn' : 'tray.showingOn', { user });
  const extra = reason || (status.startedAtMs ? formatElapsed(status.startedAtMs) : '');
  footText.replaceChildren(...nodes, ...(extra ? [` · ${extra}`] : []));
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
  // Closing never waits behind a slow action (an update check can take long).
  if (action === 'close') {
    invoke('tray_action', { action }).catch(() => {});
    return;
  }
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
    fitWindow();
  }
}

document.querySelectorAll('[data-action]').forEach((item) => {
  item.addEventListener('click', () => act(item.dataset.action));
});
document.querySelectorAll('[data-pause]').forEach((chip) => {
  chip.addEventListener('click', () => act(`pause:${pauseEnd(chip.dataset.pause)}`));
});
pauseSwitch.addEventListener('click', () => {
  const paused = trayState && (trayState.dnd || pausedUntil(trayState));
  act(paused ? 'resume' : 'dnd');
});
modeButtons.forEach((button) => {
  button.addEventListener('click', () => act(`mode_${button.dataset.mode}`));
});

document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') act('close');
});

window.addEventListener('focus', refresh);
// Keep the card live while the menu is open.
let polling = false;
setInterval(async () => {
  if (polling || !document.hasFocus() || document.hidden) return;
  polling = true;
  try {
    await refreshStatus();
  } finally {
    polling = false;
  }
}, 2000);
refresh();
