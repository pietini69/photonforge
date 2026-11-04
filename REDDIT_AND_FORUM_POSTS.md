# 🌌 PhotonForge v1.0.0 – Julkaisukanavat & Valmiit Postaukset (Reddit, Cloudy Nights, Foorumit)

---

## 1. Kannattaako PhotonForge julkaista? (Rehellinen arvio)

**Ehdottomasti KYLLÄ — ja potentiaali on jopa suurempi kuin PhotonCAMilla.** Tässä syyt:

1. **Täydellinen markkinarako (ZWO Seestar S50 / S30 + Desktop Post-Processing):**
   - Maailmalla on myyty yli **100 000+ ZWO Seestar S50 ja S30** -älykaukoputkea, mutta ZWO:lla **ei ole lainkaan virallista tietokonesovellusta (macOS / Windows / Linux)**!
   - Käyttäjät tuskastelevat jatkuvasti sen kanssa, että:
     - Puhelinapplikaatiossa ei ole kunnollista koko yön monikohteista sekvensseriä (All-Night Multi-Target Sequencer).
     - Kentänkiertyminen (field rotation) ja mosaiikkien yhdistäminen vaativat normaalisti raskaita skriptejä Sirilissä tai maksullisen **PixInsightin (~300 € + 180 € lisäosat)** tai **AstroPixelProcessorin (~200 €)**.
   - **PhotonForge** antaa heille **yhdessä ainoassa ~10 Mt natiivissa sovelluksessa** sekä **Seestarin työpöytäohjauksen + yösekvensserin + mosaiikkisuunnittelijan + reaaliaikaisen Live-pinoamisen** ETTÄ **täyden jälkikäsittelyn** (Sub-pixel pinoaminen, 2x Drizzle, Mosaiikkien saumaton yhdistäminen, ABE, SPCC/PCC, Plate Solve -annotaatiot, SHO/HOO-kanavayhdistys, Käyrät ja Tähtien poisto) — **ilmaiseksi (0 €+)**.
2. **Mac-, Windows- ja Linux-tuki ilman asennushelvettiä:**
   - Tähtikuvausohjelmistot ovat tunnetusti raskaita (gigatavujen asennuspaketteja tai komentorivipohjaisia Python-viritelmiä). Kun tarjoat valmiin `.dmg`-asentajan Macille, `.exe`-paketin Windowsille ja `.tar.gz`-binäärin Linuxille, kynnys kokeilla on nolla.
3. **Sama toimiva Freeware-malli kuin PhotonCAMissa:**
   - **Gumroad (`0 €+ Pay What You Want`)** + **GitHub Releases** + **Buy Me a Coffee**. Tähtikuvaajat ovat erittäin sitoutunut ja ostovoimainen harrastajakunta (kaukoputket ja kamerat maksavat satoja tai tuhansia euroja), joten vapaaehtoiset kahvitipit ja näkyvyys koko kaupallesi ovat erittäin todennäköisiä.

---

## 2. Missä PhotonForgesta kannattaa kertoa? (Parhaat kanavat tärkeysjärjestyksessä)

### 🔥 Kategoria A: Reddit (Suurin välitön liikenne)
1. **`r/seestar`** *(TÄRKEIN TÄSMÄKANAVA!)*
   - Seestar S50 / S30 -omistajat etsivät juuri nyt tietokoneohjelmaa, yösekvensseriä, mosaiikkien yhdistäjää ja helppoa FITS-pinoajaa.
2. **`r/AskAstrophotography`** *(Erittäin aktiivinen, sallii ohjelmistotyökalujen esittelyn)*
   - Täynnä aloittelijoita ja harrastajia, joille PixInsight (300 €) on liian kallis ja Siril liian monimutkainen.
3. **`r/telescopes`** & **`r/astrophotography`**
   - Huom: tarkista `r/astrophotography`-säännöt (postaa mieluummin oikea PhotonForgella pinottu ja käsitelty kuva esim. M42 Orionin sumusta ja kerro kommenteissa / otsikossa, että käsittelit sen omalla ilmaisella Rust-ohjelmallasi!).
4. **`r/rust`**
   - Rust-yhteisö rakastaa natiiveja `egui`-työpöytäsovelluksia, joissa on oikeaa tieteellistä laskentaa (FITS/XISF, WCS Plate Solving, SPCC fotometria, Rayon-rinnakkaislaskenta).

### 🔭 Kategoria B: Kansainväliset tähtikuvausfoorumit (Pitkäikäinen orgaaninen liikenne)
5. **Cloudy Nights (`cloudynights.com`)** — Maailman suurin tähtiharrastajien foorumi!
   - Alue 1: **Smart Telescopes** (Seestar S50 / S30 -käyttäjät)
   - Alue 2: **Astronomy Software & Computers**
   - Alue 3: **Electronically Assisted Astronomy (EAA)** (Live-pinoamisen takia!)
6. **Stargazers Lounge (`stargazerslounge.com`)** — Iso-Britannian ja Euroopan suurin foorumi
   - Osio: *Software* sekä *Smart Telescopes*.
7. **AstroBin Forum** — *Software & Processing* -keskustelualue.

### 🇫🇮 Kategoria C: Suomalaiset kanavat
8. **Ursan Tähtikuvaus- ja Kaukoputkifoorumi / Taivaanvahti / Facebookin "Tähtikuvaus" & "Suomen Seestar-harrastajat" -ryhmät**
   - Koska PhotonForgessa on täydellinen **suomenkielinen käyttöliittymä (`🇫🇮 FI`)**, se on käytännössä ainoa moderni tähtikuvauksen pinoamis- ja käsittelyohjelma maailmassa, joka toimii natiivisti suomeksi!

---

## 3. Valmiit kopioitavat julkaisutekstit

### Vaihtoehto 1: Reddit `r/seestar` (Englanti)

**Title:**
> I wanted a single native desktop app (Mac/Win/Linux) to control my Seestar, run all-night mosaic sequences, and stack/process the FITS files without paying $300 for PixInsight — so I built PhotonForge in Rust (Freeware)

**Body:**
```text
Hey everyone,

Like many of you, I love my Seestar, but I got tired of juggling a phone app alongside 4–5 different desktop programs just to plan mosaics, stack sub-frames, remove gradients, and stretch my images. And on macOS, a lot of traditional astro tools feel clunky or bloated.

So I fell down a massive rabbit hole and built **PhotonForge** from scratch in pure Rust. It's a single ~10 MB native desktop app for macOS (Apple Silicon & Intel), Windows, and Linux that starts in 0.2 seconds and combines two worlds:

1. **Seestar S50 / S30 Desktop Command Center & All-Night Sequencer:**
   - Interactive 360° Star Map & Tonight's Altitude Curve (18:00–06:00)
   - Multi-target All-Night Automated Sequencer (auto-focus on temperature drop/interval, dithering, Dual-Band LP filter switching, Wi-Fi drop auto-reconnect watchdog, and auto-park at dawn)
   - Visual Mosaic Planner (1x2, 2x2, 3x2, 3x3)
   - Real-time EAA Live Stacking over SMB network share

2. **Full 32-Bit Linear Post-Processing & Stacking Suite:**
   - Multi-threaded Star Alignment (automatically corrects alt-az field rotation!), Sigma-Clipping Stacking & **2x Drizzle Super-Resolution**
   - **Multi-Panel Mosaic Stitcher** with photometric background matching and smooth seam feathering
   - **1-Click Auto-STF**, **ABE (2D Polynomial Background Extraction)**, and **SCNR**
   - **PCC / SPCC** (Photometric & Spectrophotometric Star Color Calibration with IMX462/IMX585 sensor + dual-band filter profiles)
   - **Built-in WCS Plate Solver & Sky Annotations** (draws RA/Dec grid, Messier/NGC/IC ellipses, and star labels right over your image)
   - **SHO / HOO / LRGB Channel Combiner**, **PCHIP Spline Curves**, **Arcsinh Stretch**, **Dual-Band Ha/OIII Boost**, and **Starless / Wavelet Denoise** (plus direct SyQon Neural AI CLI support)
   - Reads and writes native **FITS**, **PixInsight .xisf**, **16-bit TIFF**, and **PNG**

I'm releasing v1.0.0 as freeware ($0 / Pay What You Want on Gumroad, or direct download from GitHub Releases):

- **Gumroad ($0+ Free Download):** https://shop.pietiniart.com/l/photonforge
- **GitHub Releases (macOS DMG, Windows x64 ZIP, Linux tar.gz):** https://github.com/pietini69/photonforge/releases/latest

Would love to hear your feedback or feature requests if you give it a spin on your Seestar data!
```

---

### Vaihtoehto 2: Reddit `r/AskAstrophotography` & Cloudy Nights (Englanti)

**Title:**
> Built a lightweight (~10 MB) native Rust astrophotography stacker & post-processing suite (FITS/XISF, 2x Drizzle, Mosaic Stitcher, ABE, SPCC/PCC, WCS Plate Solve, SHO/HOO, Seestar Sequencer) – Free for Mac, Windows & Linux

**Body:**
```text
Hi all,

I've been working on a fast, standalone native astrophotography processing and stacking application written in pure Rust called **PhotonForge**, and I just packaged v1.0.0 for macOS (Universal), Windows (64-bit), and Linux.

My goal was to make something that opens in 0.2 seconds with zero installation headaches and handles the everyday 90% of deep-sky workflow in one window:

- **Native File I/O:** Reads/writes 8/16/32-bit FITS, IEEE float FITS, PixInsight `.xisf`, 16-bit TIFF, and PNG.
- **Fast Multi-Threaded Stacking:** Automatic sub-pixel triangle/quad star registration (including field-rotation correction for alt-az mounts), Sigma Clipping, 2x Drizzle Super-Resolution, and automatic filter group batching (`Ha`, `OIII`, `SII`, `L`, `R`, `G`, `B`).
- **Multi-Panel Mosaic Stitcher:** Automatically registers overlapping panels onto a unified canvas with photometric background equalization and cosine seam feathering.
- **Calibration & Color:** 1-click Auto-STF (linked/unlinked), 2D polynomial ABE (subtraction & division), SCNR, and **PCC / SPCC** spectrophotometric star color calibration.
- **WCS Plate Solver & Annotations:** Solves field WCS and overlays RA/Dec grid, Messier/NGC/IC catalogs, and star labels.
- **Narrowband & Stretching:** SHO / HOO / HSO / LRGB channel combiner with Linear Fit, PCHIP spline curves, color-preserving Arcsinh stretch, Dual-Band & Reflection Nebula boosters, Star Reduction, and built-in multi-scale wavelet Starless/Denoise (plus SyQon AI integration).
- **Smart Telescope Integration:** Includes a built-in ZWO Seestar S50/S30 desktop controller, sky atlas, mosaic planner, all-night sequencer, and real-time EAA live stacker.

It is 100% freeware (0 €+ on Gumroad or direct binary download from GitHub):
- **Gumroad:** https://shop.pietiniart.com/l/photonforge
- **GitHub Releases:** https://github.com/pietini69/photonforge/releases/latest

Clear skies, and let me know if you run into any bugs or have ideas for v1.1!
```

---

### Vaihtoehto 3: Suomalaiset tähtikuvausryhmät (Ursa / Facebook Tähtikuvaus / Seestar Suomi)

**Otsikko:**
> Tein suomenkielisen (ja englanninkielisen) tähtikuvauksen pinoamis-, jälkikäsittely- ja Seestar-ohjausohjelman Rustilla (Mac, Windows & Linux) – ilmainen lataus!

**Teksti:**
```text
Moi kaikki!

Tuli rakennettua omaan tähtikuvauskäyttöön natiivi työpöytäohjelma nimeltä **PhotonForge** (macOS, Windows ja Linux), ja ajattelin jakaa sen ilmaiseksi muillekin harrastajille. Ohjelmassa on täysin suomenkielinen käyttöliittymä (ja yläpalkista saa napilla vaihdettua Suomi 🇫🇮 / Englanti 🇬🇧).

Mitä PhotonForge tekee yhdessä ~10 Mt ohjelmassa:
- **Nopea rinnakkaispinoaminen & 2x Drizzle:** Automaattinen tähtikohdistus (korjaa myös alt-az-jalustojen kentänkiertymisen), Sigma-Clipping (poistaa satelliittiviivat) ja 2x Drizzle -superresoluutio.
- **Mosaiikkien yhdistäminen (Mosaic Stitcher):** Yhdistää moniruutuiset mosaiikit automaattisesti pehmeällä saumahäivytyksellä ja taustan valoisuuden tasauksella.
- **Täysi 32-bittinen jälkikäsittely:** Reaaliaikainen Auto-STF, ABE-taustagradientin poisto, SPCC/PCC fotometrinen tähtien värikalibrointi, SCNR-viherkohinan poisto, SHO/HOO/LRGB-kanavayhdistys, käyrätyökalu (Curves), Arcsinh-venytys sekä tähtien pienennys ja poisto (tukee myös ilmaista SyQon AI -neuroverkkoa).
- **WCS Plate Solver & kohdeannotaatiot:** Tunnistaa tähtikentän ja piirtää kuvan päälle RA/Dec-koordinaattiruudukon sekä Messier/NGC/IC-kohteet.
- **ZWO Seestar S50 / S30 -työpöytäohjaus & Yösekvensseri:** Interaktiivinen tähtikartta, yön korkeuskäyrä, mosaiikkisuunnittelija, koko yön automaattinen kuvausjono (automaattitarkennus lämpötilamuutoksesta, dithering, yhteysvahti) sekä reaaliaikainen Live-pinoaminen suoraan verkkolevyltä.
- **Tiedostomuodot:** FITS, PixInsight .xisf, 16-bit TIFF ja PNG.

Ohjelma on täysin ilmainen (kirjoita Gumroadissa hinnaksi 0, tai lataa suoraan GitHubista):
- **Gumroad (0 €+):** https://shop.pietiniart.com/l/photonforge
- **GitHub Releases:** https://github.com/pietini69/photonforge/releases/latest

Ottakaa ihmeessä testiin omilla FITS/XISF-kuvilla tai Seestarin kanssa ja kertokaa palautetta!
```
