use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use super::protocol::{
    decode_data_buf, decode_spectrum, encode_request, execute_transaction, read_virtual_string,
    write_virtual_string, Command, VirtualString,
};
use super::{from_link, RadiacodeDriver};
use crate::clients::driver::DeviceDriver;
use crate::clients::link::MockLink;
use crate::clients::runner::events::DeviceEvent;
use crate::clients::runner::DeviceRunner;

#[test]
fn test_encode_request() {
    let req = encode_request(Command::SetExchange as u16, 3, &[0x01, 0xff, 0x12, 0xff]);
    // Expected:
    // [4 bytes len]: 4 + 4 = 8 = [0x08, 0x00, 0x00, 0x00]
    // [2 bytes cmd]: 0x0007 = [0x07, 0x00]
    // [1 byte 0x00]: [0x00]
    // [1 byte seq]: 0x80 | (3 % 32) = 0x83 = [0x83]
    // [4 bytes args]: [0x01, 0xff, 0x12, 0xff]
    assert_eq!(
        req,
        vec![0x08, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x83, 0x01, 0xff, 0x12, 0xff]
    );
}

#[tokio::test]
async fn test_execute_transaction_and_virtual_string() {
    let mock = MockLink::usb_mock("MOCK:Radiacode");

    // Queue response for RD_VIRT_STRING (0x0826)
    // Response framing:
    // [4 bytes len]: 4 (header) + 8 (retcode + flen) + 4 (data "TEST") = 16
    // [header]: cmd=0x0826, 0x00, seq=0x81 (for seq=1)
    // [retcode]: 1 (u32 LE)
    // [flen]: 4 (u32 LE)
    // [data]: b"TEST"
    let mut resp = Vec::new();
    resp.extend_from_slice(&(16u32).to_le_bytes()); // len
    resp.extend_from_slice(&(Command::RdVirtString as u16).to_le_bytes());
    resp.push(0x00);
    resp.push(0x81); // seq 1
    resp.extend_from_slice(&(1u32).to_le_bytes()); // retcode = 1
    resp.extend_from_slice(&(4u32).to_le_bytes()); // flen = 4
    resp.extend_from_slice(b"TEST");

    mock.push_incoming(resp);

    let res = read_virtual_string(
        &mock,
        1,
        VirtualString::SerialNumber as u32,
        Duration::from_millis(500),
    )
    .await
    .unwrap();

    assert_eq!(res, b"TEST");
}

#[tokio::test]
async fn test_write_virtual_string() {
    let mock = MockLink::usb_mock("MOCK:Radiacode");

    // Queue response for WR_VIRT_STRING (0x0827)
    // [4 bytes len]: 4 (header) + 4 (retcode) = 8
    // [header]: cmd=0x0827, 0x00, seq=0x82 (seq=2)
    // [retcode]: 1 (u32 LE)
    let mut resp = Vec::new();
    resp.extend_from_slice(&(8u32).to_le_bytes());
    resp.extend_from_slice(&(Command::WrVirtString as u16).to_le_bytes());
    resp.push(0x00);
    resp.push(0x82);
    resp.extend_from_slice(&(1u32).to_le_bytes());

    mock.push_incoming(resp);

    let res = write_virtual_string(
        &mock,
        2,
        VirtualString::Spectrum as u32,
        &[],
        Duration::from_millis(500),
    )
    .await;

    assert!(res.is_ok());
}

#[test]
fn test_decode_spectrum_v0() {
    let mut payload = Vec::new();
    // Header: ts=100s, a0=1.5, a1=2.5, a2=0.003
    payload.extend_from_slice(&(100u32).to_le_bytes());
    payload.extend_from_slice(&(1.5f32).to_le_bytes());
    payload.extend_from_slice(&(2.5f32).to_le_bytes());
    payload.extend_from_slice(&(0.003f32).to_le_bytes());

    // Counts: [10, 20, 30]
    payload.extend_from_slice(&(10u32).to_le_bytes());
    payload.extend_from_slice(&(20u32).to_le_bytes());
    payload.extend_from_slice(&(30u32).to_le_bytes());

    let decoded = decode_spectrum(&payload, 0).unwrap();
    assert_eq!(decoded.duration_secs, 100);
    assert!((decoded.a0 - 1.5).abs() < 1e-6);
    assert!((decoded.a1 - 2.5).abs() < 1e-6);
    assert!((decoded.a2 - 0.003).abs() < 1e-6);
    assert_eq!(decoded.counts, vec![10, 20, 30]);
}

#[test]
fn test_decode_spectrum_v1_compression() {
    let mut payload = Vec::new();
    // Header
    payload.extend_from_slice(&(60u32).to_le_bytes());
    payload.extend_from_slice(&(0.0f32).to_le_bytes());
    payload.extend_from_slice(&(1.0f32).to_le_bytes());
    payload.extend_from_slice(&(0.0f32).to_le_bytes());

    // Chunk 1: 3 zero counts (vlen = 0, count = 3)
    // u16 = (3 << 4) | 0 = 0x0030
    payload.extend_from_slice(&(0x0030u16).to_le_bytes());

    // Chunk 2: 2 byte counts (vlen = 1, count = 2): values [5, 12]
    // u16 = (2 << 4) | 1 = 0x0021
    payload.extend_from_slice(&(0x0021u16).to_le_bytes());
    payload.push(5);
    payload.push(12);

    // Chunk 3: 1 delta i8 (vlen = 2, count = 1): delta -2 -> value 10
    // u16 = (1 << 4) | 2 = 0x0012
    payload.extend_from_slice(&(0x0012u16).to_le_bytes());
    payload.push((-2i8) as u8);

    // Chunk 4: 1 delta i16 (vlen = 3, count = 1): delta +100 -> value 110
    // u16 = (1 << 4) | 3 = 0x0013
    payload.extend_from_slice(&(0x0013u16).to_le_bytes());
    payload.extend_from_slice(&(100i16).to_le_bytes());

    let decoded = decode_spectrum(&payload, 1).unwrap();
    assert_eq!(decoded.counts, vec![0, 0, 0, 5, 12, 10, 110]);
}

#[test]
fn test_decode_data_buf() {
    let mut buf = Vec::new();

    // 1. Record 1: RealTimeData (eid=0, gid=0)
    buf.push(1); // seq
    buf.push(0); // eid
    buf.push(0); // gid
    buf.extend_from_slice(&(100i32).to_le_bytes()); // ts_offset = 100 * 10ms = 1s

    // RealTime payload: count_rate=45.2, dose_rate=0.12, err_cp=15 (1.5%), err_dr=20 (2.0%), flags=0, rt_flags=0
    buf.extend_from_slice(&(45.2f32).to_le_bytes());
    buf.extend_from_slice(&(0.12f32).to_le_bytes());
    buf.extend_from_slice(&(15u16).to_le_bytes());
    buf.extend_from_slice(&(20u16).to_le_bytes());
    buf.extend_from_slice(&(0u16).to_le_bytes());
    buf.push(0);

    // 2. Record 2: RareData (eid=0, gid=3)
    buf.push(2); // seq
    buf.push(0); // eid
    buf.push(3); // gid
    buf.extend_from_slice(&(100i32).to_le_bytes());

    // RareData payload: duration=3600, dose=12.34, temp=4250 ((4250-2000)/100 = 22.5 C), charge=8500 (85%), flags=0
    buf.extend_from_slice(&(3600u32).to_le_bytes());
    buf.extend_from_slice(&(12.34f32).to_le_bytes());
    buf.extend_from_slice(&(4250u16).to_le_bytes());
    buf.extend_from_slice(&(8500u16).to_le_bytes());
    buf.extend_from_slice(&(0u16).to_le_bytes());

    let (rt, status) = decode_data_buf(&buf, 1000.0);

    let rt = rt.expect("expected RealTimeData");
    assert!((rt.cps - 45.2).abs() < 1e-4);
    assert!((rt.dr - 1200.0).abs() < 1e-2); // 0.12 * 1e4
    assert!((rt.cps_error - 1.5).abs() < 1e-4);
    assert!((rt.dr_error - 2.0).abs() < 1e-4);
    assert!((rt.timestamp - 1001.0).abs() < 1e-4);

    let status = status.expect("expected RareData");
    assert_eq!(status.battery, Some(85));
    assert!((status.temperature - 22.5).abs() < 1e-4);
    assert!((status.total_dose - 12.34).abs() < 1e-4);
    assert_eq!(status.total_uptime, 3600.0);
}

#[tokio::test]
async fn test_radiacode_driver_read_realtime_and_status() {
    let mock = Arc::new(MockLink::usb_mock("MOCK:Radiacode"));
    let driver = from_link(mock.clone(), "Test RadiaCode");

    // Prepare simulated DATA_BUF response
    let mut data_buf = Vec::new();
    // RealTimeData record
    data_buf.push(1);
    data_buf.push(0);
    data_buf.push(0);
    data_buf.extend_from_slice(&(0i32).to_le_bytes());
    data_buf.extend_from_slice(&(23.5f32).to_le_bytes());
    data_buf.extend_from_slice(&(0.08f32).to_le_bytes());
    data_buf.extend_from_slice(&(10u16).to_le_bytes());
    data_buf.extend_from_slice(&(12u16).to_le_bytes());
    data_buf.extend_from_slice(&(0u16).to_le_bytes());
    data_buf.push(0);

    // RareData record
    data_buf.push(2);
    data_buf.push(0);
    data_buf.push(3);
    data_buf.extend_from_slice(&(0i32).to_le_bytes());
    data_buf.extend_from_slice(&(7200u32).to_le_bytes());
    data_buf.extend_from_slice(&(5.5f32).to_le_bytes());
    data_buf.extend_from_slice(&(4500u16).to_le_bytes()); // 25.0 C
    data_buf.extend_from_slice(&(9200u16).to_le_bytes()); // 92%
    data_buf.extend_from_slice(&(0u16).to_le_bytes());

    // Wrap in RD_VIRT_STRING response
    let mut resp = Vec::new();
    let flen = data_buf.len() as u32;
    let packet_len = 4 + 8 + flen;
    resp.extend_from_slice(&packet_len.to_le_bytes());
    resp.extend_from_slice(&(Command::RdVirtString as u16).to_le_bytes());
    resp.push(0x00);
    resp.push(0x80); // seq 0
    resp.extend_from_slice(&(1u32).to_le_bytes()); // retcode = 1
    resp.extend_from_slice(&flen.to_le_bytes());
    resp.extend_from_slice(&data_buf);

    mock.push_incoming(resp);

    let rt = driver.read_realtime().await.unwrap();
    assert!((rt.cps - 23.5).abs() < 1e-4);

    let status = driver.read_status().await.unwrap();
    assert_eq!(status.battery, Some(92));
    assert!((status.temperature - 25.0).abs() < 1e-4);
    assert_eq!(status.total_uptime, 7200.0);
}

#[tokio::test]
async fn test_radiacode_runner_integration() {
    let mock = Arc::new(MockLink::usb_mock("MOCK:Radiacode"));
    let driver = from_link(mock.clone(), "Runner RadiaCode");

    // Prepare simulated DATA_BUF response
    let mut data_buf = Vec::new();
    data_buf.push(1);
    data_buf.push(0);
    data_buf.push(0);
    data_buf.extend_from_slice(&(0i32).to_le_bytes());
    data_buf.extend_from_slice(&(55.0f32).to_le_bytes());
    data_buf.extend_from_slice(&(0.2f32).to_le_bytes());
    data_buf.extend_from_slice(&(5u16).to_le_bytes());
    data_buf.extend_from_slice(&(8u16).to_le_bytes());
    data_buf.extend_from_slice(&(0u16).to_le_bytes());
    data_buf.push(0);

    let flen = data_buf.len() as u32;
    let mut resp = Vec::new();
    resp.extend_from_slice(&(4 + 8 + flen).to_le_bytes());
    resp.extend_from_slice(&(Command::RdVirtString as u16).to_le_bytes());
    resp.push(0x00);
    resp.push(0x80);
    resp.extend_from_slice(&(1u32).to_le_bytes());
    resp.extend_from_slice(&flen.to_le_bytes());
    resp.extend_from_slice(&data_buf);

    mock.push_incoming(resp);

    let (events_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let config = std::sync::Arc::new(std::sync::RwLock::new(
        crate::clients::PollConfig {
            update_loop_delay: 0.05,
            spectrum_update_delay: 0.5,
        },
    ));

    let handle = DeviceRunner::spawn(
        Box::new(driver),
        Box::new((*mock).clone()),
        events_tx,
        config,
    );

    // We should receive telemetry events
    let mut got_realtime = false;
    for _ in 0..10 {
        if let Ok(Some(DeviceEvent::CurrentUpdated { data, .. })) =
            tokio::time::timeout(Duration::from_millis(300), rx.recv()).await
        {
            assert!((data.cps - 55.0).abs() < 1e-4);
            got_realtime = true;
            break;
        }
    }

    assert!(got_realtime, "expected real-time telemetry from runner");
    handle.abort();
}

#[tokio::test]
async fn test_radiacode_driver_methods_and_protocol_errors() {
    use super::protocol::{decode_spectrum, execute_transaction};
    use crate::clients::device_wrapper_base::ConnectionType;

    let mock = Arc::new(MockLink::usb_mock("MOCK:RC2"));
    let driver = from_link(mock.clone(), "RC-methods");
    assert_eq!(driver.link().address_name(), "MOCK:RC2");
    assert_eq!(driver.info().connection_type, ConnectionType::Usb);

    // Helper to build a valid RD_VIRT_STRING reply
    let make_vs_reply = |seq: u8, payload: &[u8]| -> Vec<u8> {
        let flen = payload.len() as u32;
        let mut resp = Vec::new();
        resp.extend_from_slice(&(4 + 8 + flen).to_le_bytes());
        resp.extend_from_slice(&(Command::RdVirtString as u16).to_le_bytes());
        resp.push(0x00);
        resp.push(0x80 | seq);
        resp.extend_from_slice(&(1u32).to_le_bytes());
        resp.extend_from_slice(&flen.to_le_bytes());
        resp.extend_from_slice(payload);
        resp
    };

    // read_configuration (seq 0) & read_serial_number (seq 1)
    mock.push_incoming(make_vs_reply(0x00, b"cfg=1\n"));
    assert_eq!(driver.read_configuration().await.unwrap(), "cfg=1\n");
    mock.push_incoming(make_vs_reply(0x01, b"RC-102-001\0"));
    assert_eq!(driver.read_serial_number().await.unwrap(), "RC-102-001");

    // read_spectrum (seq 2): v0 spectrum with 2 channels
    let mut spec_v0 = Vec::new();
    spec_v0.extend_from_slice(&10.0f32.to_le_bytes()); // duration
    spec_v0.extend_from_slice(&0.0f32.to_le_bytes()); // a0
    spec_v0.extend_from_slice(&2.0f32.to_le_bytes()); // a1
    spec_v0.extend_from_slice(&0.0f32.to_le_bytes()); // a2
    spec_v0.extend_from_slice(&5u32.to_le_bytes());
    spec_v0.extend_from_slice(&15u32.to_le_bytes());
    mock.push_incoming(make_vs_reply(0x02, &spec_v0));
    let spec = driver.read_spectrum().await.unwrap();
    assert_eq!(spec.y_axis.len(), 2);

    // reset_spectrum (seq 3): WR_VIRT_STRING / SET_EXCHANGE / spectrum reset reply (retcode=1)
    let mut wr_reply = Vec::new();
    wr_reply.extend_from_slice(&(4 + 4u32).to_le_bytes());
    wr_reply.extend_from_slice(&(Command::WrVirtString as u16).to_le_bytes());
    wr_reply.push(0x00);
    wr_reply.push(0x80 | 0x03);
    wr_reply.extend_from_slice(&1u32.to_le_bytes());
    mock.push_incoming(wr_reply);
    driver.reset_spectrum().await.unwrap();

    // decode_spectrum error branches
    assert!(decode_spectrum(&[0u8; 4], 0).is_err());
    let bad_ver = vec![0u8; 20];
    assert!(decode_spectrum(&bad_ver, 99).is_err());

    // execute_transaction short response error
    let mut short_resp = Vec::new();
    short_resp.extend_from_slice(&2u32.to_le_bytes());
    short_resp.extend_from_slice(&[0x01, 0x02]);
    mock.push_incoming(short_resp);
    assert!(
        execute_transaction(
            &*mock,
            Command::RdVirtString as u16,
            0x00,
            &[],
            Duration::from_millis(20)
        )
        .await
        .is_err()
    );
}
