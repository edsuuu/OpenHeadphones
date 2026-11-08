#!/usr/bin/env python3
"""Generates devices/<model>/ (profile.toml + photos) from the JBL Headphones app's own data.

    python3 tools/import_jbl_apk.py <JBL Headphones .apkm or base.apk>

Reads, from the APK: assets/product_list.json (every model: names, PIDs, photos, presets),
assets/config/dash4/<model>.json (which cards the JBL app shows for it), assets/standard_preset_eq.json
(the EQ presets) and the product photos (res/drawable-*/<photo>.webp, made PNG with macOS `sips`).

Only models with a dash4 screen config and on a "u3" protocol family this app speaks: the Tune Flex 2
(u3.protocol3) is what the app was written against, and a u3.bes model (JBL Wave Flex) answered all
of its commands. The photos are JBL's: they are generated here and kept out of git.

The rows of each card are the Tune Flex 2's (src: JBL Headphones 5.38.2, TuneFlex2Control); a model
whose card works differently needs the card's code read (jadx) and a row of its own here.
"""

import io
import json
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

FAMILIES = ("u3.protocol3", "u3.protocol4", "u3.bes")
# color id the device reports (0x12 packet) → photo suffix; from TuneFlex2Control's photos
COLORS = {0: "black", 1: "white", 2: "blue", 14: "ghost_black", 15: "ghost_purple", 16: "ghost_white"}
EFFECTS = {"NATURAL": "natural", "INCREASED TREBLE": "treble", "INCREASED BASS": "bass"}
# cards that are not settings rows: the sound tab, the tabs themselves, headers, phone-only screens
NOT_ROWS = {"ANC", "AA", "TT", "AscCard", "AscCardEarTips", "AdaptiveAncCard", "EQCard", "GestureCard",
            "FindMyBudCard", "SupportCard", "OTA", "UpdateAvailable", "PaddingView", "EntertainmentSettingsCard",
            "AudioChatSettingsCard", "SetupCard"}

ROOT = Path(__file__).resolve().parent.parent


def family(m):
    c = (m.get("cmdCtrlList") or [""])[0]
    return c.split(".imp.control.")[-1].rsplit(".", 1)[0]


def slug(name):
    return re.sub(r"[^a-z0-9]+", "-", name.lower().replace("+", " plus")).strip("-")


def toml_list(v):
    return "[" + ", ".join(f"{x:g}" if isinstance(x, float) else str(x) for x in v) + "]"


def setting_rows(m, cards, support, dash):
    """The settings tab: the Tune Flex 2's rows for the cards this model has, in its order."""
    has = cards.__contains__
    out = []
    add = out.append
    sav = m.get("smartAudioVideo")
    if has("SmartAudioVideoCard") and sav:
        add('    # Smart Audio & Video: AAC, SBC, latency presets (product_list.json "smartAudioVideo")')
        add('    { name = "smart_audio", section = "audio", kind = "choice", register = "smart_switch", width = 2, options = [')
        out += [f'        {{ name = "{k}", values = {toml_list(v)} }},' for k, v in sav.items()]
        add("    ] },")
    if has("Spatial3DCard"):
        add("    # [on, type]; picking a type turns it on, like the app")
        add('    { name = "spatial", section = "audio", kind = "switch", register = { feature = 0x0C } },')
        add('    { name = "spatial_mode", section = "audio", kind = "choice", register = { feature = 0x0C }, byte = 1, set = [[0, 1]], when = "spatial", options = [')
        add('        { name = "movie", values = [1] },')
        add('        { name = "music", values = [2] },')
        add('        { name = "game", values = [3] },')
        add("    ] },")
    if has("LeftRightBalanceCard"):
        add("    # [on, level]: 84…116, 100 = center")
        add('    { name = "balance", section = "audio", kind = "switch", register = "balance" },')
        add('    { name = "balance_level", section = "audio", kind = "level", register = "balance", byte = 1, min = 84, max = 116, center = 100, set = [[0, 1]], when = "balance" },')
    if has("SafeSoundCard"):
        add('    { name = "safe_sound", section = "audio", kind = "switch", register = { feature = 0x20 } },')
    if has("PersoniFiCard"):
        add("    # the hearing test itself runs in the phone app; here only on/off")
        add('    { name = "personi_fi", section = "audio", kind = "switch", register = "personi_fi" },')
    if has("SoundFitCard"):
        add("    # SoundFitCard: which ear tips are on")
        add('    { name = "tips", section = "audio", kind = "choice", register = "tips", options = [')
        add('        { name = "open", values = [0] },')
        add('        { name = "sealing", values = [1] },')
        add("    ] },")
    if has("VoiceAwareCard"):
        add("    # [on, level 1…3]")
        add('    { name = "voice_aware", section = "calls", kind = "switch", register = { feature = 0x0D } },')
        add('    { name = "voice_aware_level", section = "calls", kind = "choice", register = { feature = 0x0D }, byte = 1, when = "voice_aware", options = [')
        add('        { name = "low", values = [1] },')
        add('        { name = "mid", values = [2] },')
        add('        { name = "high", values = [3] },')
        add("    ] },")
    effects = m.get("mEffectList") or []
    if has("SoundEffectCard") and effects and all(e in EFFECTS for e in effects):
        add("    # sound effect [status, type]: the app always sends status 1 (product_list.json \"mEffectList\")")
        add('    { name = "caller_voice", section = "calls", kind = "choice", register = { feature = 0x14 }, byte = 1, set = [[0, 1]], options = [')
        out += [f'        {{ name = "{EFFECTS[e]}", values = [{i}] }},' for i, e in enumerate(effects)]
        add("    ] },")
    if has("VoiceSettingCard"):
        add("    # mic [on, gain, effect]: the app always sends on 1, gain 0xFF")
        add('    { name = "my_voice", section = "calls", kind = "choice", register = { feature = 0x16 }, byte = 2, set = [[0, 1], [1, 255]], options = [')
        add('        { name = "natural", values = [0] },')
        add('        { name = "powerful", values = [1] },')
        add('        { name = "bright", values = [2] },')
        add("    ] },")
    if has("AudioLimiterCard"):
        add('    { name = "audio_limiter", section = "calls", kind = "switch", register = { feature = 0x23 } },')
    if dash.get("isSupportAccidentalTouchPrevention"):
        add("    # accidental touch prevention (firmware 2.2.17 and up)")
        add('    { name = "touch_guard", section = "touch", kind = "switch", register = { key = 4 } },')
    if has("VoicePromptCard"):
        add('    { name = "voice_prompts", section = "device", kind = "switch", register = "voice_prompt" },')
    if has("PowerSavingCard") or has("AutoPowerOffCard"):
        add("    # [on, minutes u16 little-endian]")
        add('    { name = "auto_off", section = "device", kind = "switch", register = { feature = 0x0B } },')
        add('    { name = "auto_off_time", section = "device", kind = "choice", register = { feature = 0x0B }, byte = 1, width = 2, when = "auto_off", options = [')
        add('        { name = "min30", values = [30] },')
        add('        { name = "min60", values = [60] },')
        add('        { name = "min120", values = [120] },')
        add("    ] },")
    if has("PowerSavingCard"):
        add('    { name = "auto_standby", section = "device", kind = "switch", register = "standby" },')
    if "DashBoardPowerOff" in support:
        add('    { name = "power_off", section = "device", kind = "button", command = "power_off" },')
    if has("SoftwareCard"):
        add('    { name = "firmware", section = "about", kind = "info", register = { feature = 0x05 }, format = "version" },')
    if "SnNumber" in support:
        add('    { name = "serial", section = "about", kind = "info", register = "serial", format = "text" },')
    if has("ResetToFactoryRest"):
        add('    { name = "factory_reset", section = "about", kind = "button", command = "factory_reset" },')
    return out


ROWS_FROM = {"SmartAudioVideoCard", "Spatial3DCard", "LeftRightBalanceCard", "SafeSoundCard", "PersoniFiCard",
             "SoundFitCard", "VoiceAwareCard", "SoundEffectCard", "VoiceSettingCard", "AudioLimiterCard",
             "VoicePromptCard", "PowerSavingCard", "AutoPowerOffCard", "SoftwareCard", "ResetToFactoryRest"}

GESTURES = '''# Touch gestures and their actions: TuneFlex2Control's ids{unverified}; action id = GestureActionType ordinal.
gestures = [
    {{ id = 6, side = "left", kind = "tap" }},
    {{ id = 7, side = "left", kind = "double_tap" }},
    {{ id = 8, side = "left", kind = "triple_tap" }},
    {{ id = 12, side = "left", kind = "hold" }},
    {{ id = 9, side = "right", kind = "tap" }},
    {{ id = 10, side = "right", kind = "double_tap" }},
    {{ id = 11, side = "right", kind = "triple_tap" }},
    {{ id = 14, side = "right", kind = "hold" }},
]
actions = [
    {{ id = 0x00, name = "none" }},
    {{ id = 0x08, name = "play_pause" }},
    {{ id = 0x05, name = "next" }},
    {{ id = 0x06, name = "previous" }},
    {{ id = 0x01, name = "volume_up" }},
    {{ id = 0x02, name = "volume_down" }},
    {{ id = 0x0B, name = "anc_ambient" }},
    {{ id = 0x09, name = "anc_ambient_off" }},
    {{ id = 0x0C, name = "anc_off" }},
    {{ id = 0x0D, name = "ambient_off" }},
    {{ id = 0x04, name = "talk_thru" }},
    {{ id = 0xA1, name = "voice_assistant" }},
]
'''


def photo_to_png(apk, names, src, dest):
    """`src` (a .webp in the APK) → `dest` .png, with macOS `sips` (or `dwebp`); False if neither."""
    if dest.exists():
        return True
    path = next((f"res/{d}/{src}.webp" for d in ("drawable-xxxhdpi-v4", "drawable-xxhdpi-v4", "drawable") if f"res/{d}/{src}.webp" in names), None)
    if not path:
        return False
    with tempfile.TemporaryDirectory() as tmp:
        webp = Path(tmp) / "photo.webp"
        webp.write_bytes(apk.read(path))
        if shutil.which("sips"):
            cmd = ["sips", "-s", "format", "png", str(webp), "--out", str(dest)]
        elif shutil.which("dwebp"):
            cmd = ["dwebp", str(webp), "-o", str(dest)]
        else:
            return False
        return subprocess.run(cmd, capture_output=True).returncode == 0 and dest.exists()


def main(path):
    outer = zipfile.ZipFile(path)
    version = "?"
    if "base.apk" in outer.namelist():
        try:
            version = json.loads(outer.read("info.json")).get("versionName", "?")
        except (KeyError, ValueError):
            pass
        apk = zipfile.ZipFile(io.BytesIO(outer.read("base.apk")))
    else:
        apk = outer
    if version == "?":  # APKMirror's name: jbl.stc.com_5.38.2-260928180_…
        version = next(iter(re.findall(r"_(\d+(?:\.\d+)+)-", Path(path).name)), "?")
    names = set(apk.namelist())
    read = lambda p: json.loads(apk.read(p))
    products = read("assets/product_list.json")["defaults"]
    std = read("assets/standard_preset_eq.json")
    by_category = {p["category"]: p for p in std["preset"]}

    models, skipped = {}, []
    for m in products:
        dash_path = f"assets/config/dash4/{m.get('dash4Config')}"
        if family(m) not in FAMILIES or dash_path not in names:
            skipped.append(f"{m['name']} ({family(m) or 'no control'}{'' if dash_path in names else ', no dash4'})")
            continue
        models.setdefault(slug(m["name"]), []).append((m, read(dash_path)))

    labels = set()
    for folder, entries in sorted(models.items()):
        m, dash = entries[0]
        cards = [f["name"] for f in dash.get("uiFeatures", []) if f.get("visible", True)]
        support = {c for group in m.get("isSupportConfig", []) for c in group}
        pids = sorted({int(p, 16) for e, _ in entries for p in e["pidS"]})
        prefixes = list(dict.fromkeys([m["name"]] + [n.removesuffix("-LE") for e, _ in entries for n in e.get("nickName") or []]))
        modes = [mode for card, mode in (("ANC", "anc"), ("AA", "ambient"), ("TT", "talk_thru")) if card in cards]
        out_dir = ROOT / "devices" / folder
        out_dir.mkdir(parents=True, exist_ok=True)

        lines = [
            f"# Generated by tools/import_jbl_apk.py from JBL Headphones {version} ({m.get('dash4Config')},",
            f"# {m['cmdCtrlList'][0].rsplit('.', 1)[-1]}). Do not edit: change the importer and run it again.",
            "",
            f'name = "{m["name"]}"',
            f"product_ids = [{', '.join(f'0x{p:04x}' for p in pids)}]",
            f"name_prefixes = [{', '.join(json.dumps(p) for p in prefixes)}]",
        ]
        if modes:
            lines.append(f"noise_modes = [{', '.join(json.dumps(x) for x in modes + ['off'])}]")
        if "AdaptiveAncCard" in cards:
            lines.append("adaptive_anc = true")
        if "AscCardEarTips" in cards:
            lines += ["# AscCardEarTips: 6 ANC levels with open tips, 7 sealed; the device takes 7 either way",
                      "anc_levels = 7", "ambient_levels = 7"]
        if "FindMyBudCard" in cards:
            lines.append("find_buds = true")
        if "GestureCard" in cards and m.get("customizedGesture"):
            unverified = "" if m["name"] == "JBL Tune Flex 2" else " (not checked on this model)"
            lines += ["", GESTURES.format(unverified=unverified).rstrip("\n")]
        rows = setting_rows(m, cards, support, dash)
        if rows:
            lines += ["", "# The settings tab, card by card in the JBL app's order (see the Tune Flex 2's rows).", "settings = ["] + rows + ["]"]
        todo = [c for c in cards if c not in NOT_ROWS and c not in ROWS_FROM]
        if todo:
            lines.append(f"# JBL cards this app has no row for yet: {', '.join(todo)}")

        # photos: the default one, then one per color the device can report
        default = (m.get("deviceImg") or [None])[0]
        if default:
            base = re.sub(r"_(" + "|".join(sorted(COLORS.values(), key=len, reverse=True)) + r"|pink)$", "", default)
            colors = {cid: f"{c.replace('_', '-')}.png" for cid, c in COLORS.items()
                      if photo_to_png(apk, names, f"{base}_{c}", out_dir / f"{c.replace('_', '-')}.png")}
            default_png = default.removeprefix(base + "_").replace("_", "-") + ".png"
            if photo_to_png(apk, names, default, out_dir / default_png):
                lines += ["", "[images]", f'default = "{default_png}"']
                if colors:
                    lines.append("colors = { " + ", ".join(f'{k} = "{v}"' for k, v in sorted(colors.items())) + " }")

        group = std["group"].get(m.get("serialName", "").lower(), [])
        presets = [by_category[c] for c in group if c in by_category]
        if "EQCard" in cards:
            fs = presets[0]["payload"]["fs"] if presets else [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]
            lines += ["", "[eq]", f"max_db = {max(abs(x) for x in m.get('eqGainRange', [6])):.1f}", f"bands = {toml_list(fs)}"]
            if presets:
                lines.append(f'# standard_preset_eq.json, group "{m["serialName"].lower()}"; category = what the device gets')
                lines.append("presets = [")
                for p in presets:
                    key = slug(p["name"]).replace("-", "_")
                    labels.add((key, p["name"]))
                    lines.append(f'    {{ name = "{key}", category = {p["category"]}, gains = {toml_list(p["payload"]["gain"])} }},')
                lines.append("]")
        (out_dir / "profile.toml").write_text("\n".join(lines) + "\n")

    print(f"{len(models)} models written to devices/: {', '.join(sorted(models))}")
    print(f"{len(skipped)} skipped (protocol not spoken here, or no dash4 screen config)")
    print("EQ preset labels the locales need (eq.<key>):", ", ".join(f"{k} = {n!r}" for k, n in sorted(labels)))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
