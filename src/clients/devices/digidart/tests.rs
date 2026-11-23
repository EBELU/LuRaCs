use std::sync::Arc;

use super::from_link;
use super::protocol::{decode_fine_gain, encode_command, encode_fine_gain, Opcode};
use crate::clients::driver::DeviceDriver;
use crate::clients::link::MockLink;

fn reply_u32_1(val: u32) -> Vec<u8> {
    let mut out = vec![0u8, 0u8];
    out.extend_from_slice(&val.to_be_bytes());
    out
}

fn reply_u32_2(a: u32, b: u32) -> Vec<u8> {
    let mut out = vec![0u8, 0u8];
    out.extend_from_slice(&a.to_be_bytes());
    out.extend_from_slice(&b.to_be_bytes());
    out
}

fn reply_spectrum(counts: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + counts.len() * 4);
    out.extend_from_slice(&0u32.to_be_bytes()); // status = 0
    for &c in counts {
        out.extend_from_slice(&c.to_le_bytes());
    }
    out
}

#[test]
fn test_digidart_command_and_fine_gain_encoding() {
    let pkt = encode_command(Opcode::Clear as u16, 2, &[0, 0x8000]);
    assert_eq!(
        pkt,
        vec![0x02, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00]
    );

    assert!(encode_fine_gain(0.40).is_err());
    assert!(encode_fine_gain(1.05).is_err());

    let raw = encode_fine_gain(0.75).unwrap();
    let decoded = decode_fine_gain(raw);
    assert!((decoded - 0.75).abs() < 1e-5);
}

#[tokio::test]
async fn test_digidart_reconnect_status_and_realtime_cps() {
    let mock = Arc::new(MockLink::usb_mock("USB:0a2d:0005"));
    let driver = from_link(mock.clone(), "digiDART-test");

    // 1. on_reconnect: SHOW_GAIN_CONVERSION (512 ch), CLEAR, START (with warning bit 0x08)
    mock.push_incoming(reply_u32_1(512));
    mock.push_incoming(vec![0, 0]);
    mock.push_incoming(vec![0, 0x08]);

    driver.on_reconnect().await.unwrap();
    assert_eq!(driver.info().channels, 512);

    // 2. read_status: SHOW_HV_TARGET, SHOW_HV_ACTUAL, SHOW_LLD, SHOW_GAIN_FINE
    mock.push_incoming(reply_u32_2(1000, 0b100));
    mock.push_incoming(reply_u32_2(998, 0b100));
    mock.push_incoming(reply_u32_1(15));
    mock.push_incoming(reply_u32_1(encode_fine_gain(0.8).unwrap()));

    let status = driver.read_status().await.unwrap();
    assert_eq!(status.desired_voltage, 1000.0);
    assert_eq!(status.voltage, 998.0);
    assert_eq!(status.lower_level_discriminator, 15.0);
    assert!((status.fine_gain - 0.8).abs() < 1e-5);

    // 3. read_realtime sample 1: SHOW_HV_ACTUAL, SPECTRUM (512 ch), SHOW_LIVE (500 ticks = 10s), SHOW_TRUE
    let mut counts1 = vec![0u32; 512];
    counts1[10] = 500;
    mock.push_incoming(reply_u32_2(998, 0b100));
    mock.push_incoming(reply_spectrum(&counts1));
    mock.push_incoming(reply_u32_1(500));
    mock.push_incoming(reply_u32_1(510));

    let rt1 = driver.read_realtime().await.unwrap();
    assert!((rt1.cps - 0.0).abs() < 1e-6);

    // 4. read_realtime sample 2: +300 counts over +100 ticks (2.0s) -> 150.0 CPS
    let mut counts2 = vec![0u32; 512];
    counts2[10] = 800;
    mock.push_incoming(reply_u32_2(998, 0b100));
    mock.push_incoming(reply_spectrum(&counts2));
    mock.push_incoming(reply_u32_1(600));
    mock.push_incoming(reply_u32_1(610));

    let rt2 = driver.read_realtime().await.unwrap();
    assert!((rt2.cps - 150.0).abs() < 1e-4);

    // 5. read_spectrum
    mock.push_incoming(reply_u32_2(998, 0b100));
    mock.push_incoming(reply_spectrum(&counts2));
    mock.push_incoming(reply_u32_1(600));
    mock.push_incoming(reply_u32_1(610));

    let spec = driver.read_spectrum().await.unwrap();
    assert_eq!(spec.y_axis.len(), 512);
    assert!((spec.y_axis[10] - 800.0).abs() < 1e-6);
    assert!((spec.live_time - 12.0).abs() < 1e-6);
    assert!((spec.real_time.unwrap() - 12.2).abs() < 1e-6);
}

#[tokio::test]
async fn test_digidart_hv_control() {
    let mock = Arc::new(MockLink::usb_mock("USB:0a2d:0005"));
    let driver = from_link(mock.clone(), "digiDART-hv");
    let hv = driver.hv_control().expect("digiDART supports HvControl");

    // set_hv(900) calls SET_HV then reset_spectrum (CLEAR)
    mock.push_incoming(vec![0, 0]);
    mock.push_incoming(vec![0, 0]);
    hv.set_hv(900.0).await.unwrap();

    // set_lld(20)
    mock.push_incoming(vec![0, 0]);
    hv.set_lld(20.0).await.unwrap();

    let sent = mock.sent_packets();
    assert_eq!(sent[0], encode_command(Opcode::SetHv as u16, 2, &[900]));
    assert_eq!(sent[1], encode_command(Opcode::Clear as u16, 2, &[0, 0x8000]));
    assert_eq!(sent[2], encode_command(Opcode::SetLld as u16, 2, &[20]));
}
