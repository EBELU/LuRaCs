//! High-level `DeviceDriver` and `HvControl` implementation for ORTEC/AMETEK digiBase.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ndarray::Array1;

use super::protocol::{self, StatusRegister, NUM_CHANNELS};
use crate::clients::device_wrapper_base::{
    ConnectionType, SupportedSetting, WrappedRealTimePackage, WrappedSpectrumPackage,
    WrappedStatusPackage,
};
use crate::clients::driver::{CpsEstimator, DeviceDriver, DeviceError, DeviceInfo, HvControl};
use crate::clients::link::DeviceLink;

const DEFAULT_TIMEOUT: Duration = Duration::from_millis(500);

/// ORTEC/AMETEK digiBase protocol driver.
pub struct DigiBaseDriver {
    link: Arc<dyn DeviceLink>,
    device_name: String,
    io_lock: tokio::sync::Mutex<()>,
    status_reg: Mutex<StatusRegister>,
    cps_estimator: Mutex<CpsEstimator>,
    hv_ramping: AtomicBool,
}

impl DigiBaseDriver {
    /// Construct a new digiBase driver wrapping an active `DeviceLink`.
    pub fn new(link: Arc<dyn DeviceLink>, device_name: impl Into<String>) -> Self {
        Self {
            link,
            device_name: device_name.into(),
            io_lock: tokio::sync::Mutex::new(()),
            status_reg: Mutex::new(StatusRegister::default()),
            cps_estimator: Mutex::new(CpsEstimator::new()),
            hv_ramping: AtomicBool::new(false),
        }
    }

    #[must_use]
    pub fn link(&self) -> &Arc<dyn DeviceLink> {
        &self.link
    }

    fn current_timestamp() -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
    }

    async fn sync_read_status(&self) -> Result<StatusRegister, DeviceError> {
        let reg = protocol::read_status_register(&*self.link, DEFAULT_TIMEOUT).await?;
        let mut guard = self.status_reg.lock().unwrap();
        *guard = reg.clone();
        drop(guard);
        Ok(reg)
    }

    async fn sync_write_status(&self, reg: &StatusRegister) -> Result<(), DeviceError> {
        protocol::write_status_register(&*self.link, reg, DEFAULT_TIMEOUT).await?;
        let mut guard = self.status_reg.lock().unwrap();
        *guard = reg.clone();
        drop(guard);
        Ok(())
    }

    /// Trigger HV ADC readback (toggle bit 610) and read the updated status register.
    async fn read_hv_adc_locked(&self) -> Result<StatusRegister, DeviceError> {
        let mut reg = {
            let guard = self.status_reg.lock().unwrap();
            guard.clone()
        };
        reg.set_bit(610, false);
        self.sync_write_status(&reg).await?;
        reg.set_bit(610, true);
        self.sync_write_status(&reg).await?;
        tokio::time::sleep(Duration::from_millis(10)).await;
        self.sync_read_status().await
    }

    async fn clear_counters_locked(&self) -> Result<(), DeviceError> {
        let mut reg = {
            let guard = self.status_reg.lock().unwrap();
            guard.clone()
        };
        reg.set_bit(608, true);
        self.sync_write_status(&reg).await?;
        reg.set_bit(608, false);
        self.sync_write_status(&reg).await
    }
}

#[async_trait]
impl DeviceDriver for DigiBaseDriver {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            name: self.device_name.clone(),
            model: "digiBase".into(),
            channels: NUM_CHANNELS,
            connection_type: ConnectionType::Usb,
            supported_settings: vec![SupportedSetting::HvAndAmp],
        }
    }

    async fn read_realtime(&self) -> Result<WrappedRealTimePackage, DeviceError> {
        if self.hv_ramping.load(Ordering::Relaxed) {
            return Ok(WrappedRealTimePackage {
                cps: 0.0,
                dr: f64::NAN,
                timestamp: Self::current_timestamp(),
                ..Default::default()
            });
        }

        let _io = self.io_lock.lock().await;
        let reg = self.read_hv_adc_locked().await?;
        if reg.hv_readback() < 10.0 {
            return Ok(WrappedRealTimePackage {
                cps: 0.0,
                dr: f64::NAN,
                timestamp: Self::current_timestamp(),
                ..Default::default()
            });
        }

        let counts = protocol::read_spectrum_counts(&*self.link, DEFAULT_TIMEOUT).await?;
        let reg = self.sync_read_status().await?;
        drop(_io);

        let total_counts: f64 = counts.iter().sum();
        let livetime = reg.livetime_secs();

        let cps = {
            let mut est = self.cps_estimator.lock().unwrap();
            est.update(total_counts, livetime).unwrap_or(0.0)
        };

        Ok(WrappedRealTimePackage {
            cps,
            dr: f64::NAN,
            timestamp: Self::current_timestamp(),
            ..Default::default()
        })
    }

    async fn read_status(&self) -> Result<WrappedStatusPackage, DeviceError> {
        let _io = self.io_lock.lock().await;
        let reg = self.read_hv_adc_locked().await?;
        drop(_io);

        Ok(WrappedStatusPackage {
            voltage: reg.hv_readback(),
            desired_voltage: reg.hv_target(),
            lower_level_discriminator: f64::from(reg.lld()),
            upper_level_discriminator: f64::from(reg.uld()),
            fine_gain: reg.fine_gain(),
            connection_type: Some(ConnectionType::Usb),
            timestamp: Self::current_timestamp(),
            ..Default::default()
        })
    }

    async fn read_spectrum(&self) -> Result<WrappedSpectrumPackage, DeviceError> {
        let _io = self.io_lock.lock().await;
        let counts = protocol::read_spectrum_counts(&*self.link, DEFAULT_TIMEOUT).await?;
        let reg = self.sync_read_status().await?;
        drop(_io);

        Ok(WrappedSpectrumPackage {
            y_axis: Array1::from_vec(counts),
            live_time: reg.livetime_secs(),
            real_time: Some(reg.realtime_secs()),
            calib_coeff: None,
            timestamp: Self::current_timestamp(),
        })
    }

    async fn on_reconnect(&self) -> Result<(), DeviceError> {
        let _io = self.io_lock.lock().await;
        let _ = self.sync_read_status().await?;
        self.clear_counters_locked().await?;
        protocol::clear_spectrum(&*self.link, DEFAULT_TIMEOUT).await?;

        let mut reg = {
            let guard = self.status_reg.lock().unwrap();
            guard.clone()
        };
        reg.set_acq_running(true);
        self.sync_write_status(&reg).await?;
        drop(_io);

        let mut est = self.cps_estimator.lock().unwrap();
        est.reset();
        drop(est);

        Ok(())
    }

    async fn start_acquisition(&self) -> Result<(), DeviceError> {
        let _io = self.io_lock.lock().await;
        let mut reg = self.sync_read_status().await?;
        reg.set_acq_running(true);
        self.sync_write_status(&reg).await
    }

    async fn stop_acquisition(&self) -> Result<(), DeviceError> {
        let _io = self.io_lock.lock().await;
        let mut reg = self.sync_read_status().await?;
        reg.set_acq_running(false);
        self.sync_write_status(&reg).await
    }

    async fn reset_spectrum(&self) -> Result<(), DeviceError> {
        let _io = self.io_lock.lock().await;
        self.clear_counters_locked().await?;
        protocol::clear_spectrum(&*self.link, DEFAULT_TIMEOUT).await?;
        drop(_io);

        let mut est = self.cps_estimator.lock().unwrap();
        est.reset();
        drop(est);

        Ok(())
    }

    fn hv_control(&self) -> Option<&dyn HvControl> {
        Some(self)
    }
}

#[async_trait]
impl HvControl for DigiBaseDriver {
    async fn set_hv_enabled(&self, enabled: bool) -> Result<(), DeviceError> {
        self.hv_ramping.store(true, Ordering::Relaxed);
        let res = async {
            let _io = self.io_lock.lock().await;
            let mut reg = self.sync_read_status().await?;
            if !enabled {
                reg.set_hv_target(0.0)?;
            }
            reg.set_hv_enabled(enabled);
            self.sync_write_status(&reg).await
        }
        .await;
        self.hv_ramping.store(false, Ordering::Relaxed);
        if res.is_ok() {
            self.reset_spectrum().await?;
        }
        res
    }

    async fn set_hv(&self, volts: f64) -> Result<(), DeviceError> {
        self.hv_ramping.store(true, Ordering::Relaxed);
        let res = async {
            let _io = self.io_lock.lock().await;
            let mut reg = self.sync_read_status().await?;
            reg.set_hv_target(volts)?;
            self.sync_write_status(&reg).await
        }
        .await;
        self.hv_ramping.store(false, Ordering::Relaxed);
        if res.is_ok() {
            self.reset_spectrum().await?;
        }
        res
    }

    async fn set_lld(&self, lld: f64) -> Result<(), DeviceError> {
        let _io = self.io_lock.lock().await;
        let mut reg = self.sync_read_status().await?;
        reg.set_lld(lld as u32);
        self.sync_write_status(&reg).await
    }

    async fn set_uld(&self, uld: f64) -> Result<(), DeviceError> {
        let _io = self.io_lock.lock().await;
        let mut reg = self.sync_read_status().await?;
        reg.set_uld(uld as u32);
        self.sync_write_status(&reg).await
    }

    async fn set_fine_gain(&self, gain: f64) -> Result<(), DeviceError> {
        let _io = self.io_lock.lock().await;
        let mut reg = self.sync_read_status().await?;
        reg.set_fine_gain(gain)?;
        self.sync_write_status(&reg).await
    }
}
