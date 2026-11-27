use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::protocol::{parse_leading_number, parse_seconds, parse_stream_line};
use super::IdentifinderDriver;
use crate::clients::driver::DeviceDriver;

#[test]
fn test_parse_seconds_and_leading_number() {
    assert!((parse_seconds(Some(&json!("54.0 s"))) - 54.0).abs() < 1e-6);
    assert!((parse_seconds(Some(&json!(12.5))) - 12.5).abs() < 1e-6);
    assert!((parse_seconds(None) - 0.0).abs() < 1e-6);

    assert!((parse_leading_number(Some(&json!("0.45 µSv/h"))) - 0.45).abs() < 1e-6);
    assert!((parse_leading_number(Some(&json!("128.2 cps"))) - 128.2).abs() < 1e-6);
    assert!((parse_leading_number(Some(&json!(42))) - 42.0).abs() < 1e-6);
}

#[test]
fn test_parse_stream_line() {
    let sse_line = r#"data: {"M":[{"success":true,"running":true,"Misc":{"LiveTime":"10.0 s","RealTime":"10.5 s","DoseRate":"0.12 uSv/h","GammaRate":"33.3 cps"},"Spectrum":[1,2,3]}]}"#;
    let payload = parse_stream_line(sse_line).expect("expected valid SSE payload");
    assert!(payload.running);
    assert_eq!(payload.spectrum.unwrap(), vec![1.0, 2.0, 3.0]);

    // Raw JSON without `data:` prefix
    let raw_json = r#"{"M":[{"success":true,"running":false}]}"#;
    let payload2 = parse_stream_line(raw_json).expect("expected valid raw JSON payload");
    assert!(!payload2.running);

    // Unsuccessful payload is ignored
    let fail_line = r#"data: {"M":[{"success":false}]}"#;
    assert!(parse_stream_line(fail_line).is_none());

    // Keepalive / non-JSON ignored
    assert!(parse_stream_line(": keepalive").is_none());
}

#[tokio::test]
async fn test_identifinder_stream_and_commands() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let recorded = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorded_clone = recorded.clone();

    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let recorded = recorded_clone.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                if let Ok(n) = socket.read(&mut buf).await
                    && n > 0
                {
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req.lines().next().unwrap_or("").to_string();
                    recorded.lock().unwrap().push(first_line.clone());

                    if first_line.contains("/virtualPha/signalR") {
                        let sse_body = concat!(
                            "data: {\"M\":[{\"success\":true,\"running\":true,",
                            "\"Misc\":{\"LiveTime\":\"25.0 s\",\"RealTime\":\"26.0 s\",",
                            "\"DoseRate\":\"0.25 uSv/h\",\"GammaRate\":\"75.5 cps\"},",
                            "\"Spectrum\":[10,20,30,40]}]}\n\n"
                        );
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse_body}",
                            sse_body.len()
                        );
                        drop(socket.write_all(resp.as_bytes()).await);
                    } else {
                        let json_body = r#"{"ok":true}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json_body}",
                            json_body.len()
                        );
                        drop(socket.write_all(resp.as_bytes()).await);
                    }
                }
            });
        }
    });

    let base_url = format!("http://{addr}");
    let driver = IdentifinderDriver::new(&base_url, "R400_test").unwrap();

    driver.on_reconnect().await.unwrap();

    // Give the SSE task a moment to process the streamed event
    let mut got_spec = false;
    for _ in 0..20 {
        let rt = driver.read_realtime().await.unwrap();
        if (rt.cps - 75.5).abs() < 1e-6 {
            assert!((rt.dr - 0.25).abs() < 1e-6);
            got_spec = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(got_spec, "expected streamed telemetry from SignalR SSE");

    let spec = driver.read_spectrum().await.unwrap();
    assert_eq!(spec.y_axis.as_slice().unwrap(), &[10.0, 20.0, 30.0, 40.0]);
    assert!((spec.live_time - 25.0).abs() < 1e-6);
    assert_eq!(spec.real_time, Some(26.0));

    driver.reset_spectrum().await.unwrap();
    driver.stop_acquisition().await.unwrap();

    let reqs = recorded.lock().unwrap();
    assert!(reqs.iter().any(|r| r.contains("/virtualPha/api/?action=Start")));
    assert!(reqs.iter().any(|r| r.contains("/virtualPha/api/?action=Clear")));
    assert!(reqs.iter().any(|r| r.contains("/virtualPha/api/?action=Stop")));
    drop(reqs);
}

#[tokio::test]
async fn test_identifinder_connect_network_and_driver_methods() {
    use crate::clients::devices::identifinder::connect_network;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                if let Ok(n) = socket.read(&mut buf).await
                    && n > 0
                {
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req.lines().next().unwrap_or("");
                    if first_line.contains("/virtualPha/signalR") {
                        let sse_body = ": keepalive\n\n";
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse_body}",
                            sse_body.len()
                        );
                        drop(socket.write_all(resp.as_bytes()).await);
                    } else {
                        let json_body = r#"{"ok":true}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json_body}",
                            json_body.len()
                        );
                        drop(socket.write_all(resp.as_bytes()).await);
                    }
                }
            });
        }
    });

    let drv = connect_network(&addr.to_string()).await.unwrap();
    assert_eq!(drv.base_url(), format!("http://{addr}"));
    assert_eq!(drv.info().model, "IdentiFINDER R400");
    assert!(drv.read_status().await.is_ok());
    assert!(drv.read_spectrum().await.is_ok());
    assert!(drv.start_acquisition().await.is_ok());
    assert!(drv.on_reconnect().await.is_ok());
    assert!(drv.identify_spectrum().await.is_ok());
    assert!(drv.save_spectrum().await.is_ok());
    drop(drv.stop_acquisition().await);

    // Connect error -> map_reqwest_err
    let dead = crate::clients::devices::identifinder::IdentifinderDriver::new(
        "http://127.0.0.1:1",
        "Dead",
    )
    .unwrap();
    assert!(dead.command("Start").await.is_err());
}
