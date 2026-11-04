use chrono::{DateTime, Datelike, Timelike, Utc};
use std::f64::consts::PI;

/// Default observer location: Helsinki / Southern Finland (~60.17° N, 24.94° E)
pub const DEFAULT_LATITUDE_DEG: f64 = 60.17;
pub const DEFAULT_LONGITUDE_DEG: f64 = 24.94;

/// Calculates Julian Date from a UTC DateTime
pub fn julian_date(dt: &DateTime<Utc>) -> f64 {
    let year = dt.year() as f64;
    let month = dt.month() as f64;
    let day = dt.day() as f64;
    let hour = dt.hour() as f64 + dt.minute() as f64 / 60.0 + dt.second() as f64 / 3600.0;

    let (y, m) = if month <= 2.0 {
        (year - 1.0, month + 12.0)
    } else {
        (year, month)
    };

    let a = (y / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();

    (365.25 * (y + 4716.0)).floor() + (30.6001 * (m + 1.0)).floor() + day + b - 1524.5 + (hour / 24.0)
}

/// Calculates Greenwich Mean Sidereal Time (GMST) in hours (0.0 .. 24.0)
pub fn gmst_hours(dt: &DateTime<Utc>) -> f64 {
    let jd = julian_date(dt);
    let d = jd - 2451545.0; // days since J2000.0
    // Standard IAU formula
    let gmst_deg = 280.46061837 + 360.98564736629 * d;
    let gmst_norm = ((gmst_deg % 360.0) + 360.0) % 360.0;
    gmst_norm / 15.0
}

/// Calculates Local Sidereal Time (LST) in hours (0.0 .. 24.0)
/// Longitude in degrees (East is positive, e.g. Finland +24.94°)
pub fn lst_hours(dt: &DateTime<Utc>, lon_deg: f64) -> f64 {
    let gmst = gmst_hours(dt);
    let lst = gmst + lon_deg / 15.0;
    ((lst % 24.0) + 24.0) % 24.0
}

/// Converts Equatorial coordinates (RA in hours, Dec in degrees) to
/// Horizontal coordinates (Altitude and Azimuth in degrees).
///
/// Altitude: -90° (nadir) to +90° (zenith)
/// Azimuth: 0° (North), 90° (East), 180° (South), 270° (West)
pub fn ra_dec_to_alt_az(
    ra_hours: f64,
    dec_deg: f64,
    lat_deg: f64,
    lon_deg: f64,
    dt: &DateTime<Utc>,
) -> (f64, f64) {
    let lst = lst_hours(dt, lon_deg);
    let ha_hours = lst - ra_hours;
    let ha_rad = (ha_hours * 15.0).to_radians();

    let dec_rad = dec_deg.to_radians();
    let lat_rad = lat_deg.to_radians();

    let sin_alt = lat_rad.sin() * dec_rad.sin() + lat_rad.cos() * dec_rad.cos() * ha_rad.cos();
    let alt_rad = sin_alt.clamp(-1.0, 1.0).asin();
    let alt_deg = alt_rad.to_degrees();

    // Azimuth calculation
    let cos_alt = alt_rad.cos().max(1e-6);
    let sin_az = -dec_rad.cos() * ha_rad.sin() / cos_alt;
    let cos_az = (dec_rad.sin() - lat_rad.sin() * sin_alt) / (lat_rad.cos() * cos_alt);

    let mut az_rad = sin_az.atan2(cos_az);
    if az_rad < 0.0 {
        az_rad += 2.0 * PI;
    }
    let az_deg = az_rad.to_degrees();

    (alt_deg, az_deg)
}

/// Calculates the altitude profile for a target across the upcoming night (from 18:00 to 06:00 local time).
/// Returns 25 pairs of (hour_label, altitude_deg).
pub fn night_altitude_profile(
    ra_hours: f64,
    dec_deg: f64,
    lat_deg: f64,
    lon_deg: f64,
    reference_dt: &DateTime<Utc>,
) -> Vec<(String, f64)> {
    let mut profile = Vec::with_capacity(25);
    // Align with 18:00 local time (assume UTC+2 standard or UTC+3 DST)
    // We sample 25 points at 30-minute intervals covering 12 hours
    let start_dt = *reference_dt - chrono::Duration::hours(2);

    for i in 0..=24 {
        let sample_dt = start_dt + chrono::Duration::minutes(i * 30);
        let (alt, _) = ra_dec_to_alt_az(ra_hours, dec_deg, lat_deg, lon_deg, &sample_dt);
        let hour_str = format!("{:02}:{:02}", sample_dt.hour(), sample_dt.minute());
        profile.push((hour_str, alt));
    }

    profile
}

/// Projects Horizontal coordinates (Alt, Az) to 2D canvas coordinates
/// within a normalized circle of radius 1.0, with Zenith at (0, 0) and
/// Horizon at radius 1.0.
///
/// North is at Top (y = -1), South is at Bottom (y = +1),
/// East is at Left (x = -1), West is at Right (x = +1)
/// (looking up at the celestial dome from underneath).
pub fn alt_az_to_canvas_normalized(alt_deg: f64, az_deg: f64) -> Option<(f32, f32)> {
    if alt_deg < -5.0 {
        return None; // Below visible horizon
    }

    // Zenith angle (0 at zenith, 90 at horizon)
    let z_deg = (90.0 - alt_deg).max(0.0);
    let r = (z_deg / 90.0) as f32; // 0.0 at zenith, 1.0 at horizon

    // Azimuth: 0 = N (up, -y), 90 = E (left, -x), 180 = S (down, +y), 270 = W (right, +x)
    let az_rad = az_deg.to_radians();
    let x = -(az_rad.sin() as f32) * r;
    let y = -(az_rad.cos() as f32) * r;

    Some((x, y))
}

/// Formats RA hours as HH:MM:SS
pub fn format_ra(ra_hours: f64) -> String {
    let norm = ((ra_hours % 24.0) + 24.0) % 24.0;
    let h = norm.floor() as u32;
    let m = ((norm - h as f64) * 60.0).floor() as u32;
    let s = ((norm - h as f64 - m as f64 / 60.0) * 3600.0).round() as u32;
    format!("{:02}h {:02}m {:02}s", h, m, s)
}

/// Formats Declination degrees as +DD° MM' SS"
pub fn format_dec(dec_deg: f64) -> String {
    let sign = if dec_deg < 0.0 { '-' } else { '+' };
    let abs_dec = dec_deg.abs();
    let d = abs_dec.floor() as u32;
    let m = ((abs_dec - d as f64) * 60.0).floor() as u32;
    let s = ((abs_dec - d as f64 - m as f64 / 60.0) * 3600.0).round() as u32;
    format!("{}{:02}° {:02}' {:02}\"", sign, d, m, s)
}
