use std::sync::Arc;

use super::from_link;
use super::protocol::{StatusRegister, NUM_CHANNELS};
use crate::clients::driver::DeviceDriver;
use crate::clients::link::MockLink;

#[test]
fn test_status_register_bitfields() {
    let mut reg = StatusRegister::default();

    reg.set_acq_running(true);
    assert!(reg.acq_running());

    reg.set_hv_enabled(true);
    assert!(reg.hv_enabled());

    reg.set_hv_target(800.0).unwrap();
    assert!((reg.hv_target() - 800.0).abs() < 1e-6);
    assert!(reg.set_hv_target(1250.0).is_err());

    reg.set_lld(42);
    assert_eq!(reg.lld(), 42);

    reg.set_uld(900);
    assert_eq!(reg.uld(), 900);

    reg.set_fine_gain(1.25).unwrap();
    assert!(reg.set_fine_gain(2.5).is_err());

    // Livetime = 250 ticks = 5.0 seconds
    reg.set_bits(224, 256, 250);
    assert!((reg.livetime_secs() - 5.0).abs() < 1e-6);

    // Realtime = 300 ticks = 6.0 seconds
    reg.set_bits(288, 320, 300);
    assert!((reg.realtime_secs() - 6.0).abs() < 1e-6);

    // HV ADC readback: low 8 bits = 0x90 (144), high 2 bits = 0x02 (512) => 656 * 1.25 = 820.0V
    reg.set_bits(24, 32, 0x90);
    reg.set_bits(13, 15, 0x02);
    assert!((reg.hv_readback() - 820.0).abs() < 1e-6);
}

#[tokio::test]
async fn test_digibase_driver_spectrum_and_realtime_cps() {
    let mock = Arc::new(MockLink::usb_mock("USB:0a2d:000f"));
    let driver = from_link(mock.clone(), "digiBase-test");

    // Helper to build a status register with HV=800V readback & given livetime ticks
    let make_reg = |live_ticks: u32| -> Vec<u8> {
        let mut r = StatusRegister::default();
        // HV readback 800V => 640 => low=0x80, high=0x02
        r.set_bits(24, 32, 0x80);
        r.set_bits(13, 15, 0x02);
        r.set_hv_target(800.0).unwrap();
        r.set_bits(224, 256, live_ticks);
        r.set_bits(288, 320, live_ticks + 10);
        r.as_bytes().to_vec()
    };

    let make_spec = |total_counts: u32| -> Vec<u8> {
        let mut spec = vec![0u8; NUM_CHANNELS * 4];
        spec[0..4].copy_from_slice(&total_counts.to_le_bytes());
        spec
    };

    // Sample 1: read_realtime does:
    //   write_status (bit 610=0) -> ack, write_status (bit 610=1) -> ack, read_status -> reg1,
    //   read_spectrum -> spec1, read_status -> reg1
    mock.push_incoming(vec![0u8]); // ack 1
    mock.push_incoming(vec![0u8]); // ack 2
    mock.push_incoming(make_reg(500)); // 10.0s livetime
    mock.push_incoming(make_spec(1000));
    mock.push_incoming(make_reg(500));

    let rt1 = driver.read_realtime().await.unwrap();
    // First observation establishes baseline -> 0.0 CPS
    assert!((rt1.cps - 0.0).abs() < 1e-6);

    // Sample 2: 2.0s later (600 ticks = 12.0s), +200 counts -> 100.0 CPS
    mock.push_incoming(vec![0u8]);
    mock.push_incoming(vec![0u8]);
    mock.push_incoming(make_reg(600));
    mock.push_incoming(make_spec(1200));
    mock.push_incoming(make_reg(600));

    let rt2 = driver.read_realtime().await.unwrap();
    assert!((rt2.cps - 100.0).abs() < 1e-4);

    // read_spectrum
    mock.push_incoming(make_spec(1200));
    mock.push_incoming(make_reg(600));
    let spec = driver.read_spectrum().await.unwrap();
    assert_eq!(spec.y_axis.len(), NUM_CHANNELS);
    assert!((spec.y_axis[0] - 1200.0).abs() < 1e-6);
    assert!((spec.live_time - 12.0).abs() < 1e-6);
}

#[tokio::test]
async fn test_digibase_hv_control() {
    let mock = Arc::new(MockLink::usb_mock("USB:0a2d:000f"));
    let driver = from_link(mock.clone(), "digiBase-hv");
    let hv = driver.hv_control().expect("digiBase supports HvControl");

    let reg_bytes = StatusRegister::default().as_bytes().to_vec();
    // set_lld: sync_read_status -> reg, sync_write_status -> ack
    mock.push_incoming(reg_bytes);
    mock.push_incoming(vec![0u8]);
    hv.set_lld(25.0).await.unwrap();

    let sent = mock.sent_packets();
    assert_eq!(sent[0], vec![0x01]);
    assert_eq!(sent[1][0], 0x00);
    let written = StatusRegister::from_slice(&sent[1][1..]).unwrap();
    assert_eq!(written.lld(), 25);
}
