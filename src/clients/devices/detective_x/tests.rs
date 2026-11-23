use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::protocol::{endpoints, parse_iso_duration};
use super::{normalize_base_url, DetectiveXDriver, HttpLink};
use crate::clients::driver::{DeviceDriver, DeviceError};
use crate::clients::runner::events::DeviceEvent;
use crate::clients::runner::DeviceRunner;
use crate::clients::PollConfig;

struct MockHttpServer {
    base_url: String,
    recorded_requests: Arc<Mutex<Vec<String>>>,
}

impl MockHttpServer {
    async fn start(responses: Vec<(&'static str, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let queue = Arc::new(Mutex::new(VecDeque::from(responses)));
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let recorded_clone = recorded.clone();

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let queue = queue.clone();
                let recorded = recorded_clone.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    if let Ok(n) = socket.read(&mut buf).await
                        && n > 0
                    {
                        let req_str = String::from_utf8_lossy(&buf[..n]);
                        if let Some(first_line) = req_str.lines().next() {
                            recorded.lock().unwrap().push(first_line.to_string());
                        }
                        let (status, body) = queue
                            .lock()
                            .unwrap()
                            .pop_front()
                            .unwrap_or(("404 Not Found", String::new()));

                        let resp = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        drop(socket.write_all(resp.as_bytes()).await);
                    }
                });
            }
        });

        Self {
            base_url: format!("http://{addr}"),
            recorded_requests: recorded,
        }
    }
}

#[test]
fn test_normalize_base_url() {
    assert_eq!(normalize_base_url("192.168.1.50"), "http://192.168.1.50");
    assert_eq!(normalize_base_url("localhost:8000/"), "http://localhost:8000");
    assert_eq!(
        normalize_base_url("http://10.0.0.42:8080/"),
        "http://10.0.0.42:8080"
    );
}

#[test]
fn test_parse_iso_duration() {
    assert!((parse_iso_duration("PT12.34s").unwrap() - 12.34).abs() < 1e-6);
    assert!((parse_iso_duration("PT0.5S").unwrap() - 0.5).abs() < 1e-6);
    assert!((parse_iso_duration("PT1M30.5S").unwrap() - 90.5).abs() < 1e-6);
    assert!(parse_iso_duration("not-a-duration").is_err());
}

#[tokio::test]
async fn test_detective_x_http_error_status() {
    let server = MockHttpServer::start(vec![("500 Internal Server Error", "fail".into())]).await;
    let driver = DetectiveXDriver::new(&server.base_url, "DetectiveX_err", false).unwrap();

    let err = driver.read_status().await.unwrap_err();
    assert!(matches!(err, DeviceError::Hardware(_)));
}

#[tokio::test]
async fn test_detective_x_status_and_gps() {
    let json_fix = r#"{
        "InstrumentStatus": {
            "Battery": 87.6,
            "CrystalTemperature": 298.15,
            "Coordinates": [52.52, 13.405]
        }
    }"#
    .to_string();

    let json_no_fix = r#"{
        "InstrumentStatus": {
            "Battery": 50.0,
            "CrystalTemperature": 273.0,
            "Coordinates": [0.0, 0.0]
        }
    }"#
    .to_string();

    let server = MockHttpServer::start(vec![("200 OK", json_fix), ("200 OK", json_no_fix)]).await;
    let driver = DetectiveXDriver::new(&server.base_url, "DetectiveX_test", true).unwrap();

    let status = driver.read_status().await.unwrap();
    assert_eq!(status.battery, Some(88));
    assert!((status.temperature - 25.15).abs() < 1e-4);

    let gps = driver.read_gps().await.expect("expected GPS data");
    assert!(gps.valid);
    assert_eq!(gps.latitude, Some(52.52));
    assert_eq!(gps.longitude, Some(13.405));
    assert_eq!(gps.source, "DetectiveX");

    let _ = driver.read_status().await.unwrap();
    let gps_no_fix = driver.read_gps().await.unwrap();
    assert!(!gps_no_fix.valid);
}

#[tokio::test]
async fn test_detective_x_realtime_mojibake_unit() {
    let json = r#"{
        "Measurement": {
            "CountDoseData": [
                {
                    "DetectorType": "Gamma",
                    "Remark": "Current",
                    "DoseRate": [{ "unit": "ÂµSv/h", "value": 0.42 }],
                    "CountRate": { "unit": "cps", "value": 123.4 }
                }
            ]
        }
    }"#
    .to_string();

    let server = MockHttpServer::start(vec![("200 OK", json)]).await;
    let driver = DetectiveXDriver::new(&server.base_url, "DetectiveX_test", false).unwrap();

    let rt = driver.read_realtime().await.unwrap();
    assert!((rt.cps - 123.4).abs() < 1e-6);
    assert!((rt.dr - 0.42).abs() < 1e-6);
}

#[tokio::test]
async fn test_detective_x_calibration_spectrum_and_control() {
    let cal_json = r#"{
        "CalibrationType": {
            "EquationModel": {
                "Model": "Polynomial",
                "Coefficients": [1.0, 0.5, 0.001]
            }
        }
    }"#
    .to_string();

    let spec_json = r#"{
        "Measurement": {
            "Spectrum": {
                "ChannelData": [0.0, 4.0, 12.0, 3.0],
                "LiveTime": "PT42.5s",
                "RealTime": "PT43.0s"
            }
        }
    }"#
    .to_string();

    let server = MockHttpServer::start(vec![
        ("200 OK", cal_json),
        ("200 OK", spec_json),
        ("200 OK", String::new()),
        ("200 OK", String::new()),
        ("200 OK", String::new()),
    ])
    .await;

    let driver = DetectiveXDriver::new(&server.base_url, "DetectiveX_test", false).unwrap();

    let coeffs = driver.fetch_calibration().await.unwrap();
    assert_eq!(coeffs, vec![0.001, 0.5, 1.0]);

    let spec = driver.read_spectrum().await.unwrap();
    assert_eq!(spec.y_axis.as_slice().unwrap(), &[0.0, 4.0, 12.0, 3.0]);
    assert!((spec.live_time - 42.5).abs() < 1e-6);
    assert_eq!(spec.real_time, Some(43.0));
    assert_eq!(spec.calib_coeff, Some(vec![0.001, 0.5, 1.0]));

    driver.start_acquisition().await.unwrap();
    driver.reset_spectrum().await.unwrap();
    driver.stop_acquisition().await.unwrap();

    let recorded = server.recorded_requests.lock().unwrap();
    assert!(recorded[2].starts_with(&format!("PUT {}", endpoints::ACQ_START)));
    assert!(recorded[3].starts_with(&format!("PUT {}", endpoints::ACQ_CLEAR)));
    assert!(recorded[4].starts_with(&format!("PUT {}", endpoints::ACQ_STOP)));
    drop(recorded);
}

#[tokio::test]
async fn test_detective_x_runner_integration() {
    let rt_json = r#"{
        "Measurement": {
            "CountDoseData": [
                {
                    "DetectorType": "Gamma",
                    "Remark": "Current",
                    "DoseRate": [{ "unit": "µSv/h", "value": 0.15 }],
                    "CountRate": { "unit": "cps", "value": 88.0 }
                }
            ]
        }
    }"#
    .to_string();

    let server = MockHttpServer::start(vec![("200 OK", rt_json)]).await;
    let driver = DetectiveXDriver::new(&server.base_url, "Runner DetectiveX", false).unwrap();
    let link = HttpLink::new(&server.base_url);

    let (events_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let config = Arc::new(std::sync::RwLock::new(PollConfig {
        update_loop_delay: 0.05,
        spectrum_update_delay: 0.5,
    }));

    let handle = DeviceRunner::spawn(Box::new(driver), Box::new(link), events_tx, config);

    let mut got_realtime = false;
    for _ in 0..10 {
        if let Ok(Some(DeviceEvent::CurrentUpdated { data, .. })) =
            tokio::time::timeout(Duration::from_millis(300), rx.recv()).await
        {
            assert!((data.cps - 88.0).abs() < 1e-6);
            assert!((data.dr - 0.15).abs() < 1e-6);
            got_realtime = true;
            break;
        }
    }

    assert!(got_realtime, "expected real-time telemetry from DetectiveX runner");
    handle.abort();
}
