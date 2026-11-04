#!/usr/bin/env python3
"""
publish_to_gumroad.py
Creates the official PhotonForge v1.0.0 multi-platform product on Gumroad with:
- macOS Universal 2 (.dmg)
- Windows x64 (.zip)
- Linux x86_64 (.tar.gz)
- Cover image & 1000x1000 square thumbnail
- Rich HTML sales copy, quick start guide, system requirements & installation instructions
"""

import os
import sys
import json
import datetime
import subprocess
import argparse

PROJECT_DIR = "/Volumes/Dataa/Ohjelmointi/ProjektitRust/photon_forge"
DIST_DIR = os.path.join(PROJECT_DIR, "dist")
ASSETS_DIR = os.path.join(DIST_DIR, "gumroad_assets")
TRACKER_FILE = "/Volumes/Dataa/EtsyTuotteet/gumroad_published.json"

PRODUCT_NAME = "PhotonForge – Smart Telescope Control & 32-bit Astrophotography Suite (macOS, Windows, Linux)"
CUSTOM_PERMALINK = "photonforge"
CURRENCY = "eur"
PRICE = "0"  # Pay What You Want (0€+)
SUGGESTED_PRICE = "5"  # Coffee-cup suggested price (5€)

TAGS = [
    "astrophotography",
    "seestar",
    "zwo seestar",
    "dwarf 3",
    "dwarflab",
    "smart telescope",
    "ascom",
    "fits",
    "xisf",
    "image stacking",
    "astronomy",
    "macos",
    "windows",
    "linux"
]

SUMMARY = "High-performance 32-bit astrophotography processing, sub-pixel star alignment, 2x Drizzle, mosaic stitching & direct Smart Telescope control (ZWO Seestar S50/S50 Pro/S30/S30 Pro, DwarfLab DWARF 3/II, Vespera, Origin, ASCOM Alpaca) with 360° Sky Atlas and All-Night Sequencer for macOS, Windows and Linux."

DESCRIPTION_HTML = """
<h2>🔭 Meet PhotonForge v1.0.0: 32-Bit Astrophotography &amp; Smart Telescope Desktop Suite</h2>
<p>
<strong>PhotonForge</strong> is a modern, blazing-fast 32-bit astrophotography post-processing suite and direct <strong>Smart Telescope &amp; Mount</strong> desktop command station written in 100% pure <strong>Rust</strong>. Built for deep-sky astrophotographers and smart telescope owners, PhotonForge combines real-time telescope control, an interactive 360° Sky Atlas, an automated all-night multi-target sequencer, and PixInsight/Siril-class 32-bit floating-point calibration, registration, stacking, mosaic stitching, and post-processing in a single ~10 MB native application — with <strong>no subscriptions and no bloat</strong>.
</p>
<p>
<strong>Free / Pay What You Want (0 €+):</strong> Download and use PhotonForge completely free, or support independent development with the price of a cup of coffee (3 € – 5 €)!<br>
<strong>All 3 Desktop Platforms Included:</strong> macOS (Universal 2 for Apple Silicon &amp; Intel), Windows 10/11 (64-bit), and Linux (x86_64).
</p>

<hr>

<h3>⚡ Key Features in v1.0.0</h3>
<ul>
  <li><strong>📡 Comprehensive Smart Telescope &amp; Mount Support:</strong>
    <ul>
      <li><strong>ZWO Seestar Family (S50, S50 Pro 4K, S30, S30 Pro 4K):</strong> Direct Wi-Fi TCP/JSON-RPC control, telemetry, slewing joystick, autofocus, dual-band filter toggle, and SMB sub-frame synchronization.</li>
      <li><strong>DwarfLab Family (DWARF 3 4K &amp; DWARF II):</strong> Open API control, telemetry, autofocus, and live image ingestion.</li>
      <li><strong>Vaonis Family (Vespera II 4K &amp; Vespera Pro 12.5 MP):</strong> Dedicated optics &amp; sensor FOV profiles with automated folder/FTP ingest live stacking.</li>
      <li><strong>Celestron Origin RASA:</strong> Native 335mm f/2.2 widefield FOV profiling and plate-solved mosaic planning.</li>
      <li><strong>ASCOM Alpaca (Universal REST):</strong> Cross-platform network control for Sky-Watcher, ZWO AM3/AM5, iOptron, OnStep, and custom telescope rigs.</li>
    </ul>
  </li>
  <li><strong>🌌 Interactive 360° Sky Atlas &amp; Tonight's Best Targets:</strong> Real-time stereographic sky dome with horizon obstructions, meridian line, and real-time altitude curves for 110+ Messier (M1–M110) and NGC/IC deep-sky objects.</li>
  <li><strong>🗓️ Automated All-Night Multi-Target Sequencer &amp; Visual Mosaic Planner:</strong> Queue multiple deep-sky targets for unattended overnight imaging with automatic GoTo, plate-solve verification, LP/IRCUT filter selection, autofocus, and exposure time allocation. Design custom <strong>N×M multi-panel mosaics</strong> visually on the sky map with adjustable overlap %.</li>
  <li><strong>🔴 Real-Time EAA Live Stacking &amp; Automatic SMB Sub-Frame Puller:</strong> Watch your deep-sky image build on your desktop screen as new sub-exposures arrive from the telescope, with real-time star registration and Auto-STF preview.</li>
  <li><strong>📚 Full 32-Bit Calibration, Sub-Pixel Star Alignment &amp; 2x Drizzle Stacking:</strong> Native FITS (16/32-bit) and PixInsight XISF (LZ4/Zlib) reader/writer. Automatic Master Dark/Flat/Bias calibration, Bayer demosaicing, Hot &amp; Cold Pixel Map, Sub-Pixel Similarity/Affine Star Alignment (automatically corrects Alt-Az field rotation!), Winsorized Sigma Clipping, FWHM/Roundness quality weighting, and <strong>2x Super-Resolution Drizzle</strong>.</li>
  <li><strong>🧩 Multi-Panel Seamless Mosaic Stitcher:</strong> Automatically stitches overlapping panels or multi-night sessions with feather blending and linear background matching.</li>
  <li><strong>🎨 Non-Destructive 32-Bit Post-Processing Pipeline:</strong>
    <ul>
      <li><strong>Auto-STF (Screen Transfer Function)</strong> &amp; <strong>Asinh / GHS</strong> (Generalized Hyperbolic Stretch)</li>
      <li><strong>ABE (Automatic Background Extraction)</strong> 2D polynomial light-pollution gradient removal</li>
      <li><strong>SPCC / PCC Photometric Color Calibration</strong> + <strong>SCNR</strong> green noise removal</li>
      <li><strong>WCS Plate Solver &amp; Sky Overlay</strong> (RA/Dec grid, Messier/NGC/IC markers, and star catalog crosshairs)</li>
      <li><strong>Narrowband Palette Combiner (SHO Hubble / HOO / LRGB)</strong></li>
      <li><strong>Interactive PCHIP Spline Tone Curves</strong> &amp; 3-Zone Shadow/Midtone/Highlight Color Wheels</li>
      <li><strong>Richardson-Lucy Deconvolution</strong> (automatic Moffat PSF star sharpening)</li>
      <li><strong>Wavelet Starless Separation</strong> &amp; <strong>Optional SyQon AI Neural Models</strong> (<code>AxiomV2</code>, <code>StarLess</code>, <code>Nadir</code>, <code>Prism</code>)</li>
      <li><strong>Batch Directory Processor</strong> &amp; <strong>Project Save/Load (<code>.pforge</code>)</strong></li>
    </ul>
  </li>
  <li><strong>🌍 Bilingual Interface (🇬🇧 English / 🇫🇮 Suomi):</strong> Instant 1-click language switch in the top toolbar.</li>
</ul>

<hr>

<h3>📦 What’s Included in Your Download</h3>
<p>When you download PhotonForge, you receive immediate access to all three platform builds:</p>
<ol>
  <li><strong>macOS Universal 2 Installer (<code>PhotonForge-1.0.0-universal.dmg</code>):</strong> Native performance on all Macs — Apple Silicon (M1/M2/M3/M4) and Intel (x86_64). Code-signed with Apple Developer certificate.</li>
  <li><strong>Windows 64-bit Standalone (<code>PhotonForge-1.0.0-windows-x64.zip</code>):</strong> Portable standalone executable for Windows 10 &amp; Windows 11. No installation bloat required.</li>
  <li><strong>Linux 64-bit Package (<code>PhotonForge-1.0.0-linux-x86_64.tar.gz</code>):</strong> Native 64-bit ELF binary, system icon, and <code>.desktop</code> launcher for Ubuntu, Debian, Fedora, Arch, and Linux Mint.</li>
</ol>

<hr>

<h3>🛠️ Installation Instructions</h3>

<h4>🍎 macOS:</h4>
<p>
1. Open <code>PhotonForge-1.0.0-universal.dmg</code> and drag <strong>PhotonForge</strong> into your <em>Applications</em> folder.<br>
2. <strong>First Launch (Gatekeeper Security):</strong> Right-click (or Control-click) <code>PhotonForge.app</code> in Applications and select <strong>Open</strong>, or allow it in <strong>System Settings → Privacy &amp; Security → Open Anyway</strong>.
</p>

<h4>🪟 Windows:</h4>
<p>
1. Extract <code>PhotonForge-1.0.0-windows-x64.zip</code> into any folder and double-click <code>photon_forge.exe</code>.<br>
2. If Windows SmartScreen appears on first launch, click <strong>"More info" → "Run anyway"</strong>.
</p>

<h4>🐧 Linux:</h4>
<p>
1. Extract <code>PhotonForge-1.0.0-linux-x86_64.tar.gz</code> and run <code>./photon_forge</code>.
</p>

<hr>

<p><em>PhotonForge v1.0.0. Developed by Niko Pietinen. Open-source (GPLv3) with free updates included.</em></p>
""".strip()


def main():
    parser = argparse.ArgumentParser(description="Publish or draft PhotonForge v1.0.0 on Gumroad")
    parser.add_argument("--draft", action="store_true", help="Create as draft without publishing immediately")
    parser.add_argument("--dry-run", action="store_true", help="Print command without executing")
    parser.add_argument("--price", default=PRICE, help="Price in EUR (default: 0 for PWYW)")
    parser.add_argument("--suggested-price", default=SUGGESTED_PRICE, help="Suggested PWYW price in EUR (default: 5)")
    args = parser.parse_args()

    mac_dmg = os.path.join(DIST_DIR, "PhotonForge-1.0.0-universal.dmg")
    win_zip = os.path.join(DIST_DIR, "PhotonForge-1.0.0-windows-x64.zip")
    lin_tar = os.path.join(DIST_DIR, "PhotonForge-1.0.0-linux-x86_64.tar.gz")

    cover_img = os.path.join(ASSETS_DIR, "01_PhotonForge_Hero_Cover.jpg")
    thumb_img = os.path.join(ASSETS_DIR, "gumroad_thumb_square.jpg")

    for f in [mac_dmg, win_zip, lin_tar, cover_img, thumb_img]:
        if not os.path.exists(f):
            print(f"Error: Required file not found: {f}", file=sys.stderr)
            sys.exit(1)

    cmd = [
        "gumroad", "products", "create",
        "--name", PRODUCT_NAME,
        "--price", args.price,
        "--currency", CURRENCY,
        "--custom-permalink", CUSTOM_PERMALINK,
        "--custom-summary", SUMMARY,
        "--description", DESCRIPTION_HTML,
        "--cover-image", cover_img,
        "--thumbnail", thumb_img,
        "--pay-what-you-want",
        "--suggested-price", args.suggested_price,
        "--json",
        "--non-interactive",
    ]

    cmd.extend([
        # File 1: macOS Universal
        "--file", mac_dmg,
        "--file-name", "PhotonForge-1.0.0-macOS-Universal.dmg",
        "--file-description", "macOS Universal 2 Installer (Apple Silicon M1-M4 & Intel 64-bit). macOS 11.0+",
        # File 2: Windows
        "--file", win_zip,
        "--file-name", "PhotonForge-1.0.0-Windows-x64.zip",
        "--file-description", "Windows 10 & 11 (64-bit) Portable Standalone Executable",
        # File 3: Linux
        "--file", lin_tar,
        "--file-name", "PhotonForge-1.0.0-Linux-x86_64.tar.gz",
        "--file-description", "Linux 64-bit Native Binary, Desktop Launcher & App Icon",
    ])

    for t in TAGS:
        cmd.extend(["--tag", t])

    if args.draft:
        cmd.append("--draft")

    if args.dry_run:
        print("Dry run mode. Command that would be executed:")
        print(" ".join(cmd[:15]) + " ... [files and description omitted for brevity]")
        return

    print("==> Creating PhotonForge v1.0.0 product on Gumroad...")
    res = subprocess.run(cmd, capture_output=True, text=True)
    if res.returncode == 0:
        try:
            out_json = json.loads(res.stdout)
            prod_id = out_json.get("product", {}).get("id") or out_json.get("id")
            prod_url = out_json.get("product", {}).get("short_url") or out_json.get("short_url")
            if prod_id and not args.draft:
                subprocess.run(["gumroad", "products", "publish", prod_id], capture_output=True)
            custom_domain_url = "https://shop.pietiniart.com/l/photonforge"
            print(f"==> SUCCESS: PhotonForge v1.0.0 LIVE on Gumroad! ID={prod_id} URL={custom_domain_url} ({prod_url})")
            if os.path.exists(TRACKER_FILE):
                with open(TRACKER_FILE, "r") as tf:
                    tracker = json.load(tf)
                tracker["PhotonForge_v1_0_0"] = {
                    "id": prod_id,
                    "url": prod_url,
                    "custom_domain_url": custom_domain_url,
                    "title": PRODUCT_NAME,
                    "time": datetime.datetime.now().isoformat()
                }
                with open(TRACKER_FILE, "w") as tf:
                    json.dump(tracker, tf, indent=2)
        except Exception as e:
            print("STDOUT:", res.stdout)
            print("Tracker update warning:", e)
    else:
        print("STDOUT:", res.stdout)
        if res.stderr:
            print("STDERR:", res.stderr)
        print(f"==> FAILED with code {res.returncode}")
        sys.exit(res.returncode)


if __name__ == "__main__":
    main()
