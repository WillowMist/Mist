//! Connect dialog state (a simple modal-style prompt inside the toolbar).

#[derive(Debug, Clone)]
pub struct ConnectPrompt {
    /// Host (DNS name or IP string) and port. Stored as strings so the
    /// caller can pass either form to tokio::TcpStream::connect.
    pub host: String,
    pub port: String,
}

impl Default for ConnectPrompt {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            // Matches `mist-mock`'s own default (MOCK_MUD_PORT), so
            // launching the mock with no env vars and accepting the
            // dialog defaults connects. See mist-mock/src/main.rs.
            port: "4000".into(),
        }
    }
}

impl ConnectPrompt {
    /// Validates the port only. DNS resolution happens inside tokio at connect time.
    pub fn parse_port(&self) -> Option<u16> {
        self.port.parse().ok()
    }

    /// Returns `(host, port)` ready for `TcpStream::connect((host_str, port))`.
    pub fn into_target(&self) -> Option<(String, u16)> {
        let port = self.parse_port()?;
        let host = self.host.trim();
        if host.is_empty() {
            return None;
        }
        Some((host.to_string(), port))
    }
}