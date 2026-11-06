# Changelog

All notable changes to **PhotonForge** are documented in this file.

## [1.0.4] - 2026-11-06

### Added & Improved
- **Direct ASCOM Alpaca Live Stacking & Continuous Camera Capture:**
  - Automated continuous camera exposure loop for ZWO Seestar (S50/S50 Pro/S30/S30 Pro) and ASCOM Alpaca mounts.
  - Automatically commands exposure start, polls exposure completion (`imageready`), fetches 16-bit raw sensor data (`imagearray`), and writes 100% compliant FITS files with full headers (`BITPIX=16`, `BZERO=32768`, `BAYERPAT=GRBG`, `NAXIS1=1920`, `NAXIS2=1080`).
  - Automatically feeds each captured sub-frame directly into PhotonForge's Live Stacker engine in real-time.
  - Automatic dithering coordination between sub-exposures.
  - Clean `camera/0/stopexposure` handling upon stopping stacking.
- **Customizable Sub-Exposure Time Controls:**
  - Quick sub-exposure duration buttons (`[10s]` `[20s]` `[30s]` `[60s]`) plus custom slider (`1..=300s`) in Telescope sidebar and Live Stacking tab.
  - Per-target sub-exposure duration setting in the Night Sequencer and target display cards.
- **Dual Imaging Workflow Support:**
  - Full support for both direct standalone Alpaca camera capturing and traditional Seestar mobile app capture with quick `📂 Open Seestar Network Share (EMMC Images)` launcher.

## [1.0.3] - 2026-11-04

### Added & Improved
- **Astrometry.net Cloud Plate Solving:**
  - High-precision cloud-based blind plate solving via Astrometry.net API.
  - Dedicated configuration dialog with direct browser API key retrieval link, live credential validation test, and default solver selection.
  - Non-blocking background solver with stage progression, real-time status reporting, and user cancellation.
- **Left Sidebar Resizing Stability & Drift Prevention:**
  - Completely resolved egui runaway panel expansion feedback loop caused by `set_width(available_width)` inside frame groups.
  - Replaced with non-expanding `set_max_width` throughout files, stacking, live stack, channel combinations, and telescope tabs.
  - Implemented robust `Layout::right_to_left` header badge and button anchoring, ensuring buttons and status labels never overflow the panel edge.
  - Guaranteed text wrapping and automatic title truncation across overnight sequencer targets.
  - Clean 400 px default width with smooth manual resizing from 320 px to 650 px.
- **Interactive 3D Horizon Sky Dome & Smoother Zoom:**
  - Added ground-referenced 3D horizon sky dome perspective alongside zenith all-sky projection.
  - Refined exponential zoom response for stable, controllable field-of-view inspection.
- **DwarfLab Smart Telescope Support:**
  - Direct integration for DwarfLab DWARF 3 4K, DWARF II, DWARF Mini, and DRACO smart telescopes via Open API (WebSocket) and dwarfAlp Alpaca bridge.
  - Accurate optics profiles, sensor FOV grids, and automated image folder ingestion.

## [1.0.2] - 2026-11-04

### Added & Improved
- **Full Bilingual Localization in Telescope Control & Star Atlas:**
  - Complete English and Finnish translations for all telescope states, DSO object classifications, night sequencer job statuses, telemetry metrics, and coordinate badges.
  - Resolved lingering Finnish text showing up when English interface is active.
- **Telescope Sidebar & Controls Layout Redesign:**
  - Increased default sidebar width to 430 px with flexible resizing up to 750 px.
  - Implemented responsive horizontal wrapping across IP connection controls, telemetry rows, mosaic planners, and sequencer cards to prevent any text or widgets from overflowing the screen boundary.
- **Cross-Platform Neutrality:**
  - Removed platform-specific references to "Macille" / "Macilla" across user-facing strings and live frame auto-downloaders, replacing them with platform-neutral "tietokoneelle" / "computer".
  - Cross-platform network share mounting support (macOS `open`, Windows `explorer \\<IP>\...`, Linux `xdg-open`).
- **Seestar Alpaca Discovery & Persistent Connection:**
  - Automatic telescope detection via UDP broadcast and ARP/socket probing on port 32323.
  - Remembers previously connected telescope IP address automatically.

## [1.0.1] - 2026-11-04

### Added & Improved
- **ASCOM Alpaca Protocol Integration for Seestar S30/S50:**
  - Direct HTTP Alpaca REST control for slew, tracking, park, unpark, auto-focus, dithering, and LP filter switching.
  - Automated Wi-Fi reconnection watchdog with exponential backoff.
- **Automated Universal Release Packaging:**
  - Built universal macOS dmg/zip packages for both Apple Silicon (ARM64) and Intel (x86_64).

## [1.0.0] - 2026-11-04

### Added
- **Bilingual User Interface (`🇬🇧 EN` / `🇫🇮 FI`):** Instant 1-click language switching in the top toolbar between English and Finnish.
- **Photometric & Spectrophotometric Color Calibration (`PCC` / `SPCC`):** Automatic white balance and channel gain calibration using unsaturated field star photometry, Planckian locus B-V temperature modeling, sensor QE profiles (Sony IMX462, IMX585, IMX571, IMX533), and filter transmission curves (Broadband UV/IR Cut, Dual-Band Ha/OIII, SII/OIII).
- **WCS Plate Solver & Celestial Annotation Overlay:** Built-in TAN gnomonic WCS solver with non-destructive canvas overlays for RA/Dec coordinate grids, Messier/NGC/IC catalog objects, identified stars, and North/East compass rose.
- **Multi-Panel Mosaic Stitcher & Planner:** Automatic star registration across overlapping mosaic panels with photometric background equalization and cosine seam feathering.
- **ZWO Seestar S50 / S30 Desktop Control & All-Night Sequencer:** Direct Wi-Fi TCP telescope control, interactive 360° star chart, tonight's altitude curve, automated multi-target night scheduler with Auto-Focus, Dithering, Dual-Band LP filter control, Wi-Fi auto-reconnect watchdog, and SMB live sub-frame ingestion.
- **Real-Time EAA Live Stacking Engine:** Live folder monitoring with automatic CFA debayering, sub-pixel star alignment, outlier rejection, and live Auto-STF stretch.
- **Narrowband & Broadband Channel Combiner:** One-click SHO (Hubble), HOO (Bicolor), HSO, and LRGB combination with automatic sub-pixel alignment and Linear Fit background matching.
- **SyQon Neural AI & Built-in Multi-Scale Wavelet Suite:** Support for Axiom (Starless separation), Parallax (PSF deconvolution & star aberration fix), Prism (deep denoise), and Deep Gradient extraction, backed by a zero-dependency pure-Rust wavelet engine.
- **Interactive Curves, Arcsinh Stretch, ABE & SCNR:** PCHIP spline curves (`RGB, R, G, B, S`), color-preserving Arcsinh stretch, 2D polynomial Automatic Background Extraction (ABE), SCNR green cast removal, Dual-Band Ha/[OIII] boost, Reflection Nebula boost, and Morphological Star Reduction.
- **Native File Support:** Pure-Rust FITS (`8, 16, 32, -32, -64`), PixInsight `.xisf` read/write, and 8/16-bit TIFF, PNG, and JPEG.
