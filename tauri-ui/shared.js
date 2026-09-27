// Shared by the settings window (main.js) and the tray menu (tray.js):
// English/French strings, status-line parsing, time formatting and the
// 5-hour usage chart.
(() => {
  const STRINGS = {
    en: {
      'client.desktop': 'Desktop',
      'client.code': 'Claude Code',
      'client.off': 'Off',
      'client.notRunning': 'Not running',
      'theme.dark': 'Dark',
      'theme.system': 'System',
      'theme.light': 'Light',
      'nav.presence': 'Presence',
      'nav.usage': 'Usage',
      'nav.privacy': 'Privacy',
      'nav.buttons': 'Profile buttons',
      'nav.general': 'General',
      'nav.about': 'About',
      'side.discordOff': 'Discord not connected',
      'side.version': 'Claude RPC v{version}',
      'presence.title': 'Presence',
      'presence.sub': 'What your Discord profile shows while you use Claude.',
      'preview.live': 'Live preview',
      'preview.elapsed': '{time} elapsed',
      'preview.notShown': 'Not shown',
      'preview.nothing': 'Nothing is shown on your profile right now.',
      'preview.dnd': 'Do Not Disturb is on',
      'preview.paused': 'Paused until {time}',
      'preview.private': 'Private project',
      'preview.chat': 'Chat tab is hidden',
      'preview.notRunning': 'Claude is not running',
      'activity.type': 'Activity type',
      'mode.playing': 'Playing',
      'mode.watching': 'Watching',
      'mode.listening': 'Listening',
      'mode.competing': 'Competing',
      'details.title': 'Details',
      'details.show': 'Show on profile',
      'details.showHint': 'Added after the model. Plan needs Provider.',
      'labels.provider': 'Provider',
      'labels.plan': 'Plan',
      'labels.effort': 'Effort',
      'labels.sessions': 'Sessions',
      'labels.modelIcon': 'Model icon',
      'limits.label': 'Usage limits',
      'limits.hint': 'What Discord shows. The tray menu always shows all.',
      'limits.5h': '5-hour',
      'limits.weekly': 'Weekly',
      'limits.fable': 'Fable',
      'text.title': 'Custom text',
      'text.details': 'Main line',
      'text.state': 'Second line',
      'text.hint': 'Leave empty for the default text. Click a variable to insert it.',
      'usage.title': 'Usage',
      'usage.sub': 'Your Claude limits, read from your account.',
      'usage.refresh': 'Refresh',
      'usage.refreshing': 'Refreshing…',
      'usage.none': 'No usage yet. It appears once Claude reports it.',
      'usage.used': 'used',
      'credits.title': 'Usage credits',
      'credits.spent': 'Spent',
      'credits.of': 'of {limit} this month',
      'credits.month': 'spent this month',
      'credits.off': 'Off',
      'credits.hint': 'Spent automatically past your plan limits, up to this cap.',
      'credits.hintNoCap': 'Spent automatically past your plan limits.',
      'credits.billed': 'Billed in {currency}.',
      'credits.offHint': 'Turn them on in your Claude account to keep working past your plan limits.',
      'tray.credits': 'Usage credits',
      'alerts.title': 'Alerts',
      'alerts.hint': 'Windows notifications for the 5-hour session.',
      'alerts.80': 'At 80%',
      'alerts.95': 'At 95%',
      'alerts.reset': 'When it resets',
      'privacy.title': 'Privacy',
      'privacy.sub': 'Choose when nothing is shown on Discord.',
      'dnd.label': 'Do Not Disturb',
      'dnd.hint': 'Hide the activity. Detection keeps running.',
      'privacy.chat': 'Hide in Chat tab',
      'privacy.chatHint': 'Show nothing while the Claude Desktop Chat tab is open.',
      'privacy.projects': 'Private projects',
      'privacy.projectsHint': 'Nothing is shown while you work in these folders. One per line: a folder name or a full path.',
      'buttons.title': 'Profile buttons',
      'buttons.sub': 'Up to two links on your Discord activity.',
      'buttons.watchingOnly': 'Discord shows buttons only in Watching mode.',
      'buttons.switch': 'Switch to Watching',
      'buttons.nitroTitle': 'Discord Nitro only.',
      'buttons.nitro': 'Without Nitro on your account, other people will not see these two buttons.',
      'about.title': 'About',
      'about.description': 'Shows your Claude activity on your Discord profile.',
      'about.links': 'Links',
      'about.repo': 'Source code on GitHub',
      'about.profile': 'Author’s GitHub profile',
      'about.releases': 'Release notes',
      'about.releasesHint': 'What changed in each version',
      'about.issues': 'Report a problem',
      'about.issuesHint': 'Opens GitHub issues; paste the copied diagnostic',
      'about.legal': 'MIT License. Not affiliated with Anthropic or Discord.',
      'buttons.label': 'Label',
      'buttons.url': 'URL',
      'buttons.one': 'Button 1',
      'buttons.two': 'Button 2',
      'buttons.quick': 'Quick fill',
      'buttons.clear': 'Clear buttons',
      'buttons.badUrl': 'The URL must start with https://',
      'buttons.incomplete': 'Add both a label and a URL.',
      'general.title': 'General',
      'general.appearance': 'Appearance',
      'app.theme': 'Theme',
      'app.language': 'Language',
      'app.languageAuto': 'Automatic',
      'general.startup': 'Start with Windows',
      'general.startupHint': 'Claude RPC starts in the tray when you sign in.',
      'general.updates': 'Updates',
      'general.version': 'Version {version}',
      'general.check': 'Check now',
      'general.checking': 'Checking…',
      'general.upToDate': 'You are up to date.',
      'general.checkFailed': 'Check failed: {error}',
      'app.diagnostic': 'Troubleshooting',
      'app.diagnosticHint': 'Copies what the app detects, to paste in a bug report.',
      'app.copyDiagnostic': 'Copy diagnostic',
      'app.copied': 'Diagnostic copied',
      'app.copyFailed': 'Copy failed; the text is selected below',
      'footer.auto': 'Changes save automatically',
      'footer.saved': 'Saved · {time}',
      'footer.saving': 'Saving…',
      'footer.done': 'Done',
      'update.available': 'Update available: v{version}',
      'update.now': 'Update now',
      'update.downloading': 'Downloading update…',
      'update.failed': 'Update failed: {error}',
      'whatsnew.title': 'Updated to v{version}',
      'whatsnew.dismiss': 'Got it',
      'tray.notRunning': 'Waiting for Claude',

      'tray.noLimits': 'Usage appears once Claude reports it.',
      'tray.last24h': 'Last 24 h',
      'tray.peak': 'Peak {percent}%',
      'tray.alert': 'Above {percent}% of your 5-hour session',
      'tray.5h': '5-hour session',
      'tray.weekly': 'Weekly',
      'tray.fable': 'Fable weekly',
      'tray.pause': 'Pause Discord presence',
      'tray.pausedUntil': 'Until {time}',
      'tray.pausedResume': 'Until you resume',
      'tray.30min': '30 min',
      'tray.1h': '1 h',
      'tray.tomorrow': 'Tomorrow',
      'tray.activity': 'Activity',
      'tray.openDesktop': 'Open Claude Desktop',
      'tray.getDesktop': 'Download Claude Desktop',
      'tray.launchCode': 'Launch Claude Code',
      'tray.installCode': 'Install Claude Code',
      'tray.settings': 'Settings…',
      'tray.updates': 'Check for updates',
      'tray.checking': 'Checking…',
      'tray.upToDate': 'Up to date',
      'tray.checkFailed': 'Check failed',
      'tray.updateFailed': 'Update failed, retry',
      'tray.install': 'Install v{version}',
      'tray.quit': 'Quit',
      'tray.showingOn': 'Showing on {user}',
      'tray.hiddenOn': 'Hidden on {user}',
      'tray.discordOff': 'Discord not connected',
      'reason.dnd': 'Do Not Disturb',
      'reason.paused': 'paused',
      'reason.private': 'private project',
      'reason.chat': 'Chat tab',
      'reason.notRunning': 'Claude not running',
      'reset.in': 'resets in {duration}',
      'reset.text': 'resets {text}',
    },
    fr: {
      'client.desktop': 'Desktop',
      'client.code': 'Claude Code',
      'client.off': 'Inactif',
      'client.notRunning': 'Pas lancé',
      'theme.dark': 'Sombre',
      'theme.system': 'Système',
      'theme.light': 'Clair',
      'nav.presence': 'Présence',
      'nav.usage': 'Usage',
      'nav.privacy': 'Confidentialité',
      'nav.buttons': 'Boutons du profil',
      'nav.general': 'Général',
      'nav.about': 'À propos',
      'side.discordOff': 'Discord non connecté',
      'side.version': 'Claude RPC v{version}',
      'presence.title': 'Présence',
      'presence.sub': 'Ce que ton profil Discord affiche quand tu utilises Claude.',
      'preview.live': 'Aperçu en direct',
      'preview.elapsed': '{time} écoulées',
      'preview.notShown': 'Non affiché',
      'preview.nothing': 'Rien n’est affiché sur ton profil pour l’instant.',
      'preview.dnd': 'Ne pas déranger est activé',
      'preview.paused': 'En pause jusqu’à {time}',
      'preview.private': 'Projet privé',
      'preview.chat': 'Onglet Chat masqué',
      'preview.notRunning': 'Claude n’est pas lancé',
      'activity.type': 'Type d’activité',
      'mode.playing': 'Joue',
      'mode.watching': 'Regarde',
      'mode.listening': 'Écoute',
      'mode.competing': 'Compétition',
      'details.title': 'Détails',
      'details.show': 'Afficher sur le profil',
      'details.showHint': 'Ajouté après le modèle. Forfait demande Fournisseur.',
      'labels.provider': 'Fournisseur',
      'labels.plan': 'Forfait',
      'labels.effort': 'Effort',
      'labels.sessions': 'Sessions',
      'labels.modelIcon': 'Icône du modèle',
      'limits.label': 'Limites d’usage',
      'limits.hint': 'Ce que Discord affiche. Le menu les montre toujours toutes.',
      'limits.5h': '5 heures',
      'limits.weekly': 'Semaine',
      'limits.fable': 'Fable',
      'text.title': 'Texte personnalisé',
      'text.details': 'Ligne principale',
      'text.state': 'Deuxième ligne',
      'text.hint': 'Laisse vide pour le texte par défaut. Clique sur une variable pour l’insérer.',
      'usage.title': 'Usage',
      'usage.sub': 'Tes limites Claude, lues depuis ton compte.',
      'usage.refresh': 'Actualiser',
      'usage.refreshing': 'Actualisation…',
      'usage.none': 'Pas encore d’usage. Il s’affiche dès que Claude le transmet.',
      'usage.used': 'utilisés',
      'credits.title': 'Crédits d’usage',
      'credits.spent': 'Dépensé',
      'credits.of': 'sur {limit} ce mois-ci',
      'credits.month': 'dépensés ce mois-ci',
      'credits.off': 'Désactivés',
      'credits.hint': 'Utilisés automatiquement au-delà des limites du forfait, jusqu’à ce plafond.',
      'credits.hintNoCap': 'Utilisés automatiquement au-delà des limites du forfait.',
      'credits.billed': 'Facturés en {currency}.',
      'credits.offHint': 'Active-les dans ton compte Claude pour continuer au-delà des limites de ton forfait.',
      'tray.credits': 'Crédits d’usage',
      'alerts.title': 'Alertes',
      'alerts.hint': 'Notifications Windows pour la session de 5 heures.',
      'alerts.80': 'À 80 %',
      'alerts.95': 'À 95 %',
      'alerts.reset': 'À la remise à zéro',
      'privacy.title': 'Confidentialité',
      'privacy.sub': 'Choisis quand rien n’est affiché sur Discord.',
      'dnd.label': 'Ne pas déranger',
      'dnd.hint': 'Masque l’activité. La détection continue.',
      'privacy.chat': 'Masquer l’onglet Chat',
      'privacy.chatHint': 'N’affiche rien quand l’onglet Chat de Claude Desktop est ouvert.',
      'privacy.projects': 'Projets privés',
      'privacy.projectsHint': 'Rien n’est affiché quand tu travailles dans ces dossiers. Un par ligne : un nom de dossier ou un chemin complet.',
      'buttons.title': 'Boutons du profil',
      'buttons.sub': 'Jusqu’à deux liens sur ton activité Discord.',
      'buttons.watchingOnly': 'Discord n’affiche les boutons qu’en mode Regarde.',
      'buttons.switch': 'Passer en Regarde',
      'buttons.nitroTitle': 'Réservé à Discord Nitro.',
      'buttons.nitro': 'Sans Nitro sur ton compte, les autres ne verront pas ces deux boutons.',
      'about.title': 'À propos',
      'about.description': 'Affiche ton activité Claude sur ton profil Discord.',
      'about.links': 'Liens',
      'about.repo': 'Code source sur GitHub',
      'about.profile': 'Profil GitHub de l’auteur',
      'about.releases': 'Notes de version',
      'about.releasesHint': 'Ce qui change à chaque version',
      'about.issues': 'Signaler un problème',
      'about.issuesHint': 'Ouvre les issues GitHub ; colle le diagnostic copié',
      'about.legal': 'Licence MIT. Non affilié à Anthropic ni à Discord.',
      'buttons.label': 'Libellé',
      'buttons.url': 'Adresse',
      'buttons.one': 'Bouton 1',
      'buttons.two': 'Bouton 2',
      'buttons.quick': 'Remplir',
      'buttons.clear': 'Vider les boutons',
      'buttons.badUrl': 'L’adresse doit commencer par https://',
      'buttons.incomplete': 'Ajoute un libellé et une adresse.',
      'general.title': 'Général',
      'general.appearance': 'Apparence',
      'app.theme': 'Thème',
      'app.language': 'Langue',
      'app.languageAuto': 'Automatique',
      'general.startup': 'Lancer avec Windows',
      'general.startupHint': 'Claude RPC démarre dans la zone de notification à l’ouverture de session.',
      'general.updates': 'Mises à jour',
      'general.version': 'Version {version}',
      'general.check': 'Vérifier',
      'general.checking': 'Vérification…',
      'general.upToDate': 'Tu es à jour.',
      'general.checkFailed': 'Échec de la vérification : {error}',
      'app.diagnostic': 'Dépannage',
      'app.diagnosticHint': 'Copie ce que l’app détecte, à coller dans un signalement de bug.',
      'app.copyDiagnostic': 'Copier le diagnostic',
      'app.copied': 'Diagnostic copié',
      'app.copyFailed': 'Copie impossible ; le texte est sélectionné ci-dessous',
      'footer.auto': 'Enregistrement automatique',
      'footer.saved': 'Enregistré · {time}',
      'footer.saving': 'Enregistrement…',
      'footer.done': 'Terminé',
      'update.available': 'Mise à jour disponible : v{version}',
      'update.now': 'Mettre à jour',
      'update.downloading': 'Téléchargement de la mise à jour…',
      'update.failed': 'Échec de la mise à jour : {error}',
      'whatsnew.title': 'Mis à jour en v{version}',
      'whatsnew.dismiss': 'Compris',
      'tray.notRunning': 'En attente de Claude',

      'tray.noLimits': 'L’usage s’affiche dès que Claude le transmet.',
      'tray.last24h': '24 dernières heures',
      'tray.peak': 'Pic {percent} %',
      'tray.alert': 'Plus de {percent} % de ta session de 5 h',
      'tray.5h': 'Session de 5 h',
      'tray.weekly': 'Semaine',
      'tray.fable': 'Fable (semaine)',
      'tray.pause': 'Pause sur Discord',
      'tray.pausedUntil': 'Jusqu’à {time}',
      'tray.pausedResume': 'Jusqu’à la reprise',
      'tray.30min': '30 min',
      'tray.1h': '1 h',
      'tray.tomorrow': 'Demain',
      'tray.activity': 'Activité',
      'tray.openDesktop': 'Ouvrir Claude Desktop',
      'tray.getDesktop': 'Télécharger Claude Desktop',
      'tray.launchCode': 'Lancer Claude Code',
      'tray.installCode': 'Installer Claude Code',
      'tray.settings': 'Réglages…',
      'tray.updates': 'Rechercher une mise à jour',
      'tray.checking': 'Vérification…',
      'tray.upToDate': 'À jour',
      'tray.checkFailed': 'Échec de la vérification',
      'tray.updateFailed': 'Échec, réessayer',
      'tray.install': 'Installer v{version}',
      'tray.quit': 'Quitter',
      'tray.showingOn': 'Affiché sur {user}',
      'tray.hiddenOn': 'Masqué sur {user}',
      'tray.discordOff': 'Discord non connecté',
      'reason.dnd': 'ne pas déranger',
      'reason.paused': 'en pause',
      'reason.private': 'projet privé',
      'reason.chat': 'onglet Chat',
      'reason.notRunning': 'Claude pas lancé',
      'reset.in': 'remise à zéro dans {duration}',
      'reset.text': 'remise à zéro {text}',
    },
  };

  const SVG_NS = 'http://www.w3.org/2000/svg';
  let language = 'en';

  function resolveLanguage(setting) {
    if (setting === 'en' || setting === 'fr') return setting;
    return (navigator.language || '').toLowerCase().startsWith('fr') ? 'fr' : 'en';
  }

  function t(key, vars = {}) {
    const text = STRINGS[language][key] ?? STRINGS.en[key] ?? key;
    return text.replace(/\{(\w+)\}/g, (match, name) => (name in vars ? String(vars[name]) : match));
  }

  // t() as DOM nodes, with the given variables in bold ("Showing on <b>user</b>").
  function tBold(key, vars) {
    const nodes = [];
    const text = STRINGS[language][key] ?? STRINGS.en[key] ?? key;
    let last = 0;
    text.replace(/\{(\w+)\}/g, (match, name, offset) => {
      if (offset > last) nodes.push(document.createTextNode(text.slice(last, offset)));
      const bold = document.createElement('b');
      bold.textContent = name in vars ? String(vars[name]) : match;
      nodes.push(bold);
      last = offset + match.length;
      return match;
    });
    if (last < text.length) nodes.push(document.createTextNode(text.slice(last)));
    return nodes;
  }

  // Fills every [data-i18n] element (and aria-label variants).
  function applyTranslations(root = document) {
    root.querySelectorAll('[data-i18n]').forEach((el) => {
      el.textContent = t(el.dataset.i18n);
    });
    root.querySelectorAll('[data-i18n-aria]').forEach((el) => {
      el.setAttribute('aria-label', t(el.dataset.i18nAria));
    });
    root.querySelectorAll('[data-i18n-title]').forEach((el) => {
      el.title = t(el.dataset.i18nTitle);
    });
    document.documentElement.lang = language;
  }

  function setLanguage(setting) {
    language = resolveLanguage(setting);
    applyTranslations();
  }

  const locale = () => (language === 'fr' ? 'fr-FR' : 'en-GB');

  // Status lines come from status.txt, e.g. "Discord: Connected (inerthel)",
  // "Claude: Desktop (Code)", "Provider: Subscription (Claude Max (5x))".
  function inner(line, prefix) {
    if (!line || !line.startsWith(prefix)) return '';
    const rest = line.slice(prefix.length).trim();
    const open = rest.indexOf('(');
    return open >= 0 && rest.endsWith(')') ? rest.slice(open + 1, -1) : '';
  }

  // What the status says about the Claude client, for both windows.
  function describeClient(status) {
    const line = (status && status.claudeLine) || '';
    const client = line.replace(/^Claude:\s*/, '');
    if (!client || client === 'Off') return { running: false, label: t('client.off') };
    if (client.startsWith('Desktop')) {
      const mode = inner(line, 'Claude: Desktop');
      const label = [t('client.desktop'), mode].filter(Boolean).join(' · ');
      return { running: true, label: status.cliAlongside ? `${label} + CLI` : label };
    }
    return { running: true, label: t('client.code') };
  }

  // "Claude Opus 5.5 | High" -> { model: 'Opus 5.5', effort: 'High' }.
  const EFFORTS = ['low', 'medium', 'high', 'extra high', 'xhigh', 'max', 'ultracode'];
  function describeModel(status) {
    const parts = ((status && status.modelLine) || '').split(' | ');
    const last = parts[parts.length - 1] || '';
    const effort = parts.length > 1 && EFFORTS.includes(last.toLowerCase()) ? last : '';
    const model = parts[0] && parts[0] !== 'Auto-detect' ? parts[0].replace(/^Claude\s+/, '') : '';
    return { model, effort };
  }

  function planOf(status) {
    const line = (status && status.providerLine) || '';
    const plan = inner(line, 'Provider:') || line.replace(/^Provider:\s*/, '').split(' (')[0];
    return plan && plan !== 'Unknown' ? plan : '';
  }

  function discordUser(status) {
    const line = (status && status.discordLine) || '';
    return line.startsWith('Discord: Connected') ? inner(line, 'Discord:') || 'Discord' : '';
  }

  function formatTime(date) {
    return date.toLocaleTimeString(locale(), { hour: '2-digit', minute: '2-digit' });
  }

  function formatDuration(minutes) {
    const days = Math.floor(minutes / 1440);
    const hours = Math.floor((minutes % 1440) / 60);
    const mins = minutes % 60;
    if (language === 'fr') {
      if (days) return `${days} j ${hours} h`;
      return hours ? `${hours} h ${String(mins).padStart(2, '0')}` : `${mins} min`;
    }
    if (days) return `${days}d ${hours}h`;
    return hours ? `${hours}h ${mins}m` : `${mins}m`;
  }

  // OAuth resets are ISO dates; Desktop ones are free text ("in 4 hr 31 min").
  // Returns { relative: 'resets in 3h 12m', absolute: 'Sun 04:00' } or null.
  function resetParts(reset) {
    if (!reset) return null;
    let date = null;
    // "in 4 hr 31 min", "in 2 hours 5 min", "in 12 min".
    const relative = /^in\s+(?:(\d+)\s*h(?:ou)?r?s?)?\s*(?:(\d+)\s*min)?/i.exec(String(reset).trim());
    if (relative && (relative[1] || relative[2])) {
      // Free text has no fixed end: a clock time derived from "now" would creep
      // forward on every render, so show the relative part only.
      const minutes = Number(relative[1] || 0) * 60 + Number(relative[2] || 0);
      return { relative: t('reset.in', { duration: formatDuration(minutes) }), absolute: '' };
    } else if (/^\d{4}-\d{2}-\d{2}T/.test(reset)) {
      date = new Date(reset);
    }
    if (!date || Number.isNaN(date.getTime())) {
      return { relative: t('reset.text', { text: reset }), absolute: '' };
    }
    const minutes = Math.max(0, Math.round((date.getTime() - Date.now()) / 60000));
    const sameDay = date.toDateString() === new Date().toDateString();
    const absolute = sameDay
      ? formatTime(date)
      : `${date.toLocaleDateString(locale(), { weekday: 'short' })} ${formatTime(date)}`;
    return { relative: t('reset.in', { duration: formatDuration(minutes) }), absolute };
  }

  function formatReset(reset) {
    const parts = resetParts(reset);
    return parts ? parts.relative : '';
  }

  // Elapsed time since the session started: "10h 57m" (short) or "10:57:04".
  function formatElapsed(startMs, clock = false) {
    const seconds = Math.max(0, Math.floor((Date.now() - startMs) / 1000));
    if (!clock) return formatDuration(Math.floor(seconds / 60));
    const pad = (value) => String(value).padStart(2, '0');
    const hours = Math.floor(seconds / 3600);
    return `${hours ? `${pad(hours)}:` : ''}${pad(Math.floor((seconds % 3600) / 60))}:${pad(seconds % 60)}`;
  }

  // Amounts come in minor units of the account's billing currency (cents for
  // USD/EUR). The currency never changes with location; only the number format
  // follows the Windows region when it matches the UI language (fr-CA, en-GB…).
  function formatMoney(minor, currency) {
    const nav = navigator.language || '';
    const loc = nav.toLowerCase().startsWith(language) ? nav : language === 'fr' ? 'fr-FR' : 'en-US';
    if (currency) {
      try {
        const format = new Intl.NumberFormat(loc, { style: 'currency', currency });
        const digits = format.resolvedOptions().maximumFractionDigits;
        return format.format(minor / 10 ** digits);
      } catch {
        /* unknown code: plain amount below */
      }
    }
    // Unknown currency: a plain amount with cents, never a guessed symbol.
    const plain = new Intl.NumberFormat(loc, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
    return [plain.format(minor / 100), currency].filter(Boolean).join(' ');
  }

  // status.credits -> { used, limit, percent, currency } or null when unknown.
  function describeCredits(credits) {
    if (!credits || typeof credits !== 'object') return null;
    const known = (value) => typeof value === 'number' && Number.isFinite(value) && value >= 0;
    const used = known(credits.used) ? formatMoney(credits.used, credits.currency) : '';
    const limit = known(credits.limit) ? formatMoney(credits.limit, credits.currency) : '';
    const percent =
      known(credits.used) && known(credits.limit) && credits.limit > 0
        ? Math.min(100, Math.round((credits.used / credits.limit) * 100))
        : null;
    return { enabled: credits.enabled !== false, used, limit, percent, currency: credits.currency || '' };
  }

  const capitalize = (text) => (text ? text.charAt(0).toUpperCase() + text.slice(1) : text);

  // Last 24 h of the 5-hour bucket, sampled every 5 minutes by the daemon.
  function recentHistory(history) {
    return (Array.isArray(history) ? history : []).filter(
      (point) => Array.isArray(point) && Date.now() - point[0] <= 24 * 60 * 60 * 1000,
    );
  }

  function sparkline(history, height = 34) {
    const points = recentHistory(history);
    if (points.length < 2) return null;
    const width = 260;
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
    svg.setAttribute('aria-label', t('tray.last24h'));
    const area = document.createElementNS(SVG_NS, 'path');
    area.setAttribute('d', `${line} L${width} ${height} L0 ${height} Z`);
    area.setAttribute('class', 'spark-area');
    const stroke = document.createElementNS(SVG_NS, 'path');
    stroke.setAttribute('d', line);
    stroke.setAttribute('class', 'spark-line');
    svg.append(area, stroke);
    return svg;
  }

  function peakOf(history) {
    return recentHistory(history).reduce((max, point) => Math.max(max, Number(point[1]) || 0), 0);
  }

  // No browser context menu (Back, Refresh, Save as, Print…) in the app. Text
  // fields keep it for cut, copy and paste.
  document.addEventListener('contextmenu', (event) => {
    const target = event.target;
    const editable =
      target instanceof Element &&
      (target.closest('input, textarea') || target.isContentEditable);
    if (!editable) event.preventDefault();
  });

  window.RpcShared = {
    t,
    tBold,
    setLanguage,
    applyTranslations,
    inner,
    describeClient,
    describeModel,
    planOf,
    discordUser,
    formatReset,
    resetParts,
    formatTime,
    formatElapsed,
    capitalize,
    describeCredits,
    sparkline,
    peakOf,
    language: () => language,
  };
})();
