#!/usr/bin/env python3
"""
generate_gumroad_and_github_release.py
1. Generates high-resolution promotional cover images & Gumroad thumbnails for PhotonForge v1.0.0
   in /Volumes/Dataa/Ohjelmointi/ProjektitRust/photon_forge/dist/gumroad_assets/
2. Creates/updates public GitHub repository pietini69/photonforge and publishes GitHub Release v1.0.0
   with all cross-platform packages (macOS Universal DMG & ZIP, Windows x64 ZIP, Linux x86_64 tar.gz).
3. Creates publish_photonforge_to_gumroad.py for 1-click freeware ($0+ Pay What You Want) Gumroad publishing.
"""

import os
import subprocess
from PIL import Image, ImageDraw, ImageFont, ImageOps

PROJECT_DIR = "/Volumes/Dataa/Ohjelmointi/ProjektitRust/photon_forge"
DIST_DIR = os.path.join(PROJECT_DIR, "dist")
ASSETS_DIR = os.path.join(DIST_DIR, "gumroad_assets")


def get_font(name, size):
    paths = {
        "serif_b": "/System/Library/Fonts/Supplemental/Georgia Bold.ttf",
        "sans_b": "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        "sans": "/System/Library/Fonts/Supplemental/Arial.ttf",
        "mono": "/System/Library/Fonts/Supplemental/Courier New Bold.ttf",
    }
    try:
        return ImageFont.truetype(paths.get(name, paths["sans"]), size)
    except Exception:
        return ImageFont.load_default()


def build_promo_images():
    os.makedirs(ASSETS_DIR, exist_ok=True)
    icon_p = os.path.join(PROJECT_DIR, "assets/icon_512.png")
    m42_p = "/tmp/m42_full_processed.png"

    # 1. Hero Cover (2700 x 2025)
    hw, hh = 2700, 2025
    hero = Image.new("RGB", (hw, hh), (10, 14, 22))
    hd = ImageDraw.Draw(hero)

    # Subtle starfield background
    for i in range(280):
        sx = (i * 997) % hw
        sy = (i * 613) % hh
        r = 1 if i % 4 != 0 else 2
        c = (180, 205, 255) if i % 3 == 0 else (255, 240, 210)
        hd.ellipse([sx - r, sy - r, sx + r, sy + r], fill=c)

    # Top header banner
    hd.rectangle([0, 0, hw, 250], fill=(16, 22, 36))
    hd.line([(0, 250), (hw, 250)], fill=(70, 190, 255), width=5)

    if os.path.exists(icon_p):
        ic = Image.open(icon_p).convert("RGBA").resize((180, 180), Image.Resampling.LANCZOS)
        hero.paste(ic, (90, 35), ic)

    hd.text((310, 82), "PHOTONFORGE v1.0.0", fill=(95, 210, 255), font=get_font("sans_b", 72), anchor="lm")
    hd.text(
        (310, 165),
        "Native Rust Astrophotography Stacking, Post-Processing & ZWO Seestar Desktop Suite",
        fill=(235, 242, 255),
        font=get_font("sans_b", 36),
        anchor="lm",
    )
    hd.rounded_rectangle([2120, 55, 2610, 195], radius=24, fill=(32, 165, 95))
    hd.text((2365, 105), "100% FREEWARE", fill=(255, 255, 255), font=get_font("sans_b", 36), anchor="mm")
    hd.text((2365, 155), "macOS • Windows • Linux", fill=(220, 255, 235), font=get_font("sans_b", 26), anchor="mm")

    # Left showcase card: Simulated PhotonForge UI with M42 processed image + WCS Overlay
    ui_x, ui_y, ui_w, ui_h = 80, 310, 1520, 1460
    hd.rounded_rectangle([ui_x, ui_y, ui_x + ui_w, ui_y + ui_h], radius=22, fill=(18, 22, 32), outline=(70, 180, 245), width=4)
    hd.rectangle([ui_x + 4, ui_y + 4, ui_x + ui_w - 4, ui_y + 68], fill=(24, 30, 44))
    hd.text(
        (ui_x + 30, ui_y + 36),
        "⚡ PhotonForge v1.0.0  |  🇬🇧 EN / 🇫🇮 FI  |  ⚡ Auto-STF  |  🌐 ABE  |  🎯 SPCC/PCC  |  🧭 WCS & Overlay  |  🔭 Seestar",
        fill=(180, 225, 255),
        font=get_font("sans_b", 23),
        anchor="lm",
    )

    if os.path.exists(m42_p):
        m42 = Image.open(m42_p).convert("RGB")
        m42_fit = ImageOps.fit(m42, (ui_w - 40, ui_h - 110), Image.Resampling.LANCZOS)
        # Draw WCS grid & M42 annotation on preview
        md = ImageDraw.Draw(m42_fit)
        mw, mh = m42_fit.size
        for gx in range(200, mw, 320):
            md.line([(gx, 0), (gx + 60, mh)], fill=(65, 160, 220), width=2)
        for gy in range(200, mh, 280):
            md.line([(0, gy), (mw, gy - 40)], fill=(65, 160, 220), width=2)
        cx, cy = mw // 2, mh // 2
        md.ellipse([cx - 180, cy - 180, cx + 180, cy + 180], outline=(80, 240, 255), width=3)
        md.text((cx + 195, cy - 120), "M42 — Orion Nebula (RA 05h35m, Dec -05°23')", fill=(110, 245, 255), font=get_font("sans_b", 26))
        md.text((cx - 140, cy + 210), "M43 — De Mairan's Nebula", fill=(110, 245, 255), font=get_font("sans_b", 24))
        hero.paste(m42_fit, (ui_x + 20, ui_y + 82))

    # Right feature cards
    cards = [
        (
            "🌌 MULTI-THREADED STACKING & 2X DRIZZLE",
            [
                "• Automatic Star Alignment (corrects Alt-Az field rotation!)",
                "• Sigma-Clipping outlier rejection & 2x Super-Resolution Drizzle",
                "• Multi-Panel Mosaic Stitcher with photometric seam feathering",
                "• Native FITS (8/16/32/-32/-64), PixInsight .XISF, 16-bit TIFF",
            ],
        ),
        (
            "🎨 PIXINSIGHT-CLASS POST-PROCESSING",
            [
                "• Real-time Auto-STF (Linked & Unlinked RGB channels)",
                "• 2D Polynomial ABE Background Extraction & SCNR",
                "• PCC & SPCC Spectrophotometric Star Color Calibration",
                "• Built-in WCS Plate Solver & Messier/NGC/IC Sky Overlay",
            ],
        ),
        (
            "🌈 NARROWBAND, CURVES & NEURAL AI",
            [
                "• 1-Click SHO (Hubble), HOO (Bicolor), HSO & LRGB Combiner",
                "• PCHIP Spline Curves, Arcsinh Stretch & Dual-Band Ha/OIII Boost",
                "• Built-in Multi-Scale Wavelet Starless & Denoise + SyQon AI",
            ],
        ),
        (
            "🔭 ZWO SEESTAR S50 / S30 DESKTOP SUITE",
            [
                "• Direct Wi-Fi TCP Telescope Control & 360° Interactive Sky Map",
                "• Automated All-Night Multi-Target Sequencer (AF, Dither, LP)",
                "• Visual Mosaic Framing Planner & Real-Time SMB Live Stacker",
            ],
        ),
    ]

    cy_box = 310
    for c_title, c_lines in cards:
        hd.rounded_rectangle([1640, cy_box, 2620, cy_box + 340], radius=20, fill=(20, 28, 42), outline=(65, 145, 210), width=3)
        hd.text((1675, cy_box + 42), c_title, fill=(255, 205, 85), font=get_font("sans_b", 28), anchor="lm")
        for idx, ln in enumerate(c_lines):
            hd.text((1675, cy_box + 98 + idx * 58), ln, fill=(225, 235, 248), font=get_font("sans", 24), anchor="lm")
        cy_box += 372

    # Bottom banner
    hd.rounded_rectangle([80, 1835, hw - 80, 1965], radius=26, fill=(235, 185, 70))
    hd.text(
        (hw // 2, 1900),
        "INSTANT NATIVE STARTUP (<0.2s) • ~10 MB BINARY • BILINGUAL UI (🇬🇧 EN / 🇫🇮 FI) • FREE DOWNLOAD (0 €+)",
        fill=(14, 18, 26),
        font=get_font("sans_b", 31),
        anchor="mm",
    )

    hero_out = os.path.join(ASSETS_DIR, "01_PhotonForge_Hero_Cover.jpg")
    hero.save(hero_out, "JPEG", quality=95)
    print(f"Saved: {hero_out}")

    # 2. Square Gumroad Thumbnail (1000 x 1000)
    sq = Image.new("RGB", (1000, 1000), (12, 16, 26))
    sd = ImageDraw.Draw(sq)
    sd.rectangle([0, 0, 1000, 175], fill=(18, 26, 42))
    sd.text((500, 62), "PHOTONFORGE v1.0", fill=(95, 210, 255), font=get_font("sans_b", 56), anchor="mm")
    sd.text((500, 130), "ASTRO STACKER • PROCESSING • SEESTAR SUITE", fill=(255, 205, 85), font=get_font("sans_b", 27), anchor="mm")
    if os.path.exists(m42_p):
        m42_sq = ImageOps.fit(Image.open(m42_p).convert("RGB"), (900, 640), Image.Resampling.LANCZOS)
        sq.paste(m42_sq, (50, 200))
        sd.rectangle([50, 200, 950, 840], outline=(80, 195, 255), width=4)
    if os.path.exists(icon_p):
        ic_s = Image.open(icon_p).convert("RGBA").resize((150, 150), Image.Resampling.LANCZOS)
        sq.paste(ic_s, (75, 225), ic_s)
    sd.rounded_rectangle([60, 875, 940, 965], radius=20, fill=(32, 165, 95))
    sd.text((500, 920), "FREE DOWNLOAD (0 €+) • MAC • WIN • LINUX", fill=(255, 255, 255), font=get_font("sans_b", 30), anchor="mm")
    sq_out = os.path.join(ASSETS_DIR, "gumroad_thumb_square.jpg")
    sq.save(sq_out, "JPEG", quality=95)
    print(f"Saved: {sq_out}")


if __name__ == "__main__":
    build_promo_images()
