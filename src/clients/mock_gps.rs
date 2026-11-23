//! Simulated GPS client compatible with the real GPS interface.
//!
//! Port of `luracs.clients.mock_gps.MockGPS`. Simulates a detector moving in a
//! circle around a geographic center (by default Lund University, Sweden) at a
//! fixed radius and speed, generating realistic NMEA-equivalent `GpsData` fixes
//! at a fixed interval (1 Hz by default).

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::gps::GpsData;

pub const DEFAULT_CENTER_LAT: f64 = 55.589_443_533_631_39;
pub const DEFAULT_CENTER_LON: f64 = 13.001_056_311_555_528;
pub const DEFAULT_RADIUS_METERS: f64 = 25.0;
pub const DEFAULT_SPEED_MPS: f64 = 1.0;
pub const DEFAULT_UPDATE_INTERVAL: Duration = Duration::from_millis(1000);
pub const METERS_PER_DEGREE_LAT: f64 = 111_320.0;

#[derive(Debug, Clone)]
pub struct MockGps {
    pub center_lat: f64,
    pub center_lon: f64,
    pub radius: f64,
    pub speed: f64,
    pub update_interval: Duration,

    start_time: Option<Instant>,
    last_update: Option<Instant>,
    angle: f64,
    packets_received: u64,
    data: GpsData,
    is_open: bool,
}

impl Default for MockGps {
    fn default() -> Self {
        Self::new()
    }
}

impl MockGps {
    /// Create a simulated GPS client with default Lund University coordinates.
    pub fn new() -> Self {
        Self::with_params(
            DEFAULT_CENTER_LAT,
            DEFAULT_CENTER_LON,
            DEFAULT_RADIUS_METERS,
            DEFAULT_SPEED_MPS,
            DEFAULT_UPDATE_INTERVAL,
        )
    }

    /// Create a simulated GPS client with custom circular orbit parameters.
    pub fn with_params(
        center_lat: f64,
        center_lon: f64,
        radius: f64,
        speed: f64,
        update_interval: Duration,
    ) -> Self {
        let now_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let initial_data = GpsData {
            latitude: Some(center_lat),
            longitude: Some(center_lon),
            altitude: Some(35.0),
            speed: Some(0.0),
            course: Some(0.0),
            satellites: 12,
            satellites_in_view: 16,
            hdop: Some(0.8),
            vdop: Some(1.2),
            pdop: Some(1.5),
            fix_type: 3,
            valid: true,
            update_rate: 1.0,
            source: "Mock GPS".to_string(),
            timestamp: now_ts,
        };

        Self {
            center_lat,
            center_lon,
            radius,
            speed,
            update_interval,
            start_time: None,
            last_update: None,
            angle: 0.0,
            packets_received: 0,
            data: initial_data,
            is_open: false,
        }
    }

    /// Start the GPS simulation. Matches Python's `open()`.
    pub fn open(&mut self) -> bool {
        if self.is_open {
            return true;
        }

        let now = Instant::now();
        self.start_time = Some(now);
        self.last_update = Some(now);
        self.packets_received = 0;
        self.angle = 0.0;
        self.is_open = true;

        self.update_position(now);
        true
    }

    /// Stop the GPS simulation. Matches Python's `close()`.
    pub const fn close(&mut self) {
        self.is_open = false;
        self.start_time = None;
        self.last_update = None;
    }

    pub const fn is_open(&self) -> bool {
        self.is_open
    }

    pub const fn data(&self) -> &GpsData {
        &self.data
    }

    pub const fn packets_received(&self) -> u64 {
        self.packets_received
    }

    /// Advance the simulation if `update_interval` has elapsed.
    /// Returns `true` if a new GPS fix was generated this frame.
    pub fn poll(&mut self) -> bool {
        if !self.is_open {
            return false;
        }

        let now = Instant::now();
        let should_update = match self.last_update {
            Some(last) => now.duration_since(last) >= self.update_interval,
            None => true,
        };

        if should_update {
            self.last_update = Some(now);
            self.update_position(now);
            true
        } else {
            false
        }
    }

    fn update_position(&mut self, now: Instant) {
        let elapsed = self
            .start_time
            .map_or(0.0, |t| now.duration_since(t).as_secs_f64());

        // Angular velocity (radians/second)
        let angular_velocity = if self.radius > 0.0 {
            self.speed / self.radius
        } else {
            0.0
        };

        // Circular movement
        self.angle = angular_velocity * elapsed;

        // Position relative to circle center
        let east = self.radius * self.angle.cos();
        let north = self.radius * self.angle.sin();

        // Convert meters to geographic coordinates
        let latitude = self.center_lat + north / METERS_PER_DEGREE_LAT;
        let lat_rad = self.center_lat.to_radians();
        let meters_per_deg_lon = METERS_PER_DEGREE_LAT * lat_rad.cos();
        let longitude = if meters_per_deg_lon.abs() > 1e-9 {
            self.center_lon + east / meters_per_deg_lon
        } else {
            self.center_lon
        };

        // Calculate direction of movement (degrees from north)
        let course = (-self.angle.sin())
            .atan2(self.angle.cos())
            .to_degrees()
            .rem_euclid(360.0);

        self.packets_received += 1;
        let update_rate = self.packets_received as f64 / elapsed.max(1.0);

        // Small simulated altitude variation
        let altitude = 1.5f64.mul_add((self.angle * 2.0).sin(), 35.0);

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.data = GpsData {
            latitude: Some(latitude),
            longitude: Some(longitude),
            altitude: Some(altitude),
            speed: Some(self.speed),
            course: Some(course),
            satellites: 12,
            satellites_in_view: 16,
            hdop: Some(0.8),
            vdop: Some(1.2),
            pdop: Some(1.5),
            fix_type: 3,
            valid: true,
            update_rate,
            source: "Mock GPS".to_string(),
            timestamp,
        };
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_gps_lifecycle() {
        let mut mock = MockGps::new();
        assert!(!mock.is_open());

        assert!(mock.open());
        assert!(mock.is_open());
        assert_eq!(mock.packets_received(), 1);

        let data = mock.data();
        assert!(data.valid);
        assert_eq!(data.source, "Mock GPS");
        assert!(data.latitude.is_some());
        assert!(data.longitude.is_some());

        // Calling poll immediately should return false (interval not reached)
        assert!(!mock.poll());

        mock.close();
        assert!(!mock.is_open());
        assert!(!mock.poll());
    }

    #[test]
    fn test_mock_gps_circular_motion() {
        let mut mock = MockGps::with_params(
            55.0,
            13.0,
            25.0,
            1.0,
            Duration::from_millis(10),
        );
        mock.open();

        let lat0 = mock.data().latitude.unwrap();
        let lon0 = mock.data().longitude.unwrap();

        // At angle 0 (start): east = radius, north = 0
        // lat should equal center_lat
        assert!((lat0 - 55.0).abs() < 1e-6);
        // lon should be greater than center_lon
        assert!(lon0 > 13.0);
    }
}
