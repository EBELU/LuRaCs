//! Mock device link for automated testing of drivers, reconnection, and error handling.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use super::{DeviceLink, LinkError, LinkStatus, ReconnectPolicy};

/// Deterministic mock link for unit tests.
#[derive(Debug, Clone)]
pub struct MockLink {
    name: String,
    policy: ReconnectPolicy,
    state: Arc<Mutex<MockLinkState>>,
}

#[derive(Debug)]
struct MockLinkState {
    status: LinkStatus,
    reconnect_count: u32,
    fail_next_send: Option<LinkError>,
    fail_next_recv: Option<LinkError>,
    fail_next_reconnect: Option<LinkError>,
    incoming_queue: VecDeque<Vec<u8>>,
    sent_packets: Vec<Vec<u8>>,
}

impl MockLink {
    #[must_use]
    pub fn new(name: impl Into<String>, policy: ReconnectPolicy) -> Self {
        Self {
            name: name.into(),
            policy,
            state: Arc::new(Mutex::new(MockLinkState {
                status: LinkStatus::Connected,
                reconnect_count: 0,
                fail_next_send: None,
                fail_next_recv: None,
                fail_next_reconnect: None,
                incoming_queue: VecDeque::new(),
                sent_packets: Vec::new(),
            })),
        }
    }

    /// Default mock link with `never` reconnect policy (like USB).
    #[must_use]
    pub fn usb_mock(name: impl Into<String>) -> Self {
        Self::new(name, ReconnectPolicy::never())
    }

    /// Default mock link with BLE reconnect policy.
    #[must_use]
    pub fn ble_mock(name: impl Into<String>) -> Self {
        Self::new(name, ReconnectPolicy::ble_default())
    }

    /// Enqueue bytes to be returned by future `recv()` calls.
    pub fn push_incoming(&self, data: impl Into<Vec<u8>>) {
        let mut st = self.state.lock().unwrap();
        st.incoming_queue.push_back(data.into());
    }

    /// Retrieve all packets sent through this link so far.
    #[must_use]
    pub fn sent_packets(&self) -> Vec<Vec<u8>> {
        let st = self.state.lock().unwrap();
        st.sent_packets.clone()
    }

    /// Set an error to be returned by the next `send` call.
    pub fn set_fail_next_send(&self, err: Option<LinkError>) {
        let mut st = self.state.lock().unwrap();
        st.fail_next_send = err;
    }

    /// Set an error to be returned by the next `recv` call.
    pub fn set_fail_next_recv(&self, err: Option<LinkError>) {
        let mut st = self.state.lock().unwrap();
        st.fail_next_recv = err;
    }

    /// Set an error to be returned by the next `reconnect` call.
    pub fn set_fail_next_reconnect(&self, err: Option<LinkError>) {
        let mut st = self.state.lock().unwrap();
        st.fail_next_reconnect = err;
    }

    /// How many times `reconnect()` has been successfully or unsuccessfully invoked.
    #[must_use]
    pub fn reconnect_count(&self) -> u32 {
        let st = self.state.lock().unwrap();
        st.reconnect_count
    }
}

#[async_trait]
impl DeviceLink for MockLink {
    fn address_name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> LinkStatus {
        let st = self.state.lock().unwrap();
        st.status.clone()
    }

    fn reconnect_policy(&self) -> &ReconnectPolicy {
        &self.policy
    }

    async fn send(&self, data: &[u8]) -> Result<(), LinkError> {
        let mut st = self.state.lock().unwrap();
        if let Some(err) = st.fail_next_send.take() {
            st.status = LinkStatus::Disconnected;
            drop(st);
            return Err(err);
        }
        if st.status != LinkStatus::Connected {
            drop(st);
            return Err(LinkError::Disconnected("link is not connected".into()));
        }
        st.sent_packets.push(data.to_vec());
        drop(st);
        Ok(())
    }

    async fn recv(&self, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        let mut st = self.state.lock().unwrap();
        if let Some(err) = st.fail_next_recv.take() {
            st.status = LinkStatus::Disconnected;
            drop(st);
            return Err(err);
        }
        if st.status != LinkStatus::Connected {
            drop(st);
            return Err(LinkError::Disconnected("link is not connected".into()));
        }
        let res = st.incoming_queue.pop_front().ok_or(LinkError::Timeout(timeout));
        drop(st);
        res
    }

    async fn reconnect(&self) -> Result<(), LinkError> {
        let mut st = self.state.lock().unwrap();
        st.reconnect_count += 1;

        if let Some(err) = st.fail_next_reconnect.take() {
            st.status = LinkStatus::Disconnected;
            drop(st);
            return Err(err);
        }

        st.status = LinkStatus::Connected;
        drop(st);
        Ok(())
    }

    async fn close(&self) -> Result<(), LinkError> {
        let mut st = self.state.lock().unwrap();
        st.status = LinkStatus::Disconnected;
        drop(st);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_link_basic_flow() {
        let link = MockLink::usb_mock("MOCK:1");
        assert_eq!(link.status(), LinkStatus::Connected);
        assert!(!link.reconnect_policy().is_reconnectable());

        // Send
        link.send(b"ping").await.unwrap();
        assert_eq!(link.sent_packets(), vec![b"ping".to_vec()]);

        // Recv without data -> Timeout
        let err = link.recv(Duration::from_millis(10)).await.unwrap_err();
        assert_eq!(err, LinkError::Timeout(Duration::from_millis(10)));

        // Recv with data
        link.push_incoming(b"pong");
        let reply = link.recv(Duration::from_millis(10)).await.unwrap();
        assert_eq!(reply, b"pong".to_vec());
    }

    #[tokio::test]
    async fn test_mock_link_disconnect_and_reconnect() {
        let link = MockLink::ble_mock("BLE:Raysid");
        link.set_fail_next_send(Some(LinkError::Disconnected("dropped".into())));

        let err = link.send(b"hello").await.unwrap_err();
        assert_eq!(err, LinkError::Disconnected("dropped".into()));
        assert_eq!(link.status(), LinkStatus::Disconnected);

        // Reconnect
        link.reconnect().await.unwrap();
        assert_eq!(link.status(), LinkStatus::Connected);
        assert_eq!(link.reconnect_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_link_recv_and_reconnect_failures_and_close() {
        let link = MockLink::ble_mock("BLE:ErrTest");
        assert_eq!(link.name, "BLE:ErrTest");

        link.set_fail_next_recv(Some(LinkError::Disconnected("recv dropped".into())));
        let err = link.recv(Duration::from_millis(5)).await.unwrap_err();
        assert_eq!(err, LinkError::Disconnected("recv dropped".into()));
        assert_eq!(link.status(), LinkStatus::Disconnected);

        // Send and recv while disconnected return Disconnected error.
        assert!(matches!(
            link.send(b"x").await,
            Err(LinkError::Disconnected(_))
        ));
        assert!(matches!(
            link.recv(Duration::from_millis(5)).await,
            Err(LinkError::Disconnected(_))
        ));

        // Fail next reconnect
        link.set_fail_next_reconnect(Some(LinkError::Disconnected("reconn fail".into())));
        assert!(matches!(
            link.reconnect().await,
            Err(LinkError::Disconnected(_))
        ));
        assert_eq!(link.status(), LinkStatus::Disconnected);

        // Recover and close
        link.reconnect().await.unwrap();
        assert_eq!(link.status(), LinkStatus::Connected);
        link.close().await.unwrap();
        assert_eq!(link.status(), LinkStatus::Disconnected);
    }
}
