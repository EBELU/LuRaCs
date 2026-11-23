use std::sync::Arc;
use std::time::Duration;

use super::from_link;
use super::protocol::{
    build_ping_packet, build_reset_packet, build_spectrum_request, build_tx2_packet,
    checksum_decode, decode_cps_packet, decode_meta_packet, decode_spectrum_packet,
    decode_status_packet, verify_cps_checksum, verify_spectrum_checksum, EnergyRange,
};
use crate::clients::driver::DeviceDriver;
use crate::clients::link::MockLink;

fn append_spectrum_checksum(pkt: &mut Vec<u8>) {
    // Set length byte (payload_len + 3 checksum bytes) before computing checksum over the whole packet
    let len_byte = pkt.len() + 2;
    pkt[0] = if len_byte == 256 { 0 } else { len_byte as u8 };
    let chk = checksum_decode(pkt);
    pkt.push((chk & 0xFF) as u8);
    pkt.push(((chk >> 8) & 0xFF) as u8);
    pkt.push(((chk >> 16) & 0xFF) as u8);
}

fn append_cps_checksum(pkt: &mut Vec<u8>) {
    let chk = checksum_decode(&pkt[1..]);
    let b0 = ((chk >> 16) & 0xFF) as u8;
    let b1 = ((chk >> 8) & 0xFF) as u8;
    // Python stores exp_byte1 at -4 (len-4) and exp_byte0 at -3 (len-3), then 2 trailing bytes
    pkt.push(b1);
    pkt.push(b0);
    pkt.push(0x00);
    pkt.push(0x00);
    pkt[0] = (pkt.len() - 1) as u8;
}

#[test]
fn test_tx2_encoding() {
    let ping = build_ping_packet(1, 1_700_000_000);
    assert_eq!(ping[0], 0x12);
    assert_eq!(ping[1], 1);

    let tx2 = build_tx2_packet(&ping);
    assert_eq!(tx2[0], 0xFF);
    assert_eq!(tx2[2], 0xEE);
    assert_eq!(*tx2.last().unwrap() as usize, tx2.len());

    let req = build_spectrum_request(0, 1799);
    assert_eq!(req, vec![0x3E, 0x00, 0x00, 0x07, 0x08]);

    assert_eq!(build_reset_packet(EnergyRange::Mid), [0x10, 4]);
}

#[test]
fn test_decode_status_and_meta() {
    // Status packet (0x02), 14+ bytes
    // temp = 1235 -> 123.5 - 100.0 = 23.5 C
    // battery = 88, charging = 1, uart_errors = 0
    let mut status_pkt = vec![13, 0x02];
    status_pkt.extend_from_slice(&1235u16.to_le_bytes());
    status_pkt.extend_from_slice(&88u16.to_le_bytes());
    status_pkt.push(0x01); // charging
    status_pkt.extend_from_slice(&0u16.to_le_bytes());
    status_pkt.extend_from_slice(&0u16.to_le_bytes());
    status_pkt.push(0x01); // temp_ok
    status_pkt.extend_from_slice(&0u16.to_le_bytes()); // uart_errors = 0

    let st = decode_status_packet(&status_pkt).unwrap();
    assert_eq!(st.battery, 88);
    assert!((st.temperature - 23.5).abs() < 1e-4);
    assert!(st.charging);
    assert_eq!(st.uart_errors, 0);

    // Meta packet (0x01), 17 bytes
    let mut meta_pkt = vec![16, 0x01];
    meta_pkt.extend_from_slice(&5000u32.to_le_bytes()); // counts
    meta_pkt.extend_from_slice(&120u32.to_le_bytes()); // uptime = 120 / 12 = 10.0 s
    meta_pkt.extend_from_slice(&42u32.to_le_bytes());
    meta_pkt.extend_from_slice(&[1, 0, 0]);
    let meta = decode_meta_packet(&meta_pkt).unwrap();
    assert_eq!(meta.counts, 5000);
    assert!((meta.uptime_secs - 10.0).abs() < 1e-6);
}

#[test]
fn test_decode_cps_and_spectrum_packets() {
    // CPS packet (0x17)
    // raw = 3000 => 3000 / 600.0 = 5.0 CPS
    // dose raw = 6000 => mult10=1, res=0 => 0; use 6600 => mult10=1, res=600 => 6000 / 600 = 10.0 => DR = 0.10
    let mut cps_pkt = vec![0u8; 15];
    cps_pkt[1] = 0x17;
    cps_pkt[2] = 0; // CPS
    cps_pkt[3..5].copy_from_slice(&3000u16.to_le_bytes());
    cps_pkt[5] = 1; // Dose
    cps_pkt[6..8].copy_from_slice(&6600u16.to_le_bytes());
    append_cps_checksum(&mut cps_pkt);

    assert!(verify_cps_checksum(&cps_pkt));
    let cps = decode_cps_packet(&cps_pkt).unwrap();
    assert!((cps.cps - 5.0).abs() < 1e-6);
    assert!((cps.dose_rate - 0.1).abs() < 1e-6);

    // Spectrum packet (0x30: div=1), start_ch=10, init=100, then 2x 4-bit deltas (+2, -1) -> [100, 102, 101]
    let mut spec_pkt = vec![
        0x00, // placeholder len
        0x30, // div=1
        10, 0, // start_ch = 10
        100, 0, 0,    // init value = 100
        0x02, // point_type=0 (4-bit), count=2
        0x2F, // hi nibble +2, lo nibble -1 (0x0F -> 15-16 = -1)
    ];
    append_spectrum_checksum(&mut spec_pkt);

    assert!(verify_spectrum_checksum(&spec_pkt));
    let chunk = decode_spectrum_packet(&spec_pkt).unwrap();
    assert_eq!(chunk.start_ch, 10);
    assert_eq!(chunk.end_ch, 13);
    assert_eq!(chunk.values, vec![100.0, 102.0, 101.0]);
}

#[tokio::test]
async fn test_raysid_driver_stream_integration() {
    let mock = Arc::new(MockLink::ble_mock("BLE:RaySID-test"));
    let driver = from_link(mock.clone(), "RaySID-test");

    // Queue status, meta, CPS, and spectrum packets
    let mut status_pkt = vec![13, 0x02];
    status_pkt.extend_from_slice(&1250u16.to_le_bytes()); // 25.0 C
    status_pkt.extend_from_slice(&95u16.to_le_bytes());
    status_pkt.push(0x00);
    status_pkt.extend_from_slice(&[0u8; 7]);

    let mut meta_pkt = vec![16, 0x01];
    meta_pkt.extend_from_slice(&1000u32.to_le_bytes());
    meta_pkt.extend_from_slice(&360u32.to_le_bytes()); // 30.0 s
    meta_pkt.extend_from_slice(&0u32.to_le_bytes());
    meta_pkt.extend_from_slice(&[0, 0, 0]);

    let mut cps_pkt = vec![0u8; 15];
    cps_pkt[1] = 0x17;
    cps_pkt[2] = 0;
    cps_pkt[3..5].copy_from_slice(&4800u16.to_le_bytes()); // 8.0 CPS
    cps_pkt[5] = 1;
    cps_pkt[6..8].copy_from_slice(&6600u16.to_le_bytes()); // 0.1 DR
    append_cps_checksum(&mut cps_pkt);

    let mut spec_pkt = vec![0x00, 0x30, 5, 0, 50, 0, 0, 0x41, 10]; // point_type=1 (8-bit), count=1, +10 -> [50, 60]
    append_spectrum_checksum(&mut spec_pkt);

    // Push them across separate notifications
    mock.push_incoming(status_pkt);
    mock.push_incoming(meta_pkt);
    mock.push_incoming(cps_pkt);
    mock.push_incoming(spec_pkt);

    driver.on_reconnect().await.unwrap();

    // Wait briefly for the background rx_task to process all queued packets
    let mut ready = false;
    for _ in 0..20 {
        let rt = driver.read_realtime().await.unwrap();
        if (rt.cps - 8.0).abs() < 1e-6 {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    assert!(ready, "expected RaySID rx_task to decode CPS packet");

    let status = driver.read_status().await.unwrap();
    assert_eq!(status.battery, Some(95));
    assert!((status.temperature - 25.0).abs() < 1e-4);

    let spec = driver.read_spectrum().await.unwrap();
    assert!((spec.live_time - 30.0).abs() < 1e-6);
    assert!((spec.y_axis[5] - 50.0).abs() < 1e-6);
    assert!((spec.y_axis[6] - 60.0).abs() < 1e-6);

    driver.stop_acquisition().await.unwrap();
}
