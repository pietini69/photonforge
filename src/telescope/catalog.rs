use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DsoType {
    Galaxy,
    EmissionNebula,
    ReflectionNebula,
    PlanetaryNebula,
    OpenCluster,
    GlobularCluster,
    SupernovaRemnant,
    Star,
    Planet,
}

impl DsoType {
    pub fn name_fi(&self) -> &'static str {
        match self {
            DsoType::Galaxy => "Galaksi",
            DsoType::EmissionNebula => "Emissiosumu (Kaasu)",
            DsoType::ReflectionNebula => "Heijastussumu (Pöly)",
            DsoType::PlanetaryNebula => "Planetaarinen sumu",
            DsoType::OpenCluster => "Avoin tähtijoukko",
            DsoType::GlobularCluster => "Pallomainen tähtijoukko",
            DsoType::SupernovaRemnant => "Supernovajäänne",
            DsoType::Star => "Tähti",
            DsoType::Planet => "Planeetta / Aurinkokunta",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            DsoType::Galaxy => "🌀",
            DsoType::EmissionNebula => "🔴",
            DsoType::ReflectionNebula => "🔵",
            DsoType::PlanetaryNebula => "🟢",
            DsoType::OpenCluster => "✨",
            DsoType::GlobularCluster => "⚪",
            DsoType::SupernovaRemnant => "💥",
            DsoType::Star => "⭐",
            DsoType::Planet => "🪐",
        }
    }

    pub fn color_rgb(&self) -> (u8, u8, u8) {
        match self {
            DsoType::Galaxy => (240, 180, 100),         // Gold/amber
            DsoType::EmissionNebula => (255, 90, 100),   // Red
            DsoType::ReflectionNebula => (90, 190, 255), // Cyan/Blue
            DsoType::PlanetaryNebula => (80, 240, 160),  // Emerald/Teal
            DsoType::OpenCluster => (180, 220, 255),     // Ice blue
            DsoType::GlobularCluster => (230, 230, 210), // Pale white
            DsoType::SupernovaRemnant => (255, 120, 200),// Magenta
            DsoType::Star => (255, 255, 240),            // White
            DsoType::Planet => (255, 215, 80),           // Golden yellow
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkyObject {
    pub id: String,                 // e.g. "M45", "NGC 7000"
    pub name: String,               // Finnish / common name
    pub constellation: String,      // 3-letter abbreviation, e.g. "Tau", "Ori"
    pub obj_type: DsoType,
    pub ra_hours: f64,              // 0.0 .. 24.0
    pub dec_deg: f64,               // -90.0 .. +90.0
    pub magnitude: f32,             // visual magnitude
    pub size_arcmin: f32,           // approximate size in arcminutes
    pub filter_recommendation: &'static str,
}

impl SkyObject {
    pub fn new(
        id: &str,
        name: &str,
        constellation: &str,
        obj_type: DsoType,
        ra_hours: f64,
        dec_deg: f64,
        magnitude: f32,
        size_arcmin: f32,
        filter_recommendation: &'static str,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            constellation: constellation.to_string(),
            obj_type,
            ra_hours,
            dec_deg,
            magnitude,
            size_arcmin,
            filter_recommendation,
        }
    }
}

/// Returns the comprehensive DSO & Sky catalog
pub fn load_sky_catalog() -> Vec<SkyObject> {
    vec![
        // --- MESSIER CATALOG (Selected highlights + popular targets) ---
        SkyObject::new("M1", "Rapusumu (Crab Nebula)", "Tau", DsoType::SupernovaRemnant, 5.575, 22.01, 8.4, 6.0, "Dual-Band / Kapeakaista"),
        SkyObject::new("M2", "M2 Pallomainen tähtijoukko", "Aqr", DsoType::GlobularCluster, 21.558, -0.82, 6.3, 16.0, "Laajakaista (IRC)"),
        SkyObject::new("M3", "M3 Pallomainen tähtijoukko", "CVn", DsoType::GlobularCluster, 13.703, 28.38, 6.2, 18.0, "Laajakaista (IRC)"),
        SkyObject::new("M8", "Laguunisumu (Lagoon Nebula)", "Sgr", DsoType::EmissionNebula, 18.063, -24.38, 6.0, 90.0, "Dual-Band (Ha/[OIII])"),
        SkyObject::new("M13", "Herkuleksen suuri tähtijoukko", "Her", DsoType::GlobularCluster, 16.695, 36.46, 5.8, 20.0, "Laajakaista (IRC)"),
        SkyObject::new("M15", "M15 Pallomainen tähtijoukko", "Peg", DsoType::GlobularCluster, 21.500, 12.17, 6.2, 18.0, "Laajakaista (IRC)"),
        SkyObject::new("M16", "Kotkasumu (Pillars of Creation)", "Ser", DsoType::EmissionNebula, 18.313, -13.80, 6.4, 35.0, "Dual-Band (Ha/[OIII])"),
        SkyObject::new("M17", "Joutsensumu / Omega-sumu", "Sgr", DsoType::EmissionNebula, 18.347, -16.18, 6.0, 46.0, "Dual-Band (Ha/[OIII])"),
        SkyObject::new("M20", "Trifid-sumu", "Sgr", DsoType::EmissionNebula, 18.043, -23.03, 6.3, 28.0, "Laajakaista / Dual-Band"),
        SkyObject::new("M27", "Nostopainosumu (Dumbbell Nebula)", "Vul", DsoType::PlanetaryNebula, 19.993, 22.72, 7.5, 8.0, "Dual-Band ([OIII]/Ha)"),
        SkyObject::new("M31", "Andromedan galaksi", "And", DsoType::Galaxy, 0.712, 41.27, 3.4, 190.0, "Laajakaista (IRC)"),
        SkyObject::new("M32", "Andromedan seuralaisgalaksi", "And", DsoType::Galaxy, 0.712, 40.87, 8.1, 9.0, "Laajakaista (IRC)"),
        SkyObject::new("M33", "Kolmion galaksi (Triangulum)", "Tri", DsoType::Galaxy, 1.564, 30.65, 5.7, 73.0, "Laajakaista / Ha-Boost"),
        SkyObject::new("M34", "M34 Avoin tähtijoukko", "Per", DsoType::OpenCluster, 2.701, 42.79, 5.5, 35.0, "Laajakaista (IRC)"),
        SkyObject::new("M35", "M35 Avoin tähtijoukko", "Gem", DsoType::OpenCluster, 6.148, 24.33, 5.3, 28.0, "Laajakaista (IRC)"),
        SkyObject::new("M36", "Pinwheel-tähtijoukko", "Aur", DsoType::OpenCluster, 5.602, 34.14, 6.3, 12.0, "Laajakaista (IRC)"),
        SkyObject::new("M37", "M37 Rikas tähtijoukko", "Aur", DsoType::OpenCluster, 5.873, 32.55, 6.2, 24.0, "Laajakaista (IRC)"),
        SkyObject::new("M38", "M38 Tähtikuvio-joukko", "Aur", DsoType::OpenCluster, 5.478, 35.85, 7.4, 21.0, "Laajakaista (IRC)"),
        SkyObject::new("M42", "Orionin suuri kaasusumu", "Ori", DsoType::EmissionNebula, 5.590, -5.39, 4.0, 85.0, "Dual-Band / Laajakaista"),
        SkyObject::new("M43", "De Mairan -sumu (Orion)", "Ori", DsoType::EmissionNebula, 5.592, -5.27, 9.0, 20.0, "Dual-Band (Ha)"),
        SkyObject::new("M44", "Praesepe / Seimi (Beehive Cluster)", "Cnc", DsoType::OpenCluster, 8.673, 19.67, 3.7, 95.0, "Laajakaista (IRC)"),
        SkyObject::new("M45", "Plejadit / Seulaset (Pleiades)", "Tau", DsoType::ReflectionNebula, 3.790, 24.11, 1.6, 110.0, "Laajakaista (Sininen heijastus)"),
        SkyObject::new("M51", "Pyörregalaksi (Whirlpool Galaxy)", "CVn", DsoType::Galaxy, 13.498, 47.20, 8.4, 11.0, "Laajakaista (IRC)"),
        SkyObject::new("M52", "M52 Avoin tähtijoukko", "Cas", DsoType::OpenCluster, 23.404, 61.59, 7.3, 13.0, "Laajakaista (IRC)"),
        SkyObject::new("M57", "Lyyran rengassumu (Ring Nebula)", "Lyr", DsoType::PlanetaryNebula, 18.893, 33.03, 8.8, 1.4, "Dual-Band ([OIII]/Ha)"),
        SkyObject::new("M63", "Auringonkukkagalaksi (Sunflower)", "CVn", DsoType::Galaxy, 13.264, 42.03, 8.6, 12.0, "Laajakaista (IRC)"),
        SkyObject::new("M64", "Mustasilmägalaksi (Black Eye)", "Com", DsoType::Galaxy, 12.945, 21.68, 8.5, 10.0, "Laajakaista (IRC)"),
        SkyObject::new("M65", "Leijonan tripletti 1 (M65)", "Leo", DsoType::Galaxy, 11.315, 13.09, 9.3, 9.0, "Laajakaista (IRC)"),
        SkyObject::new("M66", "Leijonan tripletti 2 (M66)", "Leo", DsoType::Galaxy, 11.336, 12.99, 8.9, 9.0, "Laajakaista (IRC)"),
        SkyObject::new("M74", "Fantasmagalaksi (Phantom Galaxy)", "Psc", DsoType::Galaxy, 1.611, 15.78, 9.4, 10.5, "Laajakaista (IRC)"),
        SkyObject::new("M76", "Pieni nostopainosumu (Little Dumbbell)", "Per", DsoType::PlanetaryNebula, 1.706, 51.58, 10.1, 2.7, "Dual-Band ([OIII])"),
        SkyObject::new("M77", "Cetus A (M77)", "Cet", DsoType::Galaxy, 2.711, -0.01, 8.9, 7.1, "Laajakaista (IRC)"),
        SkyObject::new("M78", "Casperin sumu (M78)", "Ori", DsoType::ReflectionNebula, 5.779, 0.08, 8.3, 8.0, "Laajakaista (Sininen pöly)"),
        SkyObject::new("M81", "Boden galaksi (Bode's Galaxy)", "UMa", DsoType::Galaxy, 9.926, 69.07, 6.9, 27.0, "Laajakaista (IRC)"),
        SkyObject::new("M82", "Savukegalaksi (Cigar Galaxy)", "UMa", DsoType::Galaxy, 9.930, 69.68, 8.4, 11.0, "Laajakaista + Ha"),
        SkyObject::new("M92", "M92 Pallomainen tähtijoukko", "Her", DsoType::GlobularCluster, 17.285, 43.14, 6.4, 14.0, "Laajakaista (IRC)"),
        SkyObject::new("M97", "Pöllösumu (Owl Nebula)", "UMa", DsoType::PlanetaryNebula, 11.247, 55.02, 9.9, 3.4, "Dual-Band ([OIII])"),
        SkyObject::new("M101", "Tuulimyllygalaksi (Pinwheel Galaxy)", "UMa", DsoType::Galaxy, 14.054, 54.35, 7.9, 29.0, "Laajakaista (IRC)"),
        SkyObject::new("M102", "Spindel-galaksi (NGC 5866)", "Dra", DsoType::Galaxy, 15.110, 55.76, 9.9, 4.7, "Laajakaista (IRC)"),
        SkyObject::new("M103", "M103 Avoin tähtijoukko", "Cas", DsoType::OpenCluster, 1.554, 60.71, 7.4, 6.0, "Laajakaista (IRC)"),
        SkyObject::new("M104", "Sombrerogalaksi", "Vir", DsoType::Galaxy, 12.666, -11.62, 8.0, 9.0, "Laajakaista (IRC)"),
        SkyObject::new("M106", "M106 Spiraaligalaksi", "CVn", DsoType::Galaxy, 12.316, 47.30, 8.4, 19.0, "Laajakaista (IRC)"),
        SkyObject::new("M110", "Andromedan kääpiögalaksi", "And", DsoType::Galaxy, 0.673, 41.69, 8.5, 22.0, "Laajakaista (IRC)"),

        // --- TOP NGC / IC DEEP SKY TARGETS ---
        SkyObject::new("NGC 7000", "Pohjois-Amerikka -sumu (North America)", "Cyg", DsoType::EmissionNebula, 20.983, 44.53, 4.0, 120.0, "Dual-Band (Ha/[OIII])"),
        SkyObject::new("IC 5070", "Pelikaanisumu (Pelican Nebula)", "Cyg", DsoType::EmissionNebula, 20.846, 44.40, 8.0, 60.0, "Dual-Band (Ha/[OIII])"),
        SkyObject::new("NGC 6960", "Länsi-Harsosumu (Witches Broom)", "Cyg", DsoType::SupernovaRemnant, 20.762, 30.72, 7.0, 70.0, "Dual-Band ([OIII]/Ha)"),
        SkyObject::new("NGC 6992", "Itä-Harsosumu (Network Nebula)", "Cyg", DsoType::SupernovaRemnant, 20.941, 31.72, 7.0, 60.0, "Dual-Band ([OIII]/Ha)"),
        SkyObject::new("NGC 1499", "Kalifornia-sumu (California Nebula)", "Per", DsoType::EmissionNebula, 4.054, 36.42, 6.0, 145.0, "Dual-Band (Ha)"),
        SkyObject::new("NGC 2244", "Ruususumu (Rosette Nebula)", "Mon", DsoType::EmissionNebula, 6.539, 4.97, 4.8, 80.0, "Dual-Band (Ha/[OIII])"),
        SkyObject::new("IC 434", "Hevosenpääsumu (Horsehead & Flame)", "Ori", DsoType::EmissionNebula, 5.682, -2.46, 7.3, 60.0, "Dual-Band (Ha)"),
        SkyObject::new("NGC 2024", "Liekkisumu (Flame Nebula)", "Ori", DsoType::EmissionNebula, 5.698, -1.85, 7.0, 30.0, "Dual-Band (Ha)"),
        SkyObject::new("NGC 281", "Pacman-sumu", "Cas", DsoType::EmissionNebula, 0.880, 56.62, 7.4, 35.0, "Dual-Band (Ha/[OIII])"),
        SkyObject::new("IC 1805", "Sydänsumu (Heart Nebula)", "Cas", DsoType::EmissionNebula, 2.546, 61.45, 6.5, 120.0, "Dual-Band (Ha/[OIII])"),
        SkyObject::new("IC 1848", "Sielusumu (Soul Nebula)", "Cas", DsoType::EmissionNebula, 2.853, 60.41, 6.5, 100.0, "Dual-Band (Ha/[OIII])"),
        SkyObject::new("NGC 869", "Kaksoistähtijoukko (h Persei)", "Per", DsoType::OpenCluster, 2.317, 57.13, 3.7, 30.0, "Laajakaista (IRC)"),
        SkyObject::new("NGC 884", "Kaksoistähtijoukko (χ Persei)", "Per", DsoType::OpenCluster, 2.370, 57.15, 3.8, 30.0, "Laajakaista (IRC)"),
        SkyObject::new("NGC 7331", "Peurahyppygalaksi (Deer Lick)", "Peg", DsoType::Galaxy, 22.618, 34.42, 9.5, 10.0, "Laajakaista (IRC)"),
        SkyObject::new("NGC 891", "Reunagalaksi (Outer Limits)", "And", DsoType::Galaxy, 2.377, 42.35, 9.9, 13.5, "Laajakaista (IRC)"),
        SkyObject::new("NGC 253", "Veisturin galaksi (Sculptor)", "Scl", DsoType::Galaxy, 0.793, -25.29, 7.2, 27.0, "Laajakaista (IRC)"),
        SkyObject::new("IC 1396", "Elefantinkärsäsumu (Elephant's Trunk)", "Cep", DsoType::EmissionNebula, 21.613, 57.50, 3.5, 170.0, "Dual-Band (Ha)"),
        SkyObject::new("NGC 7635", "Kuplasumu (Bubble Nebula)", "Cas", DsoType::EmissionNebula, 23.346, 61.20, 8.5, 15.0, "Dual-Band ([OIII]/Ha)"),
        SkyObject::new("NGC 2359", "Thorin kypärä (Thor's Helmet)", "CMa", DsoType::EmissionNebula, 7.310, -13.20, 11.0, 14.0, "Dual-Band ([OIII])"),

        // --- BRIGHT REFERENCE STARS (Great for focus / alignment) ---
        SkyObject::new("Polaris", "Pohjantähti (Polaris)", "UMi", DsoType::Star, 2.530, 89.26, 2.0, 0.0, "Laajakaista"),
        SkyObject::new("Vega", "Vega (Lyyra)", "Lyr", DsoType::Star, 18.615, 38.78, 0.0, 0.0, "Laajakaista"),
        SkyObject::new("Deneb", "Deneb (Joutsen)", "Cyg", DsoType::Star, 20.690, 45.28, 1.25, 0.0, "Laajakaista"),
        SkyObject::new("Altair", "Altair (Kotka)", "Aql", DsoType::Star, 19.846, 8.87, 0.77, 0.0, "Laajakaista"),
        SkyObject::new("Betelgeuse", "Betelgeuse (Orionin punainen jättiläinen)", "Ori", DsoType::Star, 5.919, 7.41, 0.5, 0.0, "Laajakaista"),
        SkyObject::new("Rigel", "Rigel (Orionin sininen jättiläinen)", "Ori", DsoType::Star, 5.242, -8.20, 0.13, 0.0, "Laajakaista"),
        SkyObject::new("Sirius", "Sirius (Taivaan kirkkain tähti)", "CMa", DsoType::Star, 6.752, -16.72, -1.46, 0.0, "Laajakaista"),
        SkyObject::new("Capella", "Capella (Ajomies)", "Aur", DsoType::Star, 5.278, 45.99, 0.08, 0.0, "Laajakaista"),
        SkyObject::new("Aldebaran", "Aldebaran (Härän punainen silmä)", "Tau", DsoType::Star, 4.598, 16.51, 0.85, 0.0, "Laajakaista"),
        SkyObject::new("Arcturus", "Arcturus (Karhunvartija)", "Boo", DsoType::Star, 14.261, 19.18, -0.05, 0.0, "Laajakaista"),

        // --- SOLAR SYSTEM OBJECTS (Approximate J2026/Current Sky Positions) ---
        SkyObject::new("Jupiter", "Jupiter (Planeetta & kuut)", "Tau", DsoType::Planet, 4.85, 22.3, -2.6, 0.8, "Laajakaista (Lyhyt valotus)"),
        SkyObject::new("Saturnus", "Saturnus (Renkaat)", "Aqr", DsoType::Planet, 23.25, -6.5, 0.7, 0.3, "Laajakaista (Lyhyt valotus)"),
        SkyObject::new("Mars", "Mars (Punainen planeetta)", "Cnc", DsoType::Planet, 8.10, 21.0, -0.3, 0.2, "Laajakaista (Lyhyt valotus)"),
        SkyObject::new("Kuu", "Kuu (Moon)", "Tau", DsoType::Planet, 3.50, 18.0, -12.0, 31.0, "Laajakaista / Suodatin"),
    ]
}
