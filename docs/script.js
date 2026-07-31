// Translations dictionary for English and Turkish
const translations = {
  en: {
    "nav.features": "Features",
    "nav.dashboard": "Web Dashboard",
    "nav.quickstart": "Quick Start",
    "nav.docs": "Documentation",
    "nav.download": "Download",

    "hero.aiTag": "Antigravity & Gemini & DeepSeek AI Project",
    "hero.subtitle": "Multi-account SIP client with a built-in modern Web Dashboard, REST API, IVR Engine, Rhai & Lua plugin subsystem written in pure async Rust.",
    "hero.btnGetStarted": "Get Started",
    "hero.btnViewSource": "View Source",

    "features.titlePre": "Why",
    "features.subtitle": "A fast, reliable, and lightweight Telecom solution built in pure Rust without heavy frameworks or JVM dependencies.",
    "features.f1.title": "Multi-Account Support",
    "features.f1.desc": "Manage multiple SIP accounts simultaneously with independent registration (REGISTER) and call state machines.",
    "features.f2.title": "Visual IVR Builder",
    "features.f2.desc": "Graphically construct welcome prompts, timeout rules, and DTMF menu actions with ease.",
    "features.f3.title": "Web Softphone & Audio",
    "features.f3.desc": "Make calls, talk, and record microphone audio straight from your browser using WebSockets and Web Audio API.",
    "features.f4.title": "Rhai & Lua Scripting",
    "features.f4.desc": "Customize inbound and outbound call routing logic dynamically with embedded Rhai and Lua engines.",
    "features.f5.title": "Syslog (RFC 5424) & Tracer",
    "features.f5.desc": "Inspect SIP signaling flow sequence diagrams and forward real-time logs to central Syslog daemons.",
    "features.f6.title": "Single Binary & Pi Support",
    "features.f6.desc": "Lightweight ~2-3 MB executable. Runs seamlessly on Raspberry Pi and Headless Linux with zero external dependencies.",

    "demo.titlePre": "Advanced",
    "demo.subtitle": "Control SIP registrations, call logs, live system metrics, and Syslog settings directly from your browser.",
    "demo.menu.dashboard": "Dashboard",
    "demo.menu.accounts": "SIP Accounts",
    "demo.menu.ivr": "IVR Engine",
    "demo.menu.softphone": "Softphone",
    "demo.menu.logs": "Logs & Syslog",
    "demo.card.activeCalls": "Active Calls",
    "demo.card.registered": "Registered Accounts",
    "demo.card.resources": "CPU / RAM Usage",

    "quickstart.titlePre": "Quick",
    "quickstart.subtitle": "Download and run rsipclient in seconds with a single command.",
    "quickstart.buildCargo": "Build with Cargo",

    "docs.titlePre": "Comprehensive",
    "docs.subtitle": "Explore architecture details, JSON TCP IPC control protocol, and Raspberry Pi guides.",
    "docs.d1.title": "Configuration Guide",
    "docs.d1.desc": "Detailed config.toml parameters, SIP account definitions, TLS, and Syslog options.",
    "docs.d2.title": "JSON TCP IPC Protocol",
    "docs.d2.desc": "Control calls and IVR menus via TCP in JSON format from any language (Python, Node.js, Go).",
    "docs.d3.title": "Raspberry Pi & Headless Linux",
    "docs.d3.desc": "ALSA sound card configuration, systemd service setup, and headless console operation guide.",

    "footer.aiDesc": "Experimental AI-Generated R&D Project by Antigravity, Gemini & DeepSeek.",
    "footer.license": "Open Source under MIT License. Released on GitHub."
  },

  tr: {
    "nav.features": "Özellikler",
    "nav.dashboard": "Web Dashboard",
    "nav.quickstart": "Hızlı Başlangıç",
    "nav.docs": "Dokümantasyon",
    "nav.download": "İndir",

    "hero.aiTag": "Antigravity & Gemini & DeepSeek AI Projesi",
    "hero.subtitle": "Çoklu hesap desteği, dahili modern Web Dashboard, REST API, IVR Motoru, Rhai/Lua eklenti sistemi ve yüksek performanslı saf Rust mimarisi.",
    "hero.btnGetStarted": "Hemen Başlayın",
    "hero.btnViewSource": "Kaynak Kodu Gör",

    "features.titlePre": "Neden",
    "features.subtitle": "Ağır framework'ler ve JVM bağımlılıkları olmadan saf Rust ile yazılmış hızlı, güvenilir ve esnek Telekomünikasyon çözümü.",
    "features.f1.title": "Çoklu Hesap Desteği",
    "features.f1.desc": "Aynı anda birden fazla SIP hesabını yönetin, bağımsız kayıt (REGISTER) ve çağrı akışlarını kontrol edin.",
    "features.f2.title": "Görsel IVR Builder",
    "features.f2.desc": "Karşılama sesleri, zaman aşımı kuralları ve DTMF tuş menü eylemlerini görsel olarak kolayca yapılandırın.",
    "features.f3.title": "Web Softphone & Audio",
    "features.f3.desc": "WebSocket ve Web Audio API ile doğrudan tarayıcınızdan arama yapın, konuşun ve mikrofon sesinizi kaydedin.",
    "features.f4.title": "Rhai & Lua Eklentileri",
    "features.f4.desc": "Dahili Rhai ve Lua betik desteği sayesinde gelen ve giden çağrı senaryolarını anında özelleştirin.",
    "features.f5.title": "Syslog (RFC 5424) & Tracer",
    "features.f5.desc": "SIP paket izleyici ve RFC 5424 Syslog desteği ile canlı logları merkezi log sunucularına aktarın.",
    "features.f6.title": "Tek Binary & Pi Desteği",
    "features.f6.desc": "Sadece ~2-3 MB büyüklüğünde tek bir executable binary. Raspberry Pi ve Headless Linux sistemlerde sıfır bağımlılıkla çalışır.",

    "demo.titlePre": "Gelişmiş",
    "demo.subtitle": "Web arayüzünden SIP hesaplarınızı, çağrı kayıtlarınızı, canlı sistem istatistiklerini ve Syslog ayarlarını yönetin.",
    "demo.menu.dashboard": "Dashboard",
    "demo.menu.accounts": "SIP Hesapları",
    "demo.menu.ivr": "IVR Motoru",
    "demo.menu.softphone": "Softphone",
    "demo.menu.logs": "Loglar & Syslog",
    "demo.card.activeCalls": "Aktif Çağrılar",
    "demo.card.registered": "Kayıtlı SIP Hesapları",
    "demo.card.resources": "CPU / RAM Kullanımı",

    "quickstart.titlePre": "Hızlı",
    "quickstart.subtitle": "Saniyeler içinde rsipclient uygulamasını indirin ve tek komutla çalıştırın.",
    "quickstart.buildCargo": "Cargo ile Derle",

    "docs.titlePre": "Kapsamlı",
    "docs.subtitle": "Sistem mimarisi, TCP JSON kontrol protokolü ve Raspberry Pi rehberlerine göz atın.",
    "docs.d1.title": "Konfigürasyon Rehberi",
    "docs.d1.desc": "config.toml detaylı parametreleri, SIP hesap tanımlamaları ve TLS/Syslog seçenekleri.",
    "docs.d2.title": "JSON TCP IPC Protokolü",
    "docs.d2.desc": "Herhangi bir dilden (Python, Node.js, Go) JSON formatında TCP üzerinden arama yapma ve IVR yönetimi.",
    "docs.d3.title": "Raspberry Pi & Headless Linux",
    "docs.d3.desc": "ALSA ses kartı sürücüleri, systemd servis entegrasyonu ve konsol modunda çalıştırma kılavuzu.",

    "footer.aiDesc": "Antigravity, Gemini ve DeepSeek AI tarafından geliştirilen deneysel Ar-Ge projesi.",
    "footer.license": "MIT Lisansı ile Açık Kaynak. GitHub üzerinde yayınlandı."
  }
};

// Set Language Function
function setLanguage(lang) {
  if (!translations[lang]) return;

  // Store in localStorage if available
  try {
    localStorage.setItem('rsip_lang', lang);
  } catch (e) {}

  // Update HTML lang attribute
  document.documentElement.lang = lang;

  // Update language buttons
  const btnEn = document.getElementById('lang-en');
  const btnTr = document.getElementById('lang-tr');
  if (btnEn && btnTr) {
    btnEn.classList.toggle('active', lang === 'en');
    btnTr.classList.toggle('active', lang === 'tr');
  }

  // Update all elements with data-i18n attribute
  document.querySelectorAll('[data-i18n]').forEach(el => {
    const key = el.getAttribute('data-i18n');
    if (translations[lang][key]) {
      el.textContent = translations[lang][key];
    }
  });
}

// Tab Switcher Functionality for Installation Instructions
function switchTab(tabId) {
  const contents = document.querySelectorAll('.tab-content');
  contents.forEach(content => {
    content.classList.remove('active');
  });

  const buttons = document.querySelectorAll('.tab-btn');
  buttons.forEach(btn => {
    btn.classList.remove('active');
  });

  const targetTab = document.getElementById(tabId);
  if (targetTab) {
    targetTab.classList.add('active');
  }

  const activeBtn = Array.from(buttons).find(btn => 
    btn.getAttribute('onclick') && btn.getAttribute('onclick').includes(tabId)
  );
  if (activeBtn) {
    activeBtn.classList.add('active');
  }
}

// Initialize on DOM load
document.addEventListener('DOMContentLoaded', () => {
  // Check saved language or default to English ('en')
  let savedLang = 'en';
  try {
    const stored = localStorage.getItem('rsip_lang');
    if (stored === 'tr' || stored === 'en') {
      savedLang = stored;
    }
  } catch (e) {}

  setLanguage(savedLang);
});

// Smooth scroll for anchor links
document.querySelectorAll('a[href^="#"]').forEach(anchor => {
  anchor.addEventListener('click', function (e) {
    e.preventDefault();
    const target = document.querySelector(this.getAttribute('href'));
    if (target) {
      target.scrollIntoView({
        behavior: 'smooth',
        block: 'start'
      });
    }
  });
});
