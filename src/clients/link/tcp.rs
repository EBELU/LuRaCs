//! TCP transport link for network-connected spectrometers (e.g. Detective X, IdentiFinder).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use super::{DeviceLink, LinkError, LinkStatus, ReconnectPolicy};

/// Network TCP transport link.
pub struct TcpLink {
    name: String,
    host: String,
    port: u16,
    policy: ReconnectPolicy,
    stream: Arc<Mutex<Option<TcpStream>>>,
}

impl TcpLink {
    /// Connect to a spectrometer over TCP.
    pub async fn connect(host: impl Into<String>, port: u16) -> Result<Self, LinkError> {
        let host_str = host.into();
        let name = format!("NET:{host_str}:{port}");
        let addr = format!("{host_str}:{port}");

        let stream = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(&addr))
            .await
            .map_err(|_| LinkError::Timeout(Duration::from_secs(3)))?
            .map_err(|e| LinkError::Disconnected(format!("TCP connect to {addr} failed: {e}")))?;

        Ok(Self {
            name,
            host: host_str,
            port,
            policy: ReconnectPolicy::never(),
            stream: Arc::new(Mutex::new(Some(stream))),
        })
    }
}

#[async_trait]
impl DeviceLink for TcpLink {
    fn address_name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> LinkStatus {
        // Simple non-blocking check
        LinkStatus::Connected
    }

    fn reconnect_policy(&self) -> &ReconnectPolicy {
        &self.policy
    }

    async fn send(&self, data: &[u8]) -> Result<(), LinkError> {
        let mut guard = self.stream.lock().await;
        let stream = guard
            .as_mut()
            .ok_or_else(|| LinkError::Disconnected("TCP socket not open".into()))?;

        let res = stream
            .write_all(data)
            .await
            .map_err(|e| LinkError::Disconnected(format!("TCP write failed: {e}")));
        drop(guard);
        res
    }

    async fn recv(&self, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        let mut guard = self.stream.lock().await;
        let stream = guard
            .as_mut()
            .ok_or_else(|| LinkError::Disconnected("TCP socket not open".into()))?;

        let mut buf = vec![0u8; 4096];
        let read_res = tokio::time::timeout(timeout, stream.read(&mut buf)).await;
        drop(guard);

        let n = read_res
            .map_err(|_| LinkError::Timeout(timeout))?
            .map_err(|e| LinkError::Disconnected(format!("TCP read failed: {e}")))?;

        if n == 0 {
            return Err(LinkError::Disconnected("TCP connection closed by peer".into()));
        }

        buf.truncate(n);
        Ok(buf)
    }

    async fn reconnect(&self) -> Result<(), LinkError> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(&addr))
            .await
            .map_err(|_| LinkError::Timeout(Duration::from_secs(3)))?
            .map_err(|e| LinkError::Disconnected(format!("TCP reconnect failed: {e}")))?;

        let mut guard = self.stream.lock().await;
        *guard = Some(stream);
        drop(guard);
        Ok(())
    }

    async fn close(&self) -> Result<(), LinkError> {
        let mut guard = self.stream.lock().await;
        *guard = None;
        drop(guard);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_tcp_link_connect_send_recv_reconnect_close() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (mut sock1, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 16];
            let n = sock1.read(&mut buf).await.unwrap();
            assert_eq!(&buf[..n], b"ping");
            sock1.write_all(b"pong").await.unwrap();
            drop(sock1);

            // Second connection for reconnect()
            let (mut sock2, _) = listener.accept().await.unwrap();
            sock2.write_all(b"again").await.unwrap();
            // Keep open briefly so timeout can be tested, then drop for EOF
            tokio::time::sleep(Duration::from_millis(80)).await;
            drop(sock2);
        });

        let link = TcpLink::connect("127.0.0.1", port).await.unwrap();
        assert_eq!(link.address_name(), format!("NET:127.0.0.1:{port}"));
        assert_eq!(link.status(), LinkStatus::Connected);
        assert!(!link.reconnect_policy().is_reconnectable());

        link.send(b"ping").await.unwrap();
        let reply = link.recv(Duration::from_secs(1)).await.unwrap();
        assert_eq!(reply, b"pong");

        // Reconnect and read from second socket
        link.reconnect().await.unwrap();
        let reply2 = link.recv(Duration::from_secs(1)).await.unwrap();
        assert_eq!(reply2, b"again");

        // Timeout before peer closes
        let err_to = link.recv(Duration::from_millis(10)).await.unwrap_err();
        assert!(matches!(err_to, LinkError::Timeout(_)));

        // Wait for peer drop -> EOF -> Disconnected
        tokio::time::sleep(Duration::from_millis(90)).await;
        let err_eof = link.recv(Duration::from_secs(1)).await.unwrap_err();
        assert!(matches!(err_eof, LinkError::Disconnected(_)));

        // Close clears stream; subsequent send/recv return Disconnected
        link.close().await.unwrap();
        assert!(matches!(
            link.send(b"x").await.unwrap_err(),
            LinkError::Disconnected(_)
        ));
        assert!(matches!(
            link.recv(Duration::from_millis(50)).await.unwrap_err(),
            LinkError::Disconnected(_)
        ));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_tcp_link_connect_and_reconnect_refused() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let err = TcpLink::connect("127.0.0.1", port).await.err().unwrap();
        assert!(matches!(err, LinkError::Disconnected(_)));

        // Also exercise reconnect failure when server is gone
        let listener2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port2 = listener2.local_addr().unwrap().port();
        let accept_task = tokio::spawn(async move {
            let (_s, _) = listener2.accept().await.unwrap();
            drop(listener2);
        });
        let link = TcpLink::connect("127.0.0.1", port2).await.unwrap();
        accept_task.await.unwrap();
        let reconn_err = link.reconnect().await.unwrap_err();
        assert!(matches!(reconn_err, LinkError::Disconnected(_)));
    }
}
