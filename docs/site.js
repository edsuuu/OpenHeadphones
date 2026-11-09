// The page's behavior, in one class: the language, the download for this system, the demo video,
// the copy button and the equalizer the hero stands on. Loaded in <head>, so the language is set
// before the first paint; the rest starts once the page is there.
class OpenHeadphonesPage {
  // ---- data ----

  // JBL's presets for the Tune Flex 2 (devices/jbl-tune-flex-2/profile.toml), dB per band
  static presets = {
    "Studio": [-2, -1, -1, -0.5, 0, 0, 0, 0, 0, 0],
    "Bass": [4, 4, 3, 2.5, 0.5, 0, 0, 0, 0, 0],
    "Club": [2, 1.5, 1, 0, 0, 0, 0, 1, 1, 2],
    "Extreme Bass": [4, 4.5, 4.5, 3.5, -1, 0, 0, 2, 2, 0],
    "Vocal": [-1.5, -3, -3, 1, 3.5, 3.5, 3, 1.5, 0, -1.5],
    "Jazz": [4, 3, 1, 2.5, -1.5, -1.5, 0, 1, 3, 4],
  };
  static firstPreset = "Extreme Bass";
  static ptTitle = "OpenHeadphones: app de código aberto para fones JBL no Windows, macOS e Linux";
  static presetClass =
    "cursor-pointer rounded-full border-[1.5px] border-transparent px-[13px] py-[5px] text-base font-semibold text-ink hover:border-rule aria-pressed:bg-ink aria-pressed:text-foam";
  static knobClass = "absolute -mt-1.5 -ml-1.5 size-3 rounded-full border-2 border-ink bg-foam";
  static morphMs = 520;

  // ---- state ----

  calm = matchMedia("(prefers-reduced-motion: reduce)").matches;
  enTitle = document.title;
  gains = Array(10).fill(0);
  eq = {};

  // ---- start ----

  static start() {
    OpenHeadphonesPage.applyLanguage(OpenHeadphonesPage.savedLanguage());
    document.addEventListener("DOMContentLoaded", () => new OpenHeadphonesPage());
  }

  constructor() {
    this.setUpLanguage();
    this.highlightDownload();
    this.setUpVideo();
    this.setUpCopy();
    this.setUpEqualizer();
  }

  // ---- language: the page carries English and Portuguese, one is shown ----

  static savedLanguage() {
    try {
      const saved = localStorage.getItem("lang");
      if (saved) return saved;
    } catch (e) {}
    return /^pt\b/i.test(navigator.language) ? "pt" : "en";
  }

  static applyLanguage(lang) {
    document.documentElement.dataset.lang = lang;
    document.documentElement.lang = lang === "pt" ? "pt-BR" : "en";
  }

  setUpLanguage() {
    this.langButtons = [...document.querySelectorAll("[data-set-lang]")];
    this.langButtons.forEach((b) => b.addEventListener("click", () => this.setLanguage(b.dataset.setLang)));
    this.setLanguage(document.documentElement.dataset.lang);
  }

  setLanguage(lang) {
    OpenHeadphonesPage.applyLanguage(lang);
    document.title = lang === "pt" ? OpenHeadphonesPage.ptTitle : this.enTitle;
    this.langButtons.forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.setLang === lang)));
    try { localStorage.setItem("lang", lang); } catch (e) {}
  }

  // ---- download: this system's row highlighted, first, and behind the big button ----

  static detectSystem() {
    const ua = navigator.userAgent.toLowerCase();
    const platform = ((navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "").toLowerCase();
    if (/android|iphone|ipad/.test(ua)) return null; // phones: Android says Linux
    if (platform.includes("mac")) return "mac";
    if (platform.includes("win")) return "windows";
    if (platform.includes("linux")) return "linux";
    return null;
  }

  static both(en, pt) {
    return `<span lang="en">${en}</span><span lang="pt-BR">${pt}</span>`;
  }

  highlightDownload() {
    const os = OpenHeadphonesPage.detectSystem();
    const row = os && document.querySelector(`[data-os="${os}"]`);
    if (!row) return;
    row.classList.add("mine");
    row.parentNode.prepend(row);
    const name = row.querySelector("[data-os-name]").textContent;
    const get = document.getElementById("get");
    get.innerHTML = OpenHeadphonesPage.both(`Download for ${name}`, `Baixar para ${name}`);
    // Linux has several packages: the button goes to the list
    if (os !== "linux") get.href = row.querySelector("a").href;
    document.getElementById("others").innerHTML = OpenHeadphonesPage.both("Other systems", "Outros sistemas");
  }

  // ---- video: still for reduced motion ----

  setUpVideo() {
    if (!this.calm) return;
    const video = document.getElementById("demo");
    video.removeAttribute("autoplay");
    video.pause();
    video.controls = true;
  }

  // ---- copy the install command ----

  setUpCopy() {
    const button = document.getElementById("copy");
    const command = document.getElementById("cmd");
    const label = button.innerHTML;
    button.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(command.textContent);
        button.innerHTML = OpenHeadphonesPage.both("Copied", "Copiado");
      } catch (e) {
        getSelection().selectAllChildren(command);
        button.innerHTML = OpenHeadphonesPage.both("Selected", "Selecionado");
      }
      setTimeout(() => (button.innerHTML = label), 1800);
    });
  }

  // ---- the equalizer: the curve under the hero and the preset buttons ----

  setUpEqualizer() {
    const box = document.getElementById("eq");
    this.eq.fill = box.querySelector('[data-eq="fill"]');
    this.eq.line = box.querySelector('[data-eq="line"]');
    this.eq.knobs = this.gains.map(() =>
      box.appendChild(Object.assign(document.createElement("span"), { className: OpenHeadphonesPage.knobClass })));
    this.eq.group = document.getElementById("presets");
    for (const name of Object.keys(OpenHeadphonesPage.presets)) {
      const b = Object.assign(document.createElement("button"), { type: "button", textContent: name, className: OpenHeadphonesPage.presetClass });
      b.setAttribute("aria-pressed", "false");
      b.addEventListener("click", () => this.pickPreset(name));
      this.eq.group.appendChild(b);
    }
    this.drawCurve(this.gains);
    this.pickPreset(OpenHeadphonesPage.firstPreset);
  }

  // band centers and gains in the 1000 × 200 view box: ±6 dB, up = louder
  static bandX(i) { return 50 + i * 100; }
  static gainY(g) { return 100 - (g / 6) * 72; }

  drawCurve(gains) {
    const p = gains.map((g, i) => [OpenHeadphonesPage.bandX(i), OpenHeadphonesPage.gainY(g)]);
    // Catmull-Rom through the bands, as the app draws its curve
    let d = `M0 ${p[0][1]} L${p[0][0]} ${p[0][1]}`;
    for (let i = 0; i < p.length - 1; i++) {
      const [p0, p1, p2, p3] = [p[Math.max(i - 1, 0)], p[i], p[i + 1], p[Math.min(i + 2, p.length - 1)]];
      d += ` C${p1[0] + (p2[0] - p0[0]) / 6} ${p1[1] + (p2[1] - p0[1]) / 6} ${p2[0] - (p3[0] - p1[0]) / 6} ${p2[1] - (p3[1] - p1[1]) / 6} ${p2[0]} ${p2[1]}`;
    }
    d += ` L1000 ${p[p.length - 1][1]}`;
    this.eq.line.setAttribute("d", d);
    this.eq.fill.setAttribute("d", `${d} L1000 0 L0 0 Z`);
    this.eq.knobs.forEach((k, i) => {
      k.style.left = `${p[i][0] / 10}%`;
      k.style.top = `${(p[i][1] / 200) * 100}%`;
    });
  }

  pickPreset(name) {
    this.eq.group.querySelectorAll("button").forEach((b) => b.setAttribute("aria-pressed", String(b.textContent === name)));
    this.morphTo(OpenHeadphonesPage.presets[name]);
  }

  // eases from the current curve to `target` (at once with reduced motion)
  morphTo(target) {
    const from = this.gains.slice();
    const start = performance.now();
    const ms = this.calm ? 0 : OpenHeadphonesPage.morphMs;
    const step = (now) => {
      const t = ms ? Math.min((now - start) / ms, 1) : 1;
      const e = 1 - Math.pow(1 - t, 3);
      this.gains = from.map((v, i) => v + (target[i] - v) * e);
      this.drawCurve(this.gains);
      if (t < 1) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }
}

OpenHeadphonesPage.start();
