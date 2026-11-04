# 🌌 PhotonForge v1.0.0 🦀⚡

### The Blazingly Fast, Native Rust Astrophotography Stacking, Post-Processing & ZWO Seestar Desktop Suite for macOS, Windows & Linux

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-dea584.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey.svg)]()
[![Free Download on Gumroad](https://img.shields.io/badge/Gumroad-Free%20Download-orange.svg)](https://shop.pietiniart.com/l/photonforge)
[![Buy Me A Coffee](https://img.shields.io/badge/☕%20Buy%20Me%20A%20Coffee-Donate-yellow.svg)](https://buymeacoffee.com/pnp69)

**PhotonForge** is an ultra-fast, native desktop astrophotography processing, multi-threaded stacking, mosaic stitching, and **ZWO Seestar S50 / S30** smart telescope control platform written from scratch in pure **Rust** (`eframe` / `egui`, `rayon`).

Part of the **PhotonCAM** software family, PhotonForge combines the core post-processing power of **PixInsight** and **Siril** with a native desktop command center for smart telescopes and traditional rigs — in a single **~10 MB native executable** that starts in under **0.2 seconds**.

---

## ⚡ Why PhotonForge?

Astrophotographers today often have to juggle 5–6 separate programs just to process one night's data: a phone app to control a Seestar, a file transfer tool, Siril or DeepSkyStacker to stack, GraXpert for background extraction, StarNet for star removal, ASTAP for plate solving, and a photo editor for curves and color calibration.

**PhotonForge brings the entire workflow into one cohesive, instant-startup native desktop app:**

| Feature | Legacy Free Stackers | Heavy Commercial Suites ($200–$300+) | Mobile Smart Scope Apps | **PhotonForge v1.0.0 🦀** |
|---|---|---|---|---|
| **OS Support** | Windows-centric or complex setup | Windows, macOS, Linux | Phone / Tablet only | **Native macOS (Universal 2), Windows x64 & Linux x86_64** |
| **Startup & Size** | Slow startup, huge temp scratch disks | 1.5 GB+ install, slow startup | Mobile UI only | **<0.2s cold start, ~10 MB single binary** |
| **File Formats** | FITS / TIFF | FITS / XISF / TIFF | JPEG / FITS on device | **Native FITS (8/16/32/-32/-64), PixInsight `.xisf`, 16-bit TIFF, PNG** |
| **Star Alignment & Stacking** | Manual scripts / folders | Multi-step WBPP pipeline | Internal 1x stack | **1-Click Sub-Pixel Star Alignment, Sigma Clipping & 2x Super-Resolution Drizzle** |
| **Multi-Panel Mosaic Stitcher** | ❌ None / complex scripts | Separate paid modules | Limited framing | **Built-in Multi-Panel Mosaic Planner + Photometric Seam-Feathered Stitcher** |
| **Color Calibration (PCC / SPCC)** | Basic white balance | ✅ Included | ❌ None | **1-Click Photometric (PCC) & Spectrophotometric (SPCC) Star Calibration** |
| **WCS Plate Solver & Overlay** | External CLI required | ✅ Included | Basic overlay | **Built-in Blind/Catalog WCS Solver + RA/Dec Grid, M/NGC/IC & Star Annotations** |
| **AI / Wavelet Starless & Denoise** | External tools required | Paid plugins ($60–$180 extra) | Basic denoise | **Built-in Multi-Scale Wavelet Engine + Direct SyQon Neural AI Integration (Axiom, Parallax, Prism)** |
| **Direct Seestar S50/S30 Desktop Control** | ❌ None | ❌ None | Mobile only | **Built-in Wi-Fi TCP Client, All-Night Multi-Target Sequencer, Auto-Focus, Dither & SMB Live Stack** |
| **Price & License** | Free | €250 – €400+ | Hardware locked | **100% Freeware / Pay What You Want (0 €+)** |

---

## 🎯 Complete Feature Breakdown

### 1. 🌌 Multi-Threaded Stacking, 2x Drizzle & Mosaic Stitcher
- **Automatic Star Registration:** Triangle/quad asterism matching with sub-pixel bilinear/Lanczos interpolation that automatically corrects **field rotation** (ideal for alt-az mounts like ZWO Seestar S50/S30 and Dwarf) and tracking drift.
- **Outlier Rejection:** **Sigma Clipping** (configurable Low/High thresholds to reject satellite trails, airplanes, and hot pixels), **Average**, **Median**, and **Min/Max Rejection**.
- **2x Drizzle Super-Resolution:** Reconstructs double-resolution images with rounder stars from dithered sub-exposures.
- **Multi-Panel Mosaic Stitcher:** Automatically registers overlapping sky panels onto an expanded canvas with photometric background equalization and cosine seam feathering.
- **Auto-Filter Grouping:** Automatically groups loaded frames by FITS `FILTER` header (`Ha`, `OIII`, `SII`, `L`, `R`, `G`, `B`, `LP`) and stacks each group into its own master.

### 2. ⚡ Real-Time EAA Live Stacking
- Watches any local or network folder (including mounted Seestar SMB shares) in real time.
- Automatically debayers, registers stars, rejects bad frames, updates running SNR statistics, and applies non-destructive Auto-STF stretch as each new exposure arrives.

### 3. 🎨 PixInsight-Class Post-Processing & Color Calibration
- **⚡ Real-Time Auto-STF (Screen Transfer Function):** Linked and Unlinked RGB channel Midtones Transfer Function (MTF) using Median + MAD statistics (`0.08 Deep`, `0.10 Standard`, `0.20 Bright` presets) without altering linear 32-bit float data.
- **🌐 Automatic Background Extraction (ABE):** 2D polynomial surface fitting (Degrees 1–4) with both **Subtraction** (light pollution gradients) and **Division** (vignetting correction).
- **🎯 PCC & SPCC (Photometric & Spectrophotometric Color Calibration):** Calibrates true astrophysical color balance using unsaturated field stars, Planckian blackbody temperatures ($T_{\text{eff}}$), white reference models (*Average Spiral Galaxy*, *G2V Solar Star*), camera QE curves (*Sony IMX462/IMX585/IMX571/IMX533*), and dual-band filter profiles.
- **🧭 WCS Plate Solver & Sky Annotation Overlay:** Computes TAN gnomonic WCS projection (`CRVAL`, `CD` matrix, pixel scale, field rotation) and renders non-destructive vector overlays directly on the image: **RA/Dec coordinate grid**, **Messier/NGC/IC deep-sky ellipses**, **identified stars**, and **N/E Compass Rose**.
- **🌈 Narrowband & Broadband Channel Combiner:** Instant **SHO (Hubble)**, **HOO (Bicolor)**, **HSO**, and **LRGB** combination with automatic sub-pixel channel registration and Linear Fit background matching.
- **📈 Monotonic PCHIP Spline Curves & Arcsinh Stretch:** Interactive multi-channel (`RGB, R, G, B, Saturation`) spline curves with live preview, plus color-preserving **Arcsinh Stretch**, **Dual-Band Ha/[OIII] Boost**, **Reflection Nebula Blue/Cyan Boost**, **SCNR Green Removal**, and **Morphological Star Reduction**.
- **✨ SyQon Neural AI & Built-in Wavelet Engine:** Seamless integration with free local SyQon AI models (**Axiom** Star Removal / Starless separation, **Parallax** PSF Deconvolution & Star Aberration Fix, **Prism** Deep Denoise, and **Deep Gradient** removal), plus a built-in pure-Rust À Trous wavelet fallback engine that works out-of-the-box with zero external dependencies.

### 4. 🔭 Built-in ZWO Seestar S50 / S30 Desktop Command Center
- **Interactive All-Sky Dome & Star Chart:** Real-time altitude/azimuth sky dome, tonight's visibility & altitude curve (18:00–06:00), and searchable Deep-Sky Catalog (Galaxies, Nebulae, Clusters).
- **Automated All-Night Sequencer:** Queue multiple targets for the entire night with automatic meridian/horizon checks, scheduled **Auto-Focus** (by time interval or $\Delta T$ temperature drop), **Dithering**, **Dual-Band LP Filter** switching, **Wi-Fi Drop Watchdog (Auto-Reconnect)**, and automatic dawn parking.
- **Interactive Mosaic Framing Planner:** Visually frame $1\times2$, $2\times1$, $2\times2$, $3\times2$, or $3\times3$ mosaics on the sky chart and send all panels directly to the night sequencer.
- **1-Click Bilingual UI (`🇬🇧 EN` / `🇫🇮 FI`):** Switch the entire interface between English and Finnish at any time from the top toolbar.

---

## 📥 Downloads

Pre-built binaries are available from [GitHub Releases](https://github.com/pietini69/photonforge/releases/latest) or [Gumroad](https://shop.pietiniart.com/l/photonforge):

| Operating System | Package | Architecture | Direct Download |
|---|---|---|---|
| **macOS** (11.0+) | **DMG Installer** | Universal 2 (Apple Silicon M1–M4 + Intel) | [PhotonForge-1.0.0-universal.dmg](https://github.com/pietini69/photonforge/releases/download/v1.0.0/PhotonForge-1.0.0-universal.dmg) |
| **macOS** (11.0+) | **ZIP Archive** | Universal 2 | [PhotonForge-1.0.0-macos-universal.zip](https://github.com/pietini69/photonforge/releases/download/v1.0.0/PhotonForge-1.0.0-macos-universal.zip) |
| **Windows** (10/11) | **ZIP Executable** | x86_64 (64-bit) | [PhotonForge-1.0.0-windows-x64.zip](https://github.com/pietini69/photonforge/releases/download/v1.0.0/PhotonForge-1.0.0-windows-x64.zip) |
| **Linux** | **tar.gz Archive** | x86_64 | [PhotonForge-1.0.0-linux-x86_64.tar.gz](https://github.com/pietini69/photonforge/releases/download/v1.0.0/PhotonForge-1.0.0-linux-x86_64.tar.gz) |

> **macOS First Launch Note:** If macOS Gatekeeper flags the app as unsigned, right-click `PhotonForge.app` in `/Applications` and select **Open**, or run:
> ```bash
> xattr -cr /Applications/PhotonForge.app
> ```

---

## ☕ Support the Project

PhotonForge is free software (**0 €+ Pay What You Want**). If PhotonForge saves you money on expensive astrophotography licenses or makes your imaging nights more enjoyable, you can support continued development:

- ☕ **[Buy Me a Coffee (buymeacoffee.com/pnp69)](https://buymeacoffee.com/pnp69)**
- 🛒 **[Gumroad Store (shop.pietiniart.com/l/photonforge)](https://shop.pietiniart.com/l/photonforge)**
