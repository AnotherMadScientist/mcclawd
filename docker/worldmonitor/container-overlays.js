// container-overlays.js — LHS X/Twitter stream + RHS event list for WorldMonitor
// Lives OUTSIDE app/ so git pull inside app/ never conflicts.

(function () {
  'use strict';

  var config = window.__wmConfig || {
    xAccounts: [],
    regionHashtags: { Global: ['#OSINT', '#Breaking'] },
    defaultRegion: 'Global',
  };

  // ── Utility ─────────────────────────────────────────────────────────
  function el(tag, attrs, children) {
    var node = document.createElement(tag);
    if (attrs) {
      Object.keys(attrs).forEach(function (k) {
        if (k === 'className') node.className = attrs[k];
        else if (k === 'textContent') node.textContent = attrs[k];
        else if (k === 'innerHTML') node.innerHTML = attrs[k];
        else if (k.startsWith('on')) node.addEventListener(k.slice(2).toLowerCase(), attrs[k]);
        else node.setAttribute(k, attrs[k]);
      });
    }
    if (children) {
      children.forEach(function (child) {
        if (typeof child === 'string') node.appendChild(document.createTextNode(child));
        else if (child) node.appendChild(child);
      });
    }
    return node;
  }

  function timeAgo(date) {
    if (!date || isNaN(date.getTime())) return '';
    var seconds = Math.floor((Date.now() - date.getTime()) / 1000);
    if (seconds < 0) return 'just now';
    if (seconds < 60) return seconds + 's ago';
    var minutes = Math.floor(seconds / 60);
    if (minutes < 60) return minutes + 'm ago';
    var hours = Math.floor(minutes / 60);
    if (hours < 24) return hours + 'h ago';
    return Math.floor(hours / 24) + 'd ago';
  }

  // ── Event type icons ────────────────────────────────────────────────
  function getEventIcon(title) {
    var lower = (title || '').toLowerCase();
    if (/explo|strike|attack|bomb|shell|missile|war/.test(lower)) return '\u{1F4A5}';
    if (/military|troops|army|navy|deploy/.test(lower)) return '\u{1F6E1}\uFE0F';
    if (/protest|demonstrat|rally|march/.test(lower)) return '\u270A';
    if (/earthquake|tsunami|flood|hurricane|storm/.test(lower)) return '\u{1F30A}';
    if (/fire|wildfire|blaze/.test(lower)) return '\u{1F525}';
    if (/nuclear|radiation/.test(lower)) return '\u2622\uFE0F';
    if (/plane|aircraft|flight|aviation|airline/.test(lower)) return '\u2708\uFE0F';
    if (/ship|vessel|maritime|naval/.test(lower)) return '\u{1F6A2}';
    if (/cyber|hack|breach|ransomware/.test(lower)) return '\u{1F4BB}';
    if (/market|stock|trade|economic|gdp/.test(lower)) return '\u{1F4C8}';
    if (/trump|biden|election|congress|sanction|policy/.test(lower)) return '\u{1F3DB}\uFE0F';
    if (/iran|israel|gaza|ukraine|russia|nato/.test(lower)) return '\u{1F310}';
    return '\u{1F4F0}';
  }

  function getSeverityClass(title) {
    var lower = (title || '').toLowerCase();
    if (/breaking|urgent|critical|major|mass|war/.test(lower)) return 'critical';
    if (/attack|strike|explo|missile|bomb|kill/.test(lower)) return 'high';
    if (/military|deploy|troops|protest|clash|drone/.test(lower)) return 'medium';
    return 'info';
  }

  // ── Detect region from URL ──────────────────────────────────────────
  function detectRegion() {
    var params = new URLSearchParams(window.location.search);
    var view = params.get('view') || params.get('region') || '';
    if (/mena|middle.east|iran|israel|syria|iraq/i.test(view)) return 'MENA';
    if (/europe|ukraine|russia|nato/i.test(view)) return 'Europe';
    if (/asia|china|taiwan|korea|pacific/i.test(view)) return 'Asia';
    if (/africa|sahel|sudan|ethiopia/i.test(view)) return 'Africa';
    if (/america|venezuela|mexico/i.test(view)) return 'Americas';
    return config.defaultRegion;
  }

  // ══════════════════════════════════════════════════════════════════════
  // RHS EVENT LIST — scrapes from actual news panel items in DOM
  // ══════════════════════════════════════════════════════════════════════

  function createEventPanel() {
    var panel = el('div', { id: 'wm-event-panel', className: 'wm-overlay-panel' });

    var header = el('div', { className: 'wm-overlay-header' }, [
      el('span', null, [
        el('span', { className: 'wm-overlay-title', textContent: 'Events in View' }),
        el('span', { id: 'wm-event-count', className: 'wm-overlay-count', textContent: '(0)' }),
      ]),
      el('span', { className: 'wm-overlay-toggle', textContent: '\u25B2' }),
    ]);
    header.addEventListener('click', function () {
      panel.classList.toggle('collapsed');
    });

    var body = el('div', { id: 'wm-event-list', className: 'wm-overlay-body' });
    panel.appendChild(header);
    panel.appendChild(body);
    return panel;
  }

  function scrapeEvents() {
    var events = [];
    var seen = {};

    // Scrape from .item-title inside news panels (World News, Intel Feed, etc.)
    document.querySelectorAll('.item-title, .headline-text, .news-headline').forEach(function (el) {
      var title = el.textContent?.trim();
      if (!title || title.length < 10 || title.length > 250) return;
      var key = title.toLowerCase().substring(0, 50);
      if (seen[key]) return;
      seen[key] = true;

      var source = '';
      var sourceEl = el.closest('.item')?.querySelector('.item-source');
      if (sourceEl) source = sourceEl.textContent?.trim() || '';

      var time = null;
      var timeEl = el.closest('.item')?.querySelector('.item-time, .item-date, time');
      if (timeEl) {
        var dt = timeEl.getAttribute('datetime') || timeEl.textContent;
        time = new Date(dt);
        if (isNaN(time.getTime())) time = null;
      }

      events.push({ title: title, source: source, time: time, element: el });
    });

    // Also scrape from links with headlines in panels
    document.querySelectorAll('#panelsGrid a[href*="http"]').forEach(function (a) {
      var title = a.textContent?.trim();
      if (!title || title.length < 15 || title.length > 250) return;
      if (/open on youtube|sign in|learn more/i.test(title)) return;
      var key = title.toLowerCase().substring(0, 50);
      if (seen[key]) return;
      seen[key] = true;

      events.push({ title: title, source: '', time: null, element: a });
    });

    return events;
  }

  function renderEventList(events) {
    var list = document.getElementById('wm-event-list');
    var count = document.getElementById('wm-event-count');
    if (!list || !count) return;

    count.textContent = '(' + events.length + ')';

    if (events.length === 0) {
      list.innerHTML = '';
      list.appendChild(el('div', {
        className: 'wm-event-empty',
        textContent: 'Loading events from news feeds...',
      }));
      return;
    }

    list.innerHTML = '';
    events.slice(0, 80).forEach(function (evt) {
      var row = el('div', { className: 'wm-event-row' }, [
        el('span', { className: 'wm-event-icon', textContent: getEventIcon(evt.title) }),
        el('div', { className: 'wm-event-content' }, [
          el('div', { className: 'wm-event-title' }, [
            el('span', { className: 'wm-event-severity ' + getSeverityClass(evt.title) }),
            document.createTextNode(' ' + evt.title),
          ]),
          el('div', { className: 'wm-event-meta' }, [
            evt.time ? el('span', { className: 'wm-event-time', textContent: timeAgo(evt.time) }) : null,
            evt.source ? el('span', { className: 'wm-event-location', textContent: evt.source }) : null,
          ]),
        ]),
      ]);

      row.addEventListener('click', function () {
        if (evt.element) {
          evt.element.scrollIntoView({ behavior: 'smooth', block: 'center' });
          evt.element.click();
        }
      });

      list.appendChild(row);
    });
  }

  // ══════════════════════════════════════════════════════════════════════
  // LHS X/OSINT FEED — curated accounts + embedded timelines
  // ══════════════════════════════════════════════════════════════════════

  function createXPanel() {
    var panel = el('div', { id: 'wm-x-panel', className: 'wm-overlay-panel' });
    var region = detectRegion();

    var header = el('div', { className: 'wm-overlay-header' }, [
      el('span', null, [
        el('span', { className: 'wm-overlay-title', textContent: 'X / OSINT Feed' }),
      ]),
      el('span', { className: 'wm-overlay-toggle', textContent: '\u25B2' }),
    ]);
    header.addEventListener('click', function () {
      panel.classList.toggle('collapsed');
    });

    var body = el('div', { className: 'wm-overlay-body' });

    // Section 1: Curated OSINT accounts as clickable chips
    var accountsSection = el('div', { className: 'wm-x-section' });
    accountsSection.appendChild(el('div', { className: 'wm-x-section-title', textContent: 'Curated OSINT Accounts' }));
    var accountsContainer = el('div', { className: 'wm-x-accounts' });
    config.xAccounts.forEach(function (account) {
      accountsContainer.appendChild(el('a', {
        className: 'wm-x-account-chip',
        textContent: '@' + account,
        href: 'https://x.com/' + account,
        target: '_blank',
        rel: 'noopener noreferrer',
      }));
    });
    accountsSection.appendChild(accountsContainer);
    body.appendChild(accountsSection);

    // Section 2: Region hashtags
    var hashtagSection = el('div', { className: 'wm-x-section' });
    hashtagSection.appendChild(el('div', { className: 'wm-x-section-title', textContent: 'Region Hashtags' }));
    hashtagSection.appendChild(el('div', {
      id: 'wm-x-region-label',
      className: 'wm-x-region-label',
      textContent: '\u{1F30D} ' + region,
    }));
    var hashtagContainer = el('div', { id: 'wm-x-hashtags', className: 'wm-x-hashtags' });
    renderHashtags(hashtagContainer, region);
    hashtagSection.appendChild(hashtagContainer);
    body.appendChild(hashtagSection);

    // Section 3: Live news feed (fetched from WorldMonitor's own API)
    var feedSection = el('div', { className: 'wm-x-section' });
    feedSection.appendChild(el('div', { className: 'wm-x-section-title', textContent: 'Live Feed' }));
    var feedContainer = el('div', { id: 'wm-x-feed', className: 'wm-x-feed-list' });
    feedContainer.appendChild(el('div', { className: 'wm-x-loading', textContent: 'Loading live feed...' }));
    feedSection.appendChild(feedContainer);
    body.appendChild(feedSection);

    // Section 4: OSINT community links
    var linksSection = el('div', { className: 'wm-x-section' });
    linksSection.appendChild(el('div', { className: 'wm-x-section-title', textContent: 'OSINT Community' }));
    var linksContainer = el('div', { id: 'wm-x-community', className: 'wm-x-feed-list' });
    var osintLinks = [
      { name: 'Bellingcat', url: 'https://www.bellingcat.com' },
      { name: 'ACLED Dashboard', url: 'https://acleddata.com/dashboard' },
      { name: 'Liveuamap', url: 'https://liveuamap.com' },
      { name: 'FIRMS Fire Map', url: 'https://firms.modaps.eosdis.nasa.gov/map' },
      { name: 'FlightRadar24', url: 'https://www.flightradar24.com' },
      { name: 'MarineTraffic', url: 'https://www.marinetraffic.com' },
    ];
    osintLinks.forEach(function (link) {
      linksContainer.appendChild(el('a', {
        className: 'wm-x-community-link',
        textContent: link.name,
        href: link.url,
        target: '_blank',
        rel: 'noopener noreferrer',
      }));
    });
    linksSection.appendChild(linksContainer);
    body.appendChild(linksSection);

    panel.appendChild(header);
    panel.appendChild(body);
    return panel;
  }

  function renderHashtags(container, region) {
    container.innerHTML = '';
    var tags = config.regionHashtags[region] || config.regionHashtags.Global || [];
    tags.forEach(function (tag) {
      container.appendChild(el('a', {
        className: 'wm-x-hashtag',
        textContent: tag,
        href: 'https://x.com/search?q=' + encodeURIComponent(tag) + '&f=live',
        target: '_blank',
        rel: 'noopener noreferrer',
      }));
    });
  }

  // ── RSS feed fetcher for LHS panel ──────────────────────────────────
  function fetchLiveFeed() {
    var container = document.getElementById('wm-x-feed');
    if (!container) return;

    // Scrape headlines from the app's own news panels (same as RHS but rendered differently)
    var items = [];
    var seen = {};
    document.querySelectorAll('.item-title, .headline-text, .news-headline').forEach(function (el) {
      var title = el.textContent?.trim();
      if (!title || title.length < 10 || title.length > 250) return;
      var key = title.toLowerCase().substring(0, 50);
      if (seen[key]) return;
      seen[key] = true;

      var source = '';
      var sourceEl = el.closest('.item')?.querySelector('.item-source');
      if (sourceEl) source = sourceEl.textContent?.trim() || '';

      var link = el.closest('a')?.href || '';
      items.push({ title: title, source: source, link: link });
    });

    if (items.length === 0) {
      container.innerHTML = '';
      container.appendChild(el('div', { className: 'wm-x-loading', textContent: 'Waiting for news data...' }));
      return;
    }

    container.innerHTML = '';
    items.slice(0, 15).forEach(function (item) {
      var row = el('div', { className: 'wm-x-feed-item' });
      var titleEl = item.link
        ? el('a', {
            className: 'wm-x-feed-title',
            textContent: item.title,
            href: item.link,
            target: '_blank',
            rel: 'noopener noreferrer',
          })
        : el('div', { className: 'wm-x-feed-title', textContent: item.title });
      row.appendChild(titleEl);
      if (item.source) {
        row.appendChild(el('span', { className: 'wm-x-feed-source', textContent: item.source }));
      }
      container.appendChild(row);
    });
  }

  // ══════════════════════════════════════════════════════════════════════
  // INIT
  // ══════════════════════════════════════════════════════════════════════

  function init() {
    if (window.innerWidth < 900) return;

    var eventPanel = createEventPanel();
    var xPanel = createXPanel();
    document.body.appendChild(eventPanel);
    document.body.appendChild(xPanel);

    // Poll for events from news panels + update LHS feed
    function pollAll() {
      var events = scrapeEvents();
      renderEventList(events);
      fetchLiveFeed();
    }
    // First poll after panels have loaded
    setTimeout(pollAll, 3000);
    setInterval(pollAll, 8000);

    // Watch for URL changes
    var lastUrl = window.location.href;
    setInterval(function () {
      if (window.location.href !== lastUrl) {
        lastUrl = window.location.href;
        var region = detectRegion();
        var label = document.getElementById('wm-x-region-label');
        var container = document.getElementById('wm-x-hashtags');
        if (label) label.textContent = '\u{1F30D} ' + region;
        if (container) renderHashtags(container, region);
      }
    }, 3000);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', function () { setTimeout(init, 2500); });
  } else {
    setTimeout(init, 2500);
  }
})();
