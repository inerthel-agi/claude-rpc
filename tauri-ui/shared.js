// Shared by the settings window (main.js) and the tray menu (tray.js):
// English/French strings, status-line parsing and reset-time formatting.
(() => {
  const STRINGS = {
    en: {
      'theme.dark': 'Dark',
      'theme.system': 'System',
      'theme.light': 'Light',
      'tile.claude': 'Claude',
      'tile.model': 'Model',
      'tile.plan': 'Plan',
      'tile.discord': 'Discord',
      'tile.notRunning': 'Not running',
      'tile.notConnected': 'Not connected',
      'tile.connected': 'Connected',
      'preview.hint': 'What others see on your profile',
      'preview.notShown': 'Not shown on Discord',
      'preview.nothing': 'Nothing is shown on your profile right now.',
      'preview.dnd': 'Do Not Disturb is on',
      'preview.paused': 'Paused until {time}',
      'preview.private': 'Private project',
      'preview.chat': 'Chat tab is hidden',
      'preview.notRunning': 'Claude is not running',
      'section.activity': 'Activity',
      'section.details': 'Details shown',
      'section.notifications': 'Notifications',
      'section.privacy': 'Privacy',
      'section.text': 'Discord text',
      'section.buttons': 'Buttons',
      'section.app': 'App',
      'buttons.watchingOnly': 'Watching only',
      'buttons.note': 'Shown on your Discord activity to viewers with Discord Nitro.',
      'buttons.label': 'Label',
      'buttons.url': 'URL',
      'buttons.one': 'Button 1',
      'buttons.two': 'Button 2',
      'buttons.quick': 'Quick fill',
      'buttons.clear': 'Clear buttons',
      'activity.type': 'Activity type',
      'mode.playing': 'Playing',
      'mode.watching': 'Watching',
      'mode.listening': 'Listening',
      'mode.competing': 'Competing',
      'dnd.label': 'Do Not Disturb',
      'dnd.hint': 'Hide the activity; detection keeps running.',
      'labels.label': 'Labels',
      'labels.hint': 'Plan replaces "Subscription" and needs Provider.',
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
      'limits.refresh': 'Refresh',
      'alerts.label': 'Usage alerts',
      'alerts.hint': 'Windows notifications for the 5-hour session.',
      'alerts.80': 'At 80%',
      'alerts.95': 'At 95%',
      'alerts.reset': 'On reset',
      'privacy.projects': 'Private projects',
      'privacy.projectsHint': 'Nothing is shown while you work in these folders. One per line: a folder name or a full path.',
      'privacy.chat': 'Hide in Chat tab',
      'privacy.chatHint': 'Show nothing while the Claude Desktop Chat tab is open.',
      'text.details': 'Main line',
      'text.state': 'Second line',
      'text.hint': 'Leave empty for the default text. Variables: {model} {effort} {plan} {provider} {limits} {limit5h} {limitWeekly} {limitFable} {sessions} {project} {client} {mode}',
      'app.language': 'Language',
      'app.languageAuto': 'Automatic',
      'app.diagnostic': 'Troubleshooting',
      'app.diagnosticHint': 'Copies what the app detects, to paste in a bug report.',
      'app.copyDiagnostic': 'Copy diagnostic',
      'app.copied': 'Diagnostic copied',
      'app.copyFailed': 'Copy failed; the text is selected below',
      'footer.saved': 'Changes save automatically',
      'footer.saving': 'Saving…',
      'footer.close': 'Close',
      'footer.refreshing': 'Refreshing usage…',
      'update.available': 'Update available: v{version}',
      'update.now': 'Update now',
      'update.downloading': 'Downloading update…',
      'update.failed': 'Update failed: {error}',
      'whatsnew.title': 'Updated to v{version}',
      'whatsnew.dismiss': 'Got it',
      'tray.discordConnected': 'Discord connected',
      'tray.discordOff': 'Discord not connected',
      'tray.notRunning': 'Claude not running',
      'tray.sessions': '{count} sessions',
      'tray.alert': '5-hour session above {percent}%',
      'tray.5h': '5-hour session',
      'tray.weekly': 'Weekly',
      'tray.fable': 'Fable weekly',
      'tray.today': 'Today',
      'tray.pause': 'Pause activity',
      'tray.pausedUntil': 'Paused until {time}',
      'tray.dndOn': 'Do Not Disturb on',
      'tray.resume': 'Resume',
      'tray.30min': '30 min',
      'tray.1h': '1 hour',
      'tray.tomorrow': 'Tomorrow',
      'tray.always': 'Always',
      'tray.startup': 'Start on Windows',
      'tray.activity': 'Activity',
      'tray.settings': 'Settings',
      'tray.updates': 'Updates',
      'tray.quit': 'Quit',
      'tray.checking': 'Checking…',
      'tray.upToDate': 'Up to date',
      'tray.checkFailed': 'Check failed',
      'tray.updateFailed': 'Update failed, retry',
      'tray.install': 'Install v{version}',
      'reset.in': 'resets in {duration}',
      'reset.at': 'resets {day} {time}',
      'reset.text': 'resets {text}',
    },
    fr: {
      'theme.dark': 'Sombre',
      'theme.system': 'Système',
      'theme.light': 'Clair',
      'tile.claude': 'Claude',
      'tile.model': 'Modèle',
      'tile.plan': 'Forfait',
      'tile.discord': 'Discord',
      'tile.notRunning': 'Pas lancé',
      'tile.notConnected': 'Non connecté',
      'tile.connected': 'Connecté',
      'preview.hint': 'Ce que les autres voient sur ton profil',
      'preview.notShown': 'Rien sur Discord',
      'preview.nothing': 'Rien n’est affiché sur ton profil pour l’instant.',
      'preview.dnd': 'Ne pas déranger est activé',
      'preview.paused': 'En pause jusqu’à {time}',
      'preview.private': 'Projet privé',
      'preview.chat': 'Onglet Chat masqué',
      'preview.notRunning': 'Claude n’est pas lancé',
      'section.activity': 'Activité',
      'section.details': 'Informations affichées',
      'section.notifications': 'Notifications',
      'section.privacy': 'Confidentialité',
      'section.text': 'Texte Discord',
      'section.buttons': 'Boutons',
      'section.app': 'Application',
      'buttons.watchingOnly': 'mode Regarde uniquement',
      'buttons.note': 'Affichés sur ton activité Discord pour les personnes qui ont Discord Nitro.',
      'buttons.label': 'Libellé',
      'buttons.url': 'Adresse',
      'buttons.one': 'Bouton 1',
      'buttons.two': 'Bouton 2',
      'buttons.quick': 'Remplir',
      'buttons.clear': 'Vider les boutons',
      'activity.type': 'Type d’activité',
      'mode.playing': 'Joue',
      'mode.watching': 'Regarde',
      'mode.listening': 'Écoute',
      'mode.competing': 'Compétition',
      'dnd.label': 'Ne pas déranger',
      'dnd.hint': 'Masque l’activité ; la détection continue.',
      'labels.label': 'Étiquettes',
      'labels.hint': 'Forfait remplace « Subscription » et demande Fournisseur.',
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
      'limits.refresh': 'Actualiser',
      'alerts.label': 'Alertes d’usage',
      'alerts.hint': 'Notifications Windows pour la session de 5 heures.',
      'alerts.80': 'À 80 %',
      'alerts.95': 'À 95 %',
      'alerts.reset': 'À la remise à zéro',
      'privacy.projects': 'Projets privés',
      'privacy.projectsHint': 'Rien n’est affiché quand tu travailles dans ces dossiers. Un par ligne : un nom de dossier ou un chemin complet.',
      'privacy.chat': 'Masquer l’onglet Chat',
      'privacy.chatHint': 'N’affiche rien quand l’onglet Chat de Claude Desktop est ouvert.',
      'text.details': 'Ligne principale',
      'text.state': 'Deuxième ligne',
      'text.hint': 'Laisse vide pour le texte par défaut. Variables : {model} {effort} {plan} {provider} {limits} {limit5h} {limitWeekly} {limitFable} {sessions} {project} {client} {mode}',
      'app.language': 'Langue',
      'app.languageAuto': 'Automatique',
      'app.diagnostic': 'Dépannage',
      'app.diagnosticHint': 'Copie ce que l’app détecte, à coller dans un signalement de bug.',
      'app.copyDiagnostic': 'Copier le diagnostic',
      'app.copied': 'Diagnostic copié',
      'app.copyFailed': 'Copie impossible ; le texte est sélectionné ci-dessous',
      'footer.saved': 'Enregistrement automatique',
      'footer.saving': 'Enregistrement…',
      'footer.close': 'Fermer',
      'footer.refreshing': 'Actualisation de l’usage…',
      'update.available': 'Mise à jour disponible : v{version}',
      'update.now': 'Mettre à jour',
      'update.downloading': 'Téléchargement de la mise à jour…',
      'update.failed': 'Échec de la mise à jour : {error}',
      'whatsnew.title': 'Mis à jour en v{version}',
      'whatsnew.dismiss': 'Compris',
      'tray.discordConnected': 'Discord connecté',
      'tray.discordOff': 'Discord non connecté',
      'tray.notRunning': 'Claude n’est pas lancé',
      'tray.sessions': '{count} sessions',
      'tray.alert': 'Session de 5 heures au-delà de {percent} %',
      'tray.5h': 'Session de 5 heures',
      'tray.weekly': 'Semaine',
      'tray.fable': 'Fable (semaine)',
      'tray.today': 'Aujourd’hui',
      'tray.pause': 'Mettre en pause',
      'tray.pausedUntil': 'En pause jusqu’à {time}',
      'tray.dndOn': 'Ne pas déranger activé',
      'tray.resume': 'Reprendre',
      'tray.30min': '30 min',
      'tray.1h': '1 heure',
      'tray.tomorrow': 'Demain',
      'tray.always': 'Toujours',
      'tray.startup': 'Lancer avec Windows',
      'tray.activity': 'Activité',
      'tray.settings': 'Réglages',
      'tray.updates': 'Mises à jour',
      'tray.quit': 'Quitter',
      'tray.checking': 'Vérification…',
      'tray.upToDate': 'À jour',
      'tray.checkFailed': 'Échec de la vérification',
      'tray.updateFailed': 'Échec, réessayer',
      'tray.install': 'Installer v{version}',
      'reset.in': 'remise à zéro dans {duration}',
      'reset.at': 'remise à zéro {day} {time}',
      'reset.text': 'remise à zéro {text}',
    },
  };

  let language = 'en';

  function resolveLanguage(setting) {
    if (setting === 'en' || setting === 'fr') return setting;
    return (navigator.language || '').toLowerCase().startsWith('fr') ? 'fr' : 'en';
  }

  function t(key, vars = {}) {
    const text = STRINGS[language][key] ?? STRINGS.en[key] ?? key;
    return text.replace(/\{(\w+)\}/g, (match, name) => (name in vars ? String(vars[name]) : match));
  }

  // Fills every [data-i18n] element (and placeholder/title/aria-label variants).
  function applyTranslations(root = document) {
    root.querySelectorAll('[data-i18n]').forEach((el) => {
      el.textContent = t(el.dataset.i18n);
    });
    root.querySelectorAll('[data-i18n-placeholder]').forEach((el) => {
      el.placeholder = t(el.dataset.i18nPlaceholder);
    });
    root.querySelectorAll('[data-i18n-aria]').forEach((el) => {
      el.setAttribute('aria-label', t(el.dataset.i18nAria));
    });
    document.documentElement.lang = language;
  }

  function setLanguage(setting) {
    language = resolveLanguage(setting);
    applyTranslations();
  }

  // Status lines come from status.txt, e.g. "Discord: Connected (inerthel)",
  // "Claude: Desktop (Code)", "Provider: Subscription (Claude Max (5x))".
  function inner(line, prefix) {
    if (!line || !line.startsWith(prefix)) return '';
    const rest = line.slice(prefix.length).trim();
    const open = rest.indexOf('(');
    return open >= 0 && rest.endsWith(')') ? rest.slice(open + 1, -1) : '';
  }

  function formatTime(date) {
    return date.toLocaleTimeString(language === 'fr' ? 'fr-FR' : 'en-GB', {
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  // OAuth resets are ISO dates; Desktop ones are free text ("in 4 hr 31 min").
  function formatReset(reset) {
    if (!reset) return '';
    // Desktop's relative form ("in 4 hr 31 min") is rewritten in the UI language.
    const relative = /^in\s+(?:(\d+)\s*hr?s?)?\s*(?:(\d+)\s*min)?/i.exec(reset.trim());
    if (relative && (relative[1] || relative[2])) {
      const hours = Number(relative[1] || 0);
      const minutes = Number(relative[2] || 0);
      return t('reset.in', { duration: hours ? `${hours}h ${minutes}m` : `${minutes}m` });
    }
    const date = /^\d{4}-\d{2}-\d{2}T/.test(reset) ? new Date(reset) : null;
    if (!date || Number.isNaN(date.getTime())) return t('reset.text', { text: reset });
    const minutes = Math.max(0, Math.round((date.getTime() - Date.now()) / 60000));
    if (minutes < 24 * 60) {
      const hours = Math.floor(minutes / 60);
      const duration = hours ? `${hours}h ${minutes % 60}m` : `${minutes}m`;
      return t('reset.in', { duration });
    }
    const day = date.toLocaleDateString(language === 'fr' ? 'fr-FR' : 'en-US', { weekday: 'short' });
    return t('reset.at', { day, time: formatTime(date) });
  }

  window.RpcShared = {
    t,
    setLanguage,
    applyTranslations,
    inner,
    formatReset,
    formatTime,
    language: () => language,
  };
})();
