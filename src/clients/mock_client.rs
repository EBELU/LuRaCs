//! Mock device wrapper for development and testing.
//!
//! Port of `luracs.clients.mock_client.MockClientWrapper`. Registered under
//! `"mock"`, connected by `--debug`. Emits a synthetic Cs-137 spectrum
//! (accumulating across samples), Poisson-like CPS values, and fixed status.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ndarray::Array1;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::{Rng, RngExt};
use rand_distr::weighted::WeightedIndex;
use rand_distr::{Distribution, Normal};

use super::device_wrapper_base::{
    ConnectionType, DeviceAddress, DeviceError, DeviceState, DeviceWrapper, SupportedSetting,
    WrappedRealTimePackage, WrappedSpectrumPackage, WrappedStatusPackage, register_device,
};

// ---------------------------------------------------------------------------
// Cs-137 template
// ---------------------------------------------------------------------------
//
// Python: `cs137temp = np.array([...]) + 1`. The `+1` guarantees every bin
// has nonzero probability (needed by `np.random.choice`). We keep the raw
// counts and add 1 at sample time.

const CS137_RAW: [u32; 128] = [
    32, 47, 49, 68, 70, 63, 69, 69, 70, 81, 78, 64, 52, 44, 39, 35, 32, 30, 29, 28, 28, 27, 25, 19,
    13, 8, 6, 5, 5, 5, 9, 21, 40, 44, 27, 10, 4, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

fn cs137_template() -> &'static [f64; 128] {
    static TEMPLATE: OnceLock<[f64; 128]> = OnceLock::new();
    TEMPLATE.get_or_init(|| {
        let mut out = [0.0f64; 128];
        for (i, &v) in CS137_RAW.iter().enumerate() {
            out[i] = (v + 1) as f64;
        }
        out
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Generate one 20–30 sample sparse spectrum over 1024 channels.
///
/// Each sample lands in one of 8 channels under a Cs-137 template bin,
/// chosen with probability proportional to that bin's template value.
/// Accumulated per call, so the total spectrum gradually fills in.
fn sample_sparse_spectrum(rng: &mut StdRng) -> Array1<f64> {
    let template = cs137_template();

    // Normalise the 128-bin probability distribution.
    let mut total = 0.0_f64;
    for &v in template.iter() {
        total += v;
    }
    let probs: Vec<f64> = template.iter().map(|&v| v / total).collect();
    let dist = WeightedIndex::new(&probs)
        .expect("CS-137 template weights are all positive (raw counts + 1)");

    // Random sample count 20..=30 (Python: np.random.randint(20, 31)).
    let n: usize = rng.random_range(20..=30);

    let mut spectrum = Array1::<f64>::zeros(1024);
    for _ in 0..n {
        let bin = dist.sample(rng);
        let offset: usize = rng.random_range(0..8);
        spectrum[bin * 8 + offset] += 1.0;
    }
    spectrum
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct MockState {
    /// Poll-loop control flag. Set by `start()`, cleared by `stop()`.
    running: bool,
    /// Lifecycle flag. Only true after `stop()`, matching Python's
    /// `MockClient._stopped`.
    stopped: bool,
    /// Accumulated spectrum across all samples.
    spect_buf: Array1<f64>,
    /// Used for `uptime` (Python's `MockClient.start_ts`).
    start_ts: Instant,
    /// RNG, seeded once so the mock is deterministic-ish across a session.
    rng: StdRng,
}

impl MockState {
    fn new() -> Self {
        Self {
            running: false,
            stopped: false,
            spect_buf: Array1::zeros(1024),
            start_ts: Instant::now(),
            rng: StdRng::from_seed([0; 32]),
        }
    }
}

// ---------------------------------------------------------------------------
// MockDevice
// ---------------------------------------------------------------------------

const MOCK_SUPPORTED: [SupportedSetting; 2] =
    [SupportedSetting::Calibration, SupportedSetting::HvAndAmp];

pub struct MockDevice {
    name: String,
    connection: ConnectionType,
    channels: usize,
    state: AtomicU8,
    connected_timestamp: f64,
    mock: Mutex<MockState>,
}

impl MockDevice {
    pub fn new(_address: DeviceAddress, connection: ConnectionType) -> Self {
        // Python ignores the address (`self.name = "MockClient"` regardless)
        // and only stores the connection type.
        Self {
            name: "MockClient".to_string(),
            connection,
            channels: 1024,
            state: AtomicU8::new(DeviceState::Uninitialized as u8),
            connected_timestamp: now_secs(),
            mock: Mutex::new(MockState::new()),
        }
    }

    // ---- Sync snapshot helpers -----------------------------------------

    fn snapshot_realtime(&self) -> WrappedRealTimePackage {
        let mut state = self.mock.lock().unwrap();
        let cps: f64 = Normal::new(25.0, 5.0)
            .expect("std_dev > 0")
            .sample(&mut state.rng);
        let dr = cps / 250.0
            + Normal::new(0.0, 0.01)
                .expect("std_dev > 0")
                .sample(&mut state.rng);
        WrappedRealTimePackage {
            cps,
            dr,
            timestamp: now_secs(),
            ..Default::default()
        }
    }

    fn snapshot_status(&self) -> WrappedStatusPackage {
        WrappedStatusPackage {
            battery: Some(100),
            temperature: 22.5,
            charging: false,
            timestamp: now_secs(),
            ..Default::default()
        }
    }

    fn snapshot_spectrum(&self) -> WrappedSpectrumPackage {
        let mut state = self.mock.lock().unwrap();
        let sample = sample_sparse_spectrum(&mut state.rng);
        state.spect_buf += &sample;
        let uptime = state.start_ts.elapsed().as_secs_f64();
        WrappedSpectrumPackage {
            y_axis: state.spect_buf.clone(),
            live_time: uptime,
            real_time: None,
            calib_coeff: Some(vec![0.0003705, 2.3694975, 4.2583089]),
            timestamp: now_secs(),
        }
    }
}

#[async_trait]
impl DeviceWrapper for MockDevice {
    fn type_key(&self) -> &'static str {
        "mock"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn connection(&self) -> ConnectionType {
        self.connection
    }

    fn channels(&self) -> Option<usize> {
        Some(self.channels)
    }

    fn state(&self) -> DeviceState {
        DeviceState::from_u8(self.state.load(Ordering::Relaxed))
    }

    fn set_state(&self, state: DeviceState) {
        self.state.store(state as u8, Ordering::Relaxed);
    }

    fn connected_timestamp(&self) -> f64 {
        self.connected_timestamp
    }

    fn is_running(&self) -> bool {
        self.mock.lock().unwrap().running
    }

    fn is_stopped(&self) -> bool {
        self.mock.lock().unwrap().stopped
    }

    async fn get_realtime_data(&self) -> Result<WrappedRealTimePackage, DeviceError> {
        Ok(self.snapshot_realtime())
    }

    async fn get_status(&self) -> Result<WrappedStatusPackage, DeviceError> {
        Ok(self.snapshot_status())
    }

    async fn get_spectrum(&self) -> Result<WrappedSpectrumPackage, DeviceError> {
        Ok(self.snapshot_spectrum())
    }

    async fn start(&self) -> Result<(), DeviceError> {
        self.mock.lock().unwrap().running = true;
        Ok(())
    }

    async fn stop(&self) -> Result<(), DeviceError> {
        let mut state = self.mock.lock().unwrap();
        state.running = false;
        state.stopped = true;
        Ok(())
    }

    fn supported_settings(&self) -> &'static [SupportedSetting] {
        &MOCK_SUPPORTED
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register the mock device under `"mock"`. Call once from `main()` before
/// any `--debug` dispatch. Idempotent — re-registration replaces the entry.
pub fn register_mock_device() {
    register_device("mock", |addr, conn| {
        Ok(Box::new(MockDevice::new(addr, conn)))
    });
}
