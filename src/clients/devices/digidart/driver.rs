//! High-level `DeviceDriver` and `HvControl` implementation for ORTEC digiDART.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ndarray::Array1;

use super::protocol::{self, Opcode, DEFAULT_CHANNELS};
use crate::clients::device_wrapper_base::{
    ConnectionType, SupportedSetting, WrappedRealTimePackage, WrappedSpectrumPackage,
    WrappedStatusPackage,
};
use crate::clients::driver::{CpsEstimator, DeviceDriver, DeviceError, DeviceInfo, HvControl};
use crate::clients::link::DeviceLink;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

/// ORTEC digiDART protocol driver.
pub struct DigiDartDriver {
    link: Arc<dyn DeviceLink>,
    device_name: String,
    channels: AtomicUsize,
    io_lock: tokio::sync::Mutex<()>,
    cps_estimator: Mutex<CpsEstimator>,
    hv_ramping: AtomicBool,
}

impl DigiDartDriver {
    /// Construct a new digiDART driver wrapping an active `DeviceLink`.
    pub fn new(link: Arc<dyn DeviceLink>, device_name: impl Into<String>) -> Self {
        Self {
            link,
            device_name: device_name.into(),
            channels: AtomicUsize::new(DEFAULT_CHANNELS),
            io_lock: tokio::sync::Mutex::new(()),
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

    async fn read_spectrum_and_times_locked(&self) -> Result<(Vec<f64>, f64, f64), DeviceError> {
        let count = self.channels.load(Ordering::Relaxed);
        let counts = protocol::read_spectrum(&*self.link, 0, count, DEFAULT_TIMEOUT).await?;
        let live_ticks =
            protocol::query_u32_1(&*self.link, "SHOW_LIVE", Opcode::ShowLive, DEFAULT_TIMEOUT)
                .await?;
        let true_ticks =
            protocol::query_u32_1(&*self.link, "SHOW_TRUE", Opcode::ShowTrue, DEFAULT_TIMEOUT)
                .await?;

        let live_time = f64::from(live_ticks) * 0.020;
        let real_time = f64::from(true_ticks) * 0.020;
        Ok((counts, live_time, real_time))
    }
}

#[async_trait]
impl DeviceDriver for DigiDartDriver {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            name: self.device_name.clone(),
            model: "digiDART".into(),
            channels: self.channels.load(Ordering::Relaxed),
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
        let (hv_actual, _) = protocol::query_u32_2(
            &*self.link,
            "SHOW_HV_ACTUAL",
            Opcode::ShowHvActual,
            DEFAULT_TIMEOUT,
        )
        .await?;

        if hv_actual < 10 {
            return Ok(WrappedRealTimePackage {
                cps: 0.0,
                dr: f64::NAN,
                timestamp: Self::current_timestamp(),
                ..Default::default()
            });
        }

        let (counts, live_time, _) = self.read_spectrum_and_times_locked().await?;
        drop(_io);

        let total_counts: f64 = counts.iter().sum();
        let cps = {
            let mut est = self.cps_estimator.lock().unwrap();
            est.update(total_counts, live_time).unwrap_or(0.0)
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
        let (hv_target, _) = protocol::query_u32_2(
            &*self.link,
            "SHOW_HV_TARGET",
            Opcode::ShowHvTarget,
            DEFAULT_TIMEOUT,
        )
        .await?;
        let (hv_actual, _) = protocol::query_u32_2(
            &*self.link,
            "SHOW_HV_ACTUAL",
            Opcode::ShowHvActual,
            DEFAULT_TIMEOUT,
        )
        .await?;
        let lld = protocol::query_u32_1(&*self.link, "SHOW_LLD", Opcode::ShowLld, DEFAULT_TIMEOUT)
            .await?;
        let fine_raw = protocol::query_u32_1(
            &*self.link,
            "SHOW_GAIN_FINE",
            Opcode::ShowGainFine,
            DEFAULT_TIMEOUT,
        )
        .await?;
        drop(_io);

        Ok(WrappedStatusPackage {
            voltage: f64::from(hv_actual),
            desired_voltage: f64::from(hv_target),
            lower_level_discriminator: f64::from(lld),
            upper_level_discriminator: f64::NAN,
            fine_gain: protocol::decode_fine_gain(fine_raw),
            connection_type: Some(ConnectionType::Usb),
            timestamp: Self::current_timestamp(),
            ..Default::default()
        })
    }

    async fn read_spectrum(&self) -> Result<WrappedSpectrumPackage, DeviceError> {
        let _io = self.io_lock.lock().await;
        let (hv_actual, _) = protocol::query_u32_2(
            &*self.link,
            "SHOW_HV_ACTUAL",
            Opcode::ShowHvActual,
            DEFAULT_TIMEOUT,
        )
        .await?;
        if hv_actual < 10 || self.hv_ramping.load(Ordering::Relaxed) {
            return Ok(WrappedSpectrumPackage {
                y_axis: Array1::zeros(self.channels.load(Ordering::Relaxed)),
                live_time: 0.0,
                real_time: Some(0.0),
                calib_coeff: None,
                timestamp: Self::current_timestamp(),
            });
        }

        let (counts, live_time, real_time) = self.read_spectrum_and_times_locked().await?;
        drop(_io);

        Ok(WrappedSpectrumPackage {
            y_axis: Array1::from_vec(counts),
            live_time,
            real_time: Some(real_time),
            calib_coeff: None,
            timestamp: Self::current_timestamp(),
        })
    }

    async fn on_reconnect(&self) -> Result<(), DeviceError> {
        let _io = self.io_lock.lock().await;

        let conv_gain = protocol::query_u32_1(
            &*self.link,
            "SHOW_GAIN_CONVERSION",
            Opcode::ShowGainConversion,
            DEFAULT_TIMEOUT,
        )
        .await?;
        if conv_gain > 0 {
            self.channels.store(conv_gain as usize, Ordering::Relaxed);
        }

        protocol::cmd_checked(
            &*self.link,
            "CLEAR",
            Opcode::Clear,
            &[0, 0x8000],
            DEFAULT_TIMEOUT,
        )
        .await?;
        // START may return non-zero micro warning bits (0x08/0x10/0x20)
        let (macro_err, _) =
            protocol::cmd(&*self.link, Opcode::Start, &[], DEFAULT_TIMEOUT).await?;
        if macro_err != 0 {
            return Err(DeviceError::Hardware(format!(
                "digiDART START error macro=0x{macro_err:02x}"
            )));
        }
        drop(_io);

        let mut est = self.cps_estimator.lock().unwrap();
        est.reset();
        drop(est);

        Ok(())
    }

    async fn start_acquisition(&self) -> Result<(), DeviceError> {
        let _io = self.io_lock.lock().await;
        let (macro_err, _) =
            protocol::cmd(&*self.link, Opcode::Start, &[], DEFAULT_TIMEOUT).await?;
        if macro_err != 0 {
            return Err(DeviceError::Hardware(format!(
                "digiDART START error macro=0x{macro_err:02x}"
            )));
        }
        Ok(())
    }

    async fn stop_acquisition(&self) -> Result<(), DeviceError> {
        let _io = self.io_lock.lock().await;
        protocol::cmd_checked(&*self.link, "STOP", Opcode::Stop, &[], DEFAULT_TIMEOUT).await
    }

    async fn reset_spectrum(&self) -> Result<(), DeviceError> {
        let _io = self.io_lock.lock().await;
        protocol::cmd_checked(
            &*self.link,
            "CLEAR",
            Opcode::Clear,
            &[0, 0x8000],
            DEFAULT_TIMEOUT,
        )
        .await?;
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
impl HvControl for DigiDartDriver {
    async fn set_hv_enabled(&self, enabled: bool) -> Result<(), DeviceError> {
        self.hv_ramping.store(true, Ordering::Relaxed);
        let res = async {
            let _io = self.io_lock.lock().await;
            if enabled {
                protocol::cmd_checked(
                    &*self.link,
                    "ENABLE_HV",
                    Opcode::EnableHv,
                    &[],
                    DEFAULT_TIMEOUT,
                )
                .await
            } else {
                protocol::cmd_checked(
                    &*self.link,
                    "DISABLE_HV",
                    Opcode::DisableHv,
                    &[],
                    DEFAULT_TIMEOUT,
                )
                .await
            }
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
            protocol::cmd_checked(
                &*self.link,
                "SET_HV",
                Opcode::SetHv,
                &[volts as u32],
                DEFAULT_TIMEOUT,
            )
            .await
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
        protocol::cmd_checked(
            &*self.link,
            "SET_LLD",
            Opcode::SetLld,
            &[lld as u32],
            DEFAULT_TIMEOUT,
        )
        .await
    }

    async fn set_uld(&self, uld: f64) -> Result<(), DeviceError> {
        let _io = self.io_lock.lock().await;
        protocol::cmd_checked(
            &*self.link,
            "SET_ULD",
            Opcode::SetUld,
            &[uld as u32],
            DEFAULT_TIMEOUT,
        )
        .await
    }

    async fn set_fine_gain(&self, gain: f64) -> Result<(), DeviceError> {
        let raw = protocol::encode_fine_gain(gain)?;
        let _io = self.io_lock.lock().await;
        protocol::cmd_checked(
            &*self.link,
            "SET_GAIN_FINE",
            Opcode::SetGainFine,
            &[raw],
            DEFAULT_TIMEOUT,
        )
        .await
    }
}
