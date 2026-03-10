import { motion } from 'motion/react';
import {
  Globe, Activity, ShieldAlert, Zap, Terminal, Database,
  Send, MessageCircle, Mail, MessageSquare, ChevronDown,
  ArrowRight, Check, Lock, Server, Cpu, Layers,
  Bell, Brain, Key, Plug, PanelTop, ExternalLink,
  BarChart3, Clock, Radio, Ship, Plane, Flame,
  Cable, Wifi, MapPin, Users, TrendingUp
} from 'lucide-react';
import { t } from './i18n';

const API_BASE = location.hostname === 'localhost' ? 'https://api.worldmonitor.app' : '/api';
const TURNSTILE_SITE_KEY = '0x4AAAAAACnaYgHIyxclu8Tj';
const PRO_URL = 'https://worldmonitor.app/pro';

declare global {
  interface Window { turnstile?: { getResponse: (id?: string) => string | undefined; reset: (id?: string) => void; }; }
}

function getRefCode(): string | undefined {
  const params = new URLSearchParams(window.location.search);
  return params.get('ref') || undefined;
}

function sanitize(val: unknown): string {
  return String(val ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c] || c));
}

function showReferralSuccess(formEl: HTMLFormElement, data: { referralCode?: string; position?: number; status?: string }) {
  if (!data.referralCode) {
    const btn = formEl.querySelector('button[type="submit"]') as HTMLButtonElement;
    if (btn) { btn.textContent = t('form.joinWaitlist'); btn.disabled = false; }
    return;
  }
  const safeCode = sanitize(data.referralCode);
  const safePosition = sanitize(data.position);
  const referralLink = `${PRO_URL}?ref=${safeCode}`;
  const shareText = encodeURIComponent(t('referral.shareText'));
  const shareUrl = encodeURIComponent(referralLink);

  const el = (tag: string, cls: string, text?: string) => {
    const node = document.createElement(tag);
    node.className = cls;
    if (text) node.textContent = text;
    return node;
  };

  const successDiv = el('div', 'text-center');

  const isAlreadyRegistered = data.status === 'already_registered';
  const shareHint = t('referral.shareHint');

  if (isAlreadyRegistered) {
    successDiv.appendChild(el('p', 'text-lg font-display font-bold text-wm-green mb-2', t('referral.alreadyOnList')));
    successDiv.appendChild(el('p', 'text-sm text-wm-muted mb-4', shareHint));
  } else {
    const badge = el('div', 'inline-block bg-wm-card border border-wm-green/30 px-6 py-4 mb-4');
    badge.appendChild(el('p', 'text-xs text-wm-green font-mono uppercase tracking-widest mb-1', t('referral.yourPosition')));
    badge.appendChild(el('p', 'text-4xl font-display font-bold text-wm-text', `#${safePosition || '?'}`));
    successDiv.appendChild(badge);
    successDiv.appendChild(el('p', 'text-sm text-wm-muted mb-4', shareHint));
  }

  const linkBox = el('div', 'bg-wm-card border border-wm-border px-4 py-3 mb-4 font-mono text-xs text-wm-green break-all select-all cursor-pointer', referralLink);
  linkBox.addEventListener('click', () => {
    navigator.clipboard.writeText(referralLink).then(() => {
      linkBox.textContent = t('referral.copied');
      setTimeout(() => { linkBox.textContent = referralLink; }, 2000);
    });
  });
  successDiv.appendChild(linkBox);

  const shareRow = el('div', 'flex gap-3 justify-center flex-wrap');
  const shareLinks = [
    { label: t('referral.shareOnX'), href: `https://x.com/intent/tweet?text=${shareText}&url=${shareUrl}` },
    { label: t('referral.linkedin'), href: `https://www.linkedin.com/sharing/share-offsite/?url=${shareUrl}` },
    { label: t('referral.whatsapp'), href: `https://wa.me/?text=${shareText}%20${shareUrl}` },
    { label: t('referral.telegram'), href: `https://t.me/share/url?url=${shareUrl}&text=${encodeURIComponent(t('referral.joinWaitlistShare'))}` },
  ];
  for (const s of shareLinks) {
    const a = el('a', 'bg-wm-card border border-wm-border px-4 py-2 text-xs font-mono text-wm-muted hover:text-wm-text hover:border-wm-text transition-colors', s.label);
    (a as HTMLAnchorElement).href = s.href;
    (a as HTMLAnchorElement).target = '_blank';
    (a as HTMLAnchorElement).rel = 'noreferrer';
    shareRow.appendChild(a);
  }
  successDiv.appendChild(shareRow);

  formEl.replaceWith(successDiv);
}

async function submitWaitlist(email: string, formEl: HTMLFormElement) {
  const btn = formEl.querySelector('button[type="submit"]') as HTMLButtonElement;
  const origText = btn.textContent;
  btn.disabled = true;
  btn.textContent = t('form.submitting');

  const honeypot = (formEl.querySelector('input[name="website"]') as HTMLInputElement)?.value || '';
  const turnstileWidget = formEl.querySelector('.cf-turnstile') as HTMLElement | null;
  const widgetId = turnstileWidget?.dataset.widgetId;
  const turnstileToken = window.turnstile?.getResponse(widgetId) || '';
  const ref = getRefCode();

  try {
    const res = await fetch(`${API_BASE}/register-interest`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email, source: 'pro-waitlist', website: honeypot, turnstileToken, referredBy: ref }),
    });
    const data = await res.json();
    if (!res.ok) throw new Error(data.error || 'Registration failed');
    showReferralSuccess(formEl, { referralCode: data.referralCode, position: data.position, status: data.status });
  } catch (err: any) {
    btn.textContent = err.message === 'Too many requests' ? t('form.tooManyRequests') : t('form.failedTryAgain');
    btn.disabled = false;
    window.turnstile?.reset(widgetId);
    setTimeout(() => { btn.textContent = origText; }, 3000);
  }
}

const SlackIcon = () => (
  <svg viewBox="0 0 24 24" className="w-5 h-5" fill="currentColor" aria-hidden="true">
    <path d="M5.042 15.165a2.528 2.528 0 0 1-2.52 2.523A2.528 2.528 0 0 1 0 15.165a2.527 2.527 0 0 1 2.522-2.52h2.52v2.52zM6.313 15.165a2.527 2.527 0 0 1 2.521-2.52 2.527 2.527 0 0 1 2.521 2.52v6.313A2.528 2.528 0 0 1 8.834 24a2.528 2.528 0 0 1-2.521-2.522v-6.313zM8.834 5.042a2.528 2.528 0 0 1-2.521-2.52A2.528 2.528 0 0 1 8.834 0a2.528 2.528 0 0 1 2.521 2.522v2.52H8.834zM8.834 6.313a2.528 2.528 0 0 1 2.521 2.521 2.528 2.528 0 0 1-2.521 2.521H2.522A2.528 2.528 0 0 1 0 8.834a2.528 2.528 0 0 1 2.522-2.521h6.312zM18.956 8.834a2.528 2.528 0 0 1 2.522-2.521A2.528 2.528 0 0 1 24 8.834a2.528 2.528 0 0 1-2.522 2.521h-2.522V8.834zM17.688 8.834a2.528 2.528 0 0 1-2.523 2.521 2.527 2.527 0 0 1-2.52-2.521V2.522A2.527 2.527 0 0 1 15.165 0a2.528 2.528 0 0 1 2.523 2.522v6.312zM15.165 18.956a2.528 2.528 0 0 1 2.523 2.522A2.528 2.528 0 0 1 15.165 24a2.527 2.527 0 0 1-2.52-2.522v-2.522h2.52zM15.165 17.688a2.527 2.527 0 0 1-2.52-2.523 2.526 2.526 0 0 1 2.52-2.52h6.313A2.527 2.527 0 0 1 24 15.165a2.528 2.528 0 0 1-2.522 2.523h-6.313z"/>
  </svg>
);

const Logo = () => (
  <a href="https://worldmonitor.app" className="flex items-center gap-2 hover:opacity-80 transition-opacity" aria-label="World Monitor — Home">
    <div className="relative w-8 h-8 rounded-full bg-wm-card border border-wm-border flex items-center justify-center overflow-hidden">
      <Globe className="w-5 h-5 text-wm-blue opacity-50 absolute" aria-hidden="true" />
      <Activity className="w-6 h-6 text-wm-green absolute z-10" aria-hidden="true" />
    </div>
    <div className="flex flex-col">
      <span className="font-display font-bold text-sm leading-none tracking-tight">WORLD MONITOR</span>
      <span className="text-[9px] text-wm-muted font-mono uppercase tracking-widest leading-none mt-1">by Someone.ceo</span>
    </div>
  </a>
);

const Navbar = () => (
  <nav className="fixed top-0 left-0 right-0 z-50 glass-panel border-b-0 border-x-0 rounded-none" aria-label="Main navigation">
    <div className="max-w-7xl mx-auto px-6 h-16 flex items-center justify-between">
      <Logo />
      <div className="hidden md:flex items-center gap-8 text-sm font-mono text-wm-muted">
        <a href="#tiers" className="hover:text-wm-text transition-colors">{t('nav.free')}</a>
        <a href="#pro" className="hover:text-wm-green transition-colors">{t('nav.pro')}</a>
        <a href="#api" className="hover:text-wm-text transition-colors">{t('nav.api')}</a>
        <a href="#enterprise" className="hover:text-wm-text transition-colors">{t('nav.enterprise')}</a>
      </div>
      <a href="#waitlist" className="bg-wm-green text-wm-bg px-4 py-2 rounded-sm font-mono text-xs uppercase tracking-wider font-bold hover:bg-green-400 transition-colors">
        {t('nav.joinWaitlist')}
      </a>
    </div>
  </nav>
);

const WiredBadge = () => (
  <a
    href="https://www.wired.me/story/the-music-streaming-ceo-who-built-a-global-war-map"
    target="_blank"
    rel="noreferrer"
    className="inline-flex items-center gap-2 px-3 py-1.5 rounded-full border border-wm-border bg-wm-card/50 text-wm-muted text-xs font-mono hover:border-wm-green/30 hover:text-wm-text transition-colors"
  >
    {t('wired.asFeaturedIn')} <span className="text-wm-text font-bold">WIRED</span> <ExternalLink className="w-3 h-3" aria-hidden="true" />
  </a>
);

const Hero = () => (
  <section className="pt-28 pb-16 px-6 relative overflow-hidden">
    <div className="absolute inset-0 bg-[radial-gradient(circle_at_50%_30%,rgba(74,222,128,0.05)_0%,transparent_60%)] pointer-events-none" />
    <div className="max-w-4xl mx-auto text-center relative z-10">
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.6 }}
      >
        <div className="mb-6">
          <WiredBadge />
        </div>
        <h1 className="text-5xl md:text-7xl font-display font-bold tracking-tighter mb-6 leading-[1.1]">
          {t('hero.title1')} <br className="hidden md:block" />
          <span className="text-transparent bg-clip-text bg-gradient-to-r from-wm-green to-emerald-300">{t('hero.title2')}</span>
        </h1>
        <p className="text-lg md:text-xl text-wm-muted mb-8 max-w-2xl mx-auto font-light">
          {t('hero.subtitle')}
        </p>

        <form className="flex flex-col gap-3 max-w-md mx-auto" onSubmit={(e) => { e.preventDefault(); const form = e.currentTarget; const email = new FormData(form).get('email') as string; submitWaitlist(email, form); }}>
          <input type="text" name="website" autoComplete="off" tabIndex={-1} aria-hidden="true" className="absolute opacity-0 h-0 w-0 pointer-events-none" />
          <div className="flex flex-col sm:flex-row gap-3">
            <input
              type="email"
              name="email"
              placeholder={t('hero.emailPlaceholder')}
              className="flex-1 bg-wm-card border border-wm-border rounded-sm px-4 py-3 text-sm focus:outline-none focus:border-wm-green transition-colors font-mono"
              required
              aria-label={t('hero.emailAriaLabel')}
            />
            <button type="submit" className="bg-wm-green text-wm-bg px-6 py-3 rounded-sm font-mono text-sm uppercase tracking-wider font-bold hover:bg-green-400 transition-colors flex items-center justify-center gap-2 whitespace-nowrap">
              {t('hero.joinProWaitlist')} <ArrowRight className="w-4 h-4" aria-hidden="true" />
            </button>
          </div>
          <div className="cf-turnstile mx-auto" data-sitekey={TURNSTILE_SITE_KEY} data-theme="dark" data-size="compact" />
        </form>
        <div className="flex items-center justify-center gap-4 mt-4">
          <p className="text-xs text-wm-muted font-mono">{t('hero.launchingSoon')}</p>
          <span className="text-wm-border">|</span>
          <a href="https://worldmonitor.app" className="text-xs text-wm-green font-mono hover:text-green-300 transition-colors flex items-center gap-1">
            {t('hero.tryFreeDashboard')} <ArrowRight className="w-3 h-3" aria-hidden="true" />
          </a>
        </div>
      </motion.div>
    </div>
  </section>
);

const LivePreview = () => (
  <section className="px-6 pb-16 -mt-4">
    <div className="max-w-6xl mx-auto">
      <div className="relative rounded-lg overflow-hidden border border-wm-border shadow-2xl shadow-wm-green/5">
        <div className="bg-wm-card px-4 py-2 border-b border-wm-border flex items-center gap-3">
          <div className="flex gap-1.5">
            <div className="w-3 h-3 rounded-full bg-red-500/70" />
            <div className="w-3 h-3 rounded-full bg-yellow-500/70" />
            <div className="w-3 h-3 rounded-full bg-green-500/70" />
          </div>
          <span className="font-mono text-xs text-wm-muted ml-2">{t('livePreview.windowTitle')}</span>
          <a
            href="https://worldmonitor.app"
            target="_blank"
            rel="noreferrer"
            className="ml-auto text-xs text-wm-green font-mono hover:text-green-300 transition-colors flex items-center gap-1"
          >
            {t('livePreview.openFullScreen')} <ExternalLink className="w-3 h-3" aria-hidden="true" />
          </a>
        </div>
        <div className="relative aspect-[16/9] bg-black">
          <iframe
            src="https://worldmonitor.app"
            title={t('livePreview.iframeTitle')}
            className="w-full h-full border-0"
            loading="lazy"
            sandbox="allow-scripts allow-same-origin"
          />
          <div className="absolute inset-0 pointer-events-none bg-gradient-to-t from-wm-bg/80 via-transparent to-transparent" />
          <div className="absolute bottom-4 left-0 right-0 text-center pointer-events-auto">
            <a
              href="https://worldmonitor.app"
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-2 bg-wm-green text-wm-bg px-6 py-3 rounded-sm font-mono text-sm uppercase tracking-wider font-bold hover:bg-green-400 transition-colors"
            >
              {t('livePreview.tryLiveDashboard')} <ArrowRight className="w-4 h-4" aria-hidden="true" />
            </a>
          </div>
        </div>
      </div>
      <p className="text-center text-xs text-wm-muted font-mono mt-4">
        {t('livePreview.description')}
      </p>
    </div>
  </section>
);

const SourceMarquee = () => {
  const sources = [
    "ACLED", "UCDP", "GDELT", "NASA FIRMS", "USGS", "OpenSky", "AISStream",
    "Finnhub", "FRED", "CoinGecko", "Polymarket", "UNHCR", "Cloudflare Radar",
    "BGPStream", "GPSJam", "NOAA", "Copernicus", "IAEA", "Bloomberg",
    "Reuters", "Al Jazeera", "Sky News", "Euronews", "DW News", "France 24",
    "CNBC", "Nikkei", "Haaretz", "Al Arabiya", "TRT World",
    "Defense One", "Jane's", "The War Zone", "Maritime Executive",
    "OilPrice", "Rigzone", "Hellenic Shipping News",
    "TechCrunch", "Ars Technica", "The Verge", "Wired",
    "Krebs on Security", "BleepingComputer", "The Record",
  ];
  const items = sources.join(" · ");
  return (
    <section className="border-y border-wm-border bg-wm-card/20 overflow-hidden py-4" aria-label="Data sources">
      <div className="marquee-track whitespace-nowrap font-mono text-xs text-wm-muted uppercase tracking-widest">
        <span className="inline-block px-4">{items} · </span>
        <span className="inline-block px-4">{items} · </span>
      </div>
    </section>
  );
};

const SocialProof = () => (
  <section className="border-y border-wm-border bg-wm-card/30 py-16 px-6">
    <div className="max-w-5xl mx-auto">
      <div className="grid grid-cols-2 md:grid-cols-4 gap-8 text-center mb-12">
        {[
          { value: "2M+", label: t('socialProof.uniqueVisitors') },
          { value: "216K", label: t('socialProof.peakDailyUsers') },
          { value: "190+", label: t('socialProof.countriesReached') },
          { value: "435+", label: t('socialProof.liveDataSources') },
        ].map((stat, i) => (
          <div key={i}>
            <p className="text-3xl md:text-4xl font-display font-bold text-wm-green">{stat.value}</p>
            <p className="text-xs font-mono text-wm-muted uppercase tracking-widest mt-1">{stat.label}</p>
          </div>
        ))}
      </div>
      <blockquote className="max-w-3xl mx-auto text-center">
        <p className="text-lg md:text-xl text-wm-muted italic leading-relaxed">
          "{t('socialProof.quote')}"
        </p>
        <footer className="mt-6 flex items-center justify-center gap-3">
          <div className="text-sm">
            <span className="text-wm-text font-bold">Elie Habib</span>
            <span className="text-wm-muted"> — {t('socialProof.ceo')} </span>
            <a href="https://anghami.com" target="_blank" rel="noreferrer" className="text-wm-muted underline underline-offset-4 hover:text-wm-text transition-colors">Anghami</a>
            <span className="text-wm-muted">, {t('socialProof.asToldTo')} </span>
            <a href="https://www.wired.me/story/the-music-streaming-ceo-who-built-a-global-war-map" target="_blank" rel="noreferrer" className="text-wm-text underline underline-offset-4 hover:text-wm-green transition-colors">WIRED</a>
          </div>
        </footer>
      </blockquote>
    </div>
  </section>
);

const DataCoverage = () => {
  const domains = [
    { icon: <Radio className="w-5 h-5" aria-hidden="true" />, name: t('dataCoverage.geopolitical'), desc: t('dataCoverage.geopoliticalDesc') },
    { icon: <Plane className="w-5 h-5" aria-hidden="true" />, name: t('dataCoverage.aviation'), desc: t('dataCoverage.aviationDesc') },
    { icon: <Ship className="w-5 h-5" aria-hidden="true" />, name: t('dataCoverage.maritime'), desc: t('dataCoverage.maritimeDesc') },
    { icon: <Flame className="w-5 h-5" aria-hidden="true" />, name: t('dataCoverage.fire'), desc: t('dataCoverage.fireDesc') },
    { icon: <Cable className="w-5 h-5" aria-hidden="true" />, name: t('dataCoverage.cables'), desc: t('dataCoverage.cablesDesc') },
    { icon: <Wifi className="w-5 h-5" aria-hidden="true" />, name: t('dataCoverage.internet'), desc: t('dataCoverage.internetDesc') },
    { icon: <MapPin className="w-5 h-5" aria-hidden="true" />, name: t('dataCoverage.infra'), desc: t('dataCoverage.infraDesc') },
    { icon: <TrendingUp className="w-5 h-5" aria-hidden="true" />, name: t('dataCoverage.markets'), desc: t('dataCoverage.marketsDesc') },
    { icon: <ShieldAlert className="w-5 h-5" aria-hidden="true" />, name: t('dataCoverage.cyber'), desc: t('dataCoverage.cyberDesc') },
    { icon: <Globe className="w-5 h-5" aria-hidden="true" />, name: t('dataCoverage.gdelt'), desc: t('dataCoverage.gdeltDesc') },
    { icon: <Users className="w-5 h-5" aria-hidden="true" />, name: t('dataCoverage.unrest'), desc: t('dataCoverage.unrestDesc') },
    { icon: <Activity className="w-5 h-5" aria-hidden="true" />, name: t('dataCoverage.seismology'), desc: t('dataCoverage.seismologyDesc') },
  ];

  return (
    <section className="py-24 px-6" id="coverage">
      <div className="max-w-7xl mx-auto">
        <div className="text-center mb-16">
          <h2 className="text-3xl md:text-5xl font-display font-bold mb-6">{t('dataCoverage.title')}</h2>
          <p className="text-wm-muted max-w-2xl mx-auto">
            {t('dataCoverage.subtitle')}
          </p>
        </div>
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
          {domains.map((d, i) => (
            <div key={i} className="bg-wm-card border border-wm-border p-4 hover:border-wm-green/30 transition-colors">
              <div className="text-wm-green mb-3">{d.icon}</div>
              <h3 className="font-bold text-sm mb-1">{d.name}</h3>
              <p className="text-xs text-wm-muted">{d.desc}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
};

const Tiers = () => {
  const tiers = [
    {
      name: t('tiers.free'),
      tagline: t('tiers.freeTagline'),
      desc: t('tiers.freeDesc'),
      features: [t('tiers.freeF1'), t('tiers.freeF2'), t('tiers.freeF3'), t('tiers.freeF4')],
      color: "border-wm-border",
      cta: { label: t('tiers.openDashboard'), href: "https://worldmonitor.app" }
    },
    {
      name: t('tiers.pro'),
      tagline: t('tiers.proTagline'),
      desc: t('tiers.proDesc'),
      features: [t('tiers.proF1'), t('tiers.proF2'), t('tiers.proF3'), t('tiers.proF4')],
      color: "border-wm-green",
      glow: true,
      cta: { label: t('nav.joinWaitlist'), href: "#waitlist" }
    },
    {
      name: t('tiers.enterprise'),
      tagline: t('tiers.enterpriseTagline'),
      desc: t('tiers.enterpriseDesc'),
      features: [t('tiers.entF1'), t('tiers.entF2'), t('tiers.entF3'), t('tiers.entF4')],
      color: "border-wm-border",
      cta: { label: t('tiers.contactSales'), href: "mailto:enterprise@worldmonitor.app" }
    }
  ];

  return (
    <section className="py-24 px-6 max-w-7xl mx-auto" id="tiers">
      <div className="grid md:grid-cols-3 gap-6">
        {tiers.map((tier, i) => (
          <div key={i} className={`bg-wm-card border ${tier.color} p-8 relative ${tier.glow ? 'border-glow' : ''}`}>
            {tier.glow && <div className="absolute top-0 left-0 w-full h-1 bg-wm-green" />}
            <h3 className="font-display text-2xl font-bold mb-2">{tier.name}</h3>
            <p className="text-wm-muted font-mono text-sm mb-1">{tier.tagline}</p>
            <p className="text-sm font-medium mb-8 pb-8 border-b border-wm-border">{tier.desc}</p>
            <ul className="space-y-4 mb-8">
              {tier.features.map((f, j) => (
                <li key={j} className="flex items-start gap-3 text-sm">
                  <Check className={`w-4 h-4 shrink-0 mt-0.5 ${tier.glow ? 'text-wm-green' : 'text-wm-muted'}`} aria-hidden="true" />
                  <span className="text-wm-muted">{f}</span>
                </li>
              ))}
            </ul>
            <a
              href={tier.cta.href}
              className={`block text-center py-2.5 rounded-sm font-mono text-xs uppercase tracking-wider font-bold transition-colors ${
                tier.glow
                  ? 'bg-wm-green text-wm-bg hover:bg-green-400'
                  : 'border border-wm-border text-wm-muted hover:text-wm-text hover:border-wm-text'
              }`}
            >
              {tier.cta.label}
            </a>
          </div>
        ))}
      </div>
    </section>
  );
};

const ProShowcase = () => (
  <section className="py-24 px-6 border-t border-wm-border bg-wm-card/30" id="pro">
    <div className="max-w-7xl mx-auto grid lg:grid-cols-2 gap-16 items-start">
      <div>
        <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-wm-green/30 bg-wm-green/10 text-wm-green text-xs font-mono mb-6">
          {t('proShowcase.proTier')}
        </div>
        <h2 className="text-3xl md:text-5xl font-display font-bold mb-6">{t('proShowcase.title')}</h2>
        <p className="text-wm-muted mb-8">
          {t('proShowcase.subtitle')}
        </p>

        <div className="space-y-6">
          <div className="flex gap-4">
            <Zap className="w-6 h-6 text-wm-green shrink-0" aria-hidden="true" />
            <div>
              <h4 className="font-bold mb-1">{t('proShowcase.nearRealTime')}</h4>
              <p className="text-sm text-wm-muted">{t('proShowcase.nearRealTimeDesc')}</p>
            </div>
          </div>
          <div className="flex gap-4">
            <Brain className="w-6 h-6 text-wm-green shrink-0" aria-hidden="true" />
            <div>
              <h4 className="font-bold mb-1">{t('proShowcase.soWhat')}</h4>
              <p className="text-sm text-wm-muted">{t('proShowcase.soWhatDesc')}</p>
            </div>
          </div>
          <div className="flex gap-4">
            <Clock className="w-6 h-6 text-wm-green shrink-0" aria-hidden="true" />
            <div>
              <h4 className="font-bold mb-1">{t('proShowcase.morningBriefs')}</h4>
              <p className="text-sm text-wm-muted">{t('proShowcase.morningBriefsDesc')}</p>
            </div>
          </div>
          <div className="flex gap-4">
            <Bell className="w-6 h-6 text-wm-green shrink-0" aria-hidden="true" />
            <div>
              <h4 className="font-bold mb-1">{t('proShowcase.alerting')}</h4>
              <p className="text-sm text-wm-muted">{t('proShowcase.alertingDesc')}</p>
            </div>
          </div>
          <div className="flex gap-4">
            <Key className="w-6 h-6 text-wm-green shrink-0" aria-hidden="true" />
            <div>
              <h4 className="font-bold mb-1">{t('proShowcase.oneKey')}</h4>
              <p className="text-sm text-wm-muted">{t('proShowcase.oneKeyDesc')}</p>
            </div>
          </div>
        </div>

        <div className="mt-10 pt-8 border-t border-wm-border">
          <p className="font-mono text-xs text-wm-muted uppercase tracking-widest mb-4">{t('proShowcase.deliveryLabel')}</p>
          <div className="flex gap-6">
            {[
              { icon: <SlackIcon />, label: "Slack" },
              { icon: <Send className="w-5 h-5" aria-hidden="true" />, label: "Telegram" },
              { icon: <MessageCircle className="w-5 h-5" aria-hidden="true" />, label: "WhatsApp" },
              { icon: <Mail className="w-5 h-5" aria-hidden="true" />, label: "Email" },
              { icon: <MessageSquare className="w-5 h-5" aria-hidden="true" />, label: "Discord" },
            ].map((ch, i) => (
              <div key={i} className="flex flex-col items-center gap-1.5 text-wm-muted hover:text-wm-text transition-colors cursor-pointer">
                {ch.icon}
                <span className="text-[10px] font-mono">{ch.label}</span>
              </div>
            ))}
          </div>
        </div>
      </div>

      <div className="bg-[#1a1d21] rounded-lg border border-[#35373b] overflow-hidden shadow-2xl sticky top-24">
        <div className="bg-[#222529] px-4 py-3 border-b border-[#35373b] flex items-center gap-3">
          <div className="w-3 h-3 rounded-full bg-red-500" />
          <div className="w-3 h-3 rounded-full bg-yellow-500" />
          <div className="w-3 h-3 rounded-full bg-green-500" />
          <span className="ml-2 font-mono text-xs text-gray-400">#world-monitor-alerts</span>
        </div>
        <div className="p-6 space-y-6 font-sans text-sm">
          <div className="flex gap-4">
            <div className="w-10 h-10 rounded bg-wm-green/20 flex items-center justify-center shrink-0">
              <Globe className="w-6 h-6 text-wm-green" aria-hidden="true" />
            </div>
            <div>
              <div className="flex items-baseline gap-2 mb-1">
                <span className="font-bold text-gray-200">World Monitor</span>
                <span className="text-xs text-gray-500 bg-gray-800 px-1 rounded">APP</span>
                <span className="text-xs text-gray-500">8:00 AM</span>
              </div>
              <p className="text-gray-300 font-bold mb-3">{t('slackMock.morningBrief')} &middot; Mar 6</p>

              <div className="space-y-3">
                <div className="pl-3 border-l-2 border-red-500">
                  <span className="text-red-400 font-bold text-xs uppercase tracking-wider">{t('slackMock.critical')}</span>
                  <p className="text-gray-300 mt-1">{t('slackMock.criticalText')}</p>
                </div>

                <div className="pl-3 border-l-2 border-orange-500">
                  <span className="text-orange-400 font-bold text-xs uppercase tracking-wider">{t('slackMock.elevated')}</span>
                  <p className="text-gray-300 mt-1">{t('slackMock.elevatedText')}</p>
                </div>

                <div className="pl-3 border-l-2 border-yellow-500">
                  <span className="text-yellow-400 font-bold text-xs uppercase tracking-wider">{t('slackMock.watch')}</span>
                  <p className="text-gray-300 mt-1">{t('slackMock.watchText')}</p>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
);

const ApiSection = () => (
  <section className="py-24 px-6 border-y border-wm-border bg-[#0a0a0a]" id="api">
    <div className="max-w-7xl mx-auto grid lg:grid-cols-2 gap-16 items-center">
      <div className="order-2 lg:order-1">
        <div className="bg-black border border-wm-border rounded-lg overflow-hidden font-mono text-sm">
          <div className="bg-wm-card px-4 py-2 border-b border-wm-border flex items-center gap-2">
            <Terminal className="w-4 h-4 text-wm-muted" aria-hidden="true" />
            <span className="text-wm-muted text-xs">api.worldmonitor.app</span>
          </div>
          <div className="p-6 text-gray-300 overflow-x-auto">
            <pre><code>
<span className="text-wm-blue">curl</span> \<br/>
  <span className="text-wm-green">"https://api.worldmonitor.app/v1/intelligence/convergence?region=MENA&time_window=6h"</span> \<br/>
  -H <span className="text-wm-green">"Authorization: Bearer wm_live_xxx"</span><br/><br/>
<span className="text-wm-muted">{"{"}</span><br/>
  <span className="text-wm-blue">"status"</span>: <span className="text-wm-green">"success"</span>,<br/>
  <span className="text-wm-blue">"data"</span>: <span className="text-wm-muted">{"["}</span><br/>
    <span className="text-wm-muted">{"{"}</span><br/>
      <span className="text-wm-blue">"type"</span>: <span className="text-wm-green">"multi_signal_convergence"</span>,<br/>
      <span className="text-wm-blue">"signals"</span>: <span className="text-wm-muted">["military_flights", "ais_dark_ships", "oref_sirens"]</span>,<br/>
      <span className="text-wm-blue">"confidence"</span>: <span className="text-orange-400">0.92</span>,<br/>
      <span className="text-wm-blue">"location"</span>: <span className="text-wm-muted">{"{"}</span> <span className="text-wm-blue">"lat"</span>: <span className="text-orange-400">34.05</span>, <span className="text-wm-blue">"lng"</span>: <span className="text-orange-400">35.12</span> <span className="text-wm-muted">{"}"}</span><br/>
    <span className="text-wm-muted">{"}"}</span><br/>
  <span className="text-wm-muted">{"]"}</span><br/>
<span className="text-wm-muted">{"}"}</span>
            </code></pre>
          </div>
        </div>
      </div>

      <div className="order-1 lg:order-2">
        <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-wm-border bg-wm-card text-wm-muted text-xs font-mono mb-6">
          {t('apiSection.apiTier')}
        </div>
        <h2 className="text-3xl md:text-5xl font-display font-bold mb-6">{t('apiSection.title')}</h2>
        <p className="text-wm-muted mb-8">
          {t('apiSection.subtitle')}
        </p>
        <ul className="space-y-4 mb-8">
          <li className="flex items-start gap-3">
            <Server className="w-5 h-5 text-wm-muted shrink-0" aria-hidden="true" />
            <span className="text-sm">{t('apiSection.restApi')}</span>
          </li>
          <li className="flex items-start gap-3">
            <Lock className="w-5 h-5 text-wm-muted shrink-0" aria-hidden="true" />
            <span className="text-sm">{t('apiSection.authenticated')}</span>
          </li>
          <li className="flex items-start gap-3">
            <Database className="w-5 h-5 text-wm-muted shrink-0" aria-hidden="true" />
            <span className="text-sm">{t('apiSection.structured')}</span>
          </li>
        </ul>

        <div className="grid grid-cols-2 gap-4 mb-8 p-4 bg-wm-card border border-wm-border rounded-sm">
          <div>
            <p className="font-mono text-xs text-wm-muted uppercase tracking-widest mb-2">{t('apiSection.starter')}</p>
            <p className="text-sm font-bold">{t('apiSection.starterReqs')}</p>
            <p className="text-xs text-wm-muted">{t('apiSection.starterWebhooks')}</p>
          </div>
          <div>
            <p className="font-mono text-xs text-wm-muted uppercase tracking-widest mb-2">{t('apiSection.business')}</p>
            <p className="text-sm font-bold">{t('apiSection.businessReqs')}</p>
            <p className="text-xs text-wm-muted">{t('apiSection.businessWebhooks')}</p>
          </div>
        </div>

        <p className="text-sm text-wm-muted border-l-2 border-wm-border pl-4">
          {t('apiSection.feedData')}
        </p>
      </div>
    </div>
  </section>
);

const EnterpriseShowcase = () => (
  <section className="py-24 px-6" id="enterprise">
    <div className="max-w-7xl mx-auto">
      <div className="text-center mb-16">
        <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-wm-border bg-wm-card text-wm-muted text-xs font-mono mb-6">
          {t('enterpriseShowcase.enterpriseTier')}
        </div>
        <h2 className="text-3xl md:text-5xl font-display font-bold mb-6">{t('enterpriseShowcase.title')}</h2>
        <p className="text-wm-muted max-w-2xl mx-auto">
          {t('enterpriseShowcase.subtitle')}
        </p>
      </div>

      <div className="grid md:grid-cols-3 gap-6 mb-6">
        <div className="bg-wm-card border border-wm-border p-6">
          <ShieldAlert className="w-8 h-8 text-wm-muted mb-4" aria-hidden="true" />
          <h4 className="font-bold mb-2">{t('enterpriseShowcase.security')}</h4>
          <p className="text-sm text-wm-muted">{t('enterpriseShowcase.securityDesc')}</p>
        </div>
        <div className="bg-wm-card border border-wm-border p-6">
          <Cpu className="w-8 h-8 text-wm-muted mb-4" aria-hidden="true" />
          <h4 className="font-bold mb-2">{t('enterpriseShowcase.aiAgents')}</h4>
          <p className="text-sm text-wm-muted">{t('enterpriseShowcase.aiAgentsDesc')}</p>
        </div>
        <div className="bg-wm-card border border-wm-border p-6">
          <Layers className="w-8 h-8 text-wm-muted mb-4" aria-hidden="true" />
          <h4 className="font-bold mb-2">{t('enterpriseShowcase.dataLayers')}</h4>
          <p className="text-sm text-wm-muted">{t('enterpriseShowcase.dataLayersDesc')}</p>
        </div>
      </div>
      <div className="grid md:grid-cols-3 gap-6 mb-12">
        <div className="bg-wm-card border border-wm-border p-6">
          <Plug className="w-8 h-8 text-wm-muted mb-4" aria-hidden="true" />
          <h4 className="font-bold mb-2">{t('enterpriseShowcase.connectors')}</h4>
          <p className="text-sm text-wm-muted">{t('enterpriseShowcase.connectorsDesc')}</p>
        </div>
        <div className="bg-wm-card border border-wm-border p-6">
          <PanelTop className="w-8 h-8 text-wm-muted mb-4" aria-hidden="true" />
          <h4 className="font-bold mb-2">{t('enterpriseShowcase.whiteLabel')}</h4>
          <p className="text-sm text-wm-muted">{t('enterpriseShowcase.whiteLabelDesc')}</p>
        </div>
        <div className="bg-wm-card border border-wm-border p-6">
          <BarChart3 className="w-8 h-8 text-wm-muted mb-4" aria-hidden="true" />
          <h4 className="font-bold mb-2">{t('enterpriseShowcase.financial')}</h4>
          <p className="text-sm text-wm-muted">{t('enterpriseShowcase.financialDesc')}</p>
        </div>
      </div>

      <div className="data-grid">
        <div className="data-cell">
          <h5 className="font-mono text-xs text-wm-muted uppercase tracking-widest mb-2">{t('enterpriseShowcase.commodity')}</h5>
          <p className="text-sm">{t('enterpriseShowcase.commodityDesc')}</p>
        </div>
        <div className="data-cell">
          <h5 className="font-mono text-xs text-wm-muted uppercase tracking-widest mb-2">{t('enterpriseShowcase.government')}</h5>
          <p className="text-sm">{t('enterpriseShowcase.governmentDesc')}</p>
        </div>
        <div className="data-cell">
          <h5 className="font-mono text-xs text-wm-muted uppercase tracking-widest mb-2">{t('enterpriseShowcase.risk')}</h5>
          <p className="text-sm">{t('enterpriseShowcase.riskDesc')}</p>
        </div>
        <div className="data-cell">
          <h5 className="font-mono text-xs text-wm-muted uppercase tracking-widest mb-2">{t('enterpriseShowcase.soc')}</h5>
          <p className="text-sm">{t('enterpriseShowcase.socDesc')}</p>
        </div>
      </div>
    </div>
  </section>
);

const PricingTable = () => {
  const rows = [
    { feature: t('pricingTable.dataRefresh'), free: t('pricingTable.f5_15min'), pro: t('pricingTable.fLt60s'), api: t('pricingTable.fPerRequest'), ent: t('pricingTable.fLiveEdge') },
    { feature: t('pricingTable.dashboard'), free: t('pricingTable.f50panels'), pro: t('pricingTable.f50panels'), api: "\u2014", ent: t('pricingTable.fWhiteLabel') },
    { feature: t('pricingTable.ai'), free: t('pricingTable.fBYOK'), pro: t('pricingTable.fIncluded'), api: "\u2014", ent: t('pricingTable.fAgentsPersonas') },
    { feature: t('pricingTable.briefsAlerts'), free: "\u2014", pro: t('pricingTable.fDailyFlash'), api: "\u2014", ent: t('pricingTable.fTeamDist') },
    { feature: t('pricingTable.delivery'), free: "\u2014", pro: t('pricingTable.fSlackTgWa'), api: t('pricingTable.fWebhook'), ent: t('pricingTable.fSiemMcp') },
    { feature: t('pricingTable.apiRow'), free: "\u2014", pro: "\u2014", api: t('pricingTable.fRestWebhook'), ent: t('pricingTable.fMcpBulk') },
    { feature: t('pricingTable.infraLayers'), free: t('pricingTable.f45'), pro: t('pricingTable.f45'), api: "\u2014", ent: t('pricingTable.fTensOfThousands') },
    { feature: t('pricingTable.satellite'), free: "\u2014", pro: "\u2014", api: "\u2014", ent: t('pricingTable.fImagerySar') },
    { feature: t('pricingTable.connectorsRow'), free: "\u2014", pro: "\u2014", api: "\u2014", ent: t('pricingTable.f100plus') },
    { feature: t('pricingTable.deployment'), free: t('pricingTable.fCloud'), pro: t('pricingTable.fCloud'), api: t('pricingTable.fCloud'), ent: t('pricingTable.fCloudOnPrem') },
    { feature: t('pricingTable.securityRow'), free: t('pricingTable.fStandard'), pro: t('pricingTable.fStandard'), api: t('pricingTable.fKeyAuth'), ent: t('pricingTable.fSsoMfa') },
  ];

  return (
    <section className="py-24 px-6 max-w-7xl mx-auto">
      <div className="text-center mb-16">
        <h2 className="text-3xl md:text-5xl font-display font-bold mb-6">{t('pricingTable.title')}</h2>
      </div>
      <div className="hidden md:block">
        <div className="grid grid-cols-5 gap-4 mb-4 pb-4 border-b border-wm-border font-mono text-xs uppercase tracking-widest text-wm-muted">
          <div>{t('pricingTable.feature')}</div>
          <div>{t('pricingTable.freeHeader')}</div>
          <div className="text-wm-green">{t('pricingTable.proHeader')}</div>
          <div>{t('pricingTable.apiHeader')}</div>
          <div>{t('pricingTable.entHeader')}</div>
        </div>
        {rows.map((row, i) => (
          <div key={i} className="grid grid-cols-5 gap-4 py-4 border-b border-wm-border/50 text-sm hover:bg-wm-card/50 transition-colors">
            <div className="font-medium">{row.feature}</div>
            <div className="text-wm-muted">{row.free}</div>
            <div className="text-wm-green">{row.pro}</div>
            <div className="text-wm-muted">{row.api}</div>
            <div className="text-wm-muted">{row.ent}</div>
          </div>
        ))}
      </div>
      <div className="md:hidden space-y-4">
        {rows.map((row, i) => (
          <div key={i} className="bg-wm-card border border-wm-border p-4 rounded-sm">
            <p className="font-medium text-sm mb-3">{row.feature}</p>
            <div className="grid grid-cols-2 gap-2 text-xs">
              <div><span className="text-wm-muted">{t('tiers.free')}:</span> {row.free}</div>
              <div><span className="text-wm-green">{t('tiers.pro')}:</span> <span className="text-wm-green">{row.pro}</span></div>
              <div><span className="text-wm-muted">{t('nav.api')}:</span> {row.api}</div>
              <div><span className="text-wm-muted">{t('tiers.enterprise')}:</span> {row.ent}</div>
            </div>
          </div>
        ))}
      </div>
    </section>
  );
};

const FAQ = () => {
  const faqs = [
    { q: t('faq.q1'), a: t('faq.a1'), open: true },
    { q: t('faq.q2'), a: t('faq.a2') },
    { q: t('faq.q3'), a: t('faq.a3') },
    { q: t('faq.q4'), a: t('faq.a4') },
    { q: t('faq.q5'), a: t('faq.a5') },
    { q: t('faq.q6'), a: t('faq.a6') },
  ];

  return (
    <section className="py-24 px-6 max-w-3xl mx-auto">
      <h2 className="text-3xl font-display font-bold mb-12 text-center">{t('faq.title')}</h2>
      <div className="space-y-4">
        {faqs.map((faq, i) => (
          <details key={i} open={faq.open} className="group bg-wm-card border border-wm-border rounded-sm [&_summary::-webkit-details-marker]:hidden">
            <summary className="flex items-center justify-between p-6 cursor-pointer font-medium">
              {faq.q}
              <ChevronDown className="w-5 h-5 text-wm-muted group-open:rotate-180 transition-transform" aria-hidden="true" />
            </summary>
            <div className="px-6 pb-6 text-wm-muted text-sm border-t border-wm-border pt-4 mt-2">
              {faq.a}
            </div>
          </details>
        ))}
      </div>
    </section>
  );
};

const Footer = () => (
  <footer className="border-t border-wm-border bg-[#020202] pt-24 pb-12 px-6 text-center" id="waitlist">
    <div className="max-w-2xl mx-auto mb-16">
      <h2 className="text-4xl font-display font-bold mb-6">{t('footer.beFirstInLine')}</h2>
      <form className="flex flex-col gap-3 max-w-md mx-auto mb-6" onSubmit={(e) => { e.preventDefault(); const form = e.currentTarget; const email = new FormData(form).get('email') as string; submitWaitlist(email, form); }}>
        <input type="text" name="website" autoComplete="off" tabIndex={-1} aria-hidden="true" className="absolute opacity-0 h-0 w-0 pointer-events-none" />
        <div className="flex flex-col sm:flex-row gap-3">
          <input
            type="email"
            name="email"
            placeholder={t('hero.emailPlaceholder')}
            className="flex-1 bg-wm-card border border-wm-border rounded-sm px-4 py-3 text-sm focus:outline-none focus:border-wm-green transition-colors font-mono"
            required
            aria-label={t('hero.emailAriaLabel')}
          />
          <button type="submit" className="bg-wm-green text-wm-bg px-6 py-3 rounded-sm font-mono text-sm uppercase tracking-wider font-bold hover:bg-green-400 transition-colors whitespace-nowrap">
            {t('nav.joinWaitlist')}
          </button>
        </div>
        <div className="cf-turnstile mx-auto" data-sitekey={TURNSTILE_SITE_KEY} data-theme="dark" data-size="compact" />
      </form>
      <p className="text-sm text-wm-muted">
        {t('footer.lookingForEnterprise')} <a href="mailto:enterprise@worldmonitor.app" className="text-wm-text underline underline-offset-4 hover:text-wm-green transition-colors">{t('footer.contactUs')}</a>.
      </p>
    </div>

    <div className="flex flex-col md:flex-row items-center justify-between max-w-7xl mx-auto pt-8 border-t border-wm-border/50 text-xs text-wm-muted font-mono">
      <div className="flex items-center gap-4 mb-4 md:mb-0">
        <Logo />
      </div>
      <div className="flex gap-6">
        <a href="https://x.com/eliehabib" target="_blank" rel="noreferrer" className="hover:text-wm-text transition-colors">X</a>
        <a href="https://github.com/koala73/worldmonitor" target="_blank" rel="noreferrer" className="hover:text-wm-text transition-colors">GitHub</a>
        <a href="https://www.wired.me/story/the-music-streaming-ceo-who-built-a-global-war-map" target="_blank" rel="noreferrer" className="hover:text-wm-text transition-colors">{t('footer.wiredArticle')}</a>
      </div>
    </div>
  </footer>
);

export default function App() {
  return (
    <div className="min-h-screen selection:bg-wm-green/30 selection:text-wm-green">
      <Navbar />
      <main>
        <Hero />
        <LivePreview />
        <SourceMarquee />
        <SocialProof />
        <DataCoverage />
        <Tiers />
        <ProShowcase />
        <ApiSection />
        <EnterpriseShowcase />
        <PricingTable />
        <FAQ />
      </main>
      <Footer />
    </div>
  );
}
