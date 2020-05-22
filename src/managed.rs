// Copyright 2017-2020 Aron Heinecke
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::*;
use ::std::net::{SocketAddr, ToSocketAddrs};
use ::std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use snafu::{OptionExt, ResultExt};

pub const ERR_NAME_TAKEN: usize = 513;
const MAX_LEN_NAME: usize = 20;

// Safety: see module tick interval
const TIMEOUT_CONN: Duration = Duration::from_millis(1500);
const TIMEOUT_CMD: Duration = Duration::from_millis(1500);
/// Same as super::CLIENT_CONN_ID, but TS returns a different one on whoami
const KEY_CLIENT_ID_SELF: &str = "client_id";

#[derive(Clone)]
pub struct ManagedConfig {
    addr: SocketAddr,
    user: String,
    password: String,
    server_port: u16,
    conn_timeout: Duration,
    cmd_timeout: Duration,
    name: Option<String>,
}

impl ManagedConfig {
    /// Create a new ManagedConfig wth default values
    pub fn new<A: ToSocketAddrs>(
        addr: A,
        server_port: u16,
        user: String,
        password: String,
    ) -> Result<Self> {
        Ok(Self {
            addr: addr
                .to_socket_addrs()
                .context(Io {
                    context: "invalid socket address",
                })?
                .next()
                .context(InvalidSocketAddress {})?,
            user,
            password,
            server_port,
            name: Default::default(),
            conn_timeout: TIMEOUT_CONN,
            cmd_timeout: TIMEOUT_CMD,
        })
    }

    pub fn name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    pub fn connection_timeout(mut self, timeout: Duration) -> Self {
        self.conn_timeout = timeout;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.cmd_timeout = timeout;
        self
    }
}

/// QueryClient wrapper with connection-check on access
pub struct ManagedConnection {
    cfg: ManagedConfig,
    conn: QueryClient,
    last_ping: Instant,
    conn_id: Option<TsConID>,
}

/// Ts client connection ID
pub type TsConID = i32;

impl ManagedConnection {
    fn connect(cfg: &ManagedConfig) -> Result<QueryClient> {
        // let mut conn = QueryClient::new((cfg.ip.as_ref(), cfg.port))?;
        let mut conn =
            QueryClient::with_timeout(&cfg.addr, Some(cfg.conn_timeout), Some(cfg.cmd_timeout))?;
        conn.login(&cfg.user, &cfg.password)?;
        conn.select_server_by_port(cfg.server_port)?;
        if let Some(n) = cfg.name.as_ref() {
            // prevent underflow in name fallback
            if n.len() > MAX_LEN_NAME {
                return InvalidNameLength {
                    length: n.len(),
                    expected: MAX_LEN_NAME,
                }
                .fail();
            }
            Self::set_name_fallback(&mut conn, n)?;
        }
        Ok(conn)
    }

    /// Set name of client, fallback to name+last unix timestamp MS to make it unique
    fn set_name_fallback(conn: &mut QueryClient, name: &str) -> Result<()> {
        if let Err(e) = conn.rename(name) {
            if e.error_response().map_or(true, |r| r.id != ERR_NAME_TAKEN) {
                return Err(e.into());
            } else {
                conn.rename(&Self::calc_name_retry(name))?;
            }
        }
        Ok(())
    }

    /// Calculate new name on retry
    fn calc_name_retry(name: &str) -> String {
        // leave room for 2 digits at least
        let name = if name.len() >= MAX_LEN_NAME - 2 {
            &name[0..MAX_LEN_NAME / 2]
        } else {
            name
        };
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();
        let reamining = MAX_LEN_NAME - name.len();
        let time = if reamining > time.len() {
            &time
        } else {
            &time.as_str()[time.len() - reamining..]
        };

        format!("{}{}", name, time)
    }

    /// Returns the current connection id (clid)
    pub fn conn_id(&mut self) -> Result<TsConID> {
        Ok(match self.conn_id {
            Some(v) => v,
            None => {
                let res = self.get()?.whoami(false)?;
                let clid = res
                    .get(KEY_CLIENT_ID_SELF)
                    .with_context(|| NoValueResponse {
                        key: KEY_CLIENT_ID_SELF,
                    })?;
                let clid = clid
                    .parse()
                    .with_context(|| InvalidIntResponse { data: clid })?;
                self.conn_id = Some(clid);
                clid
            }
        })
    }

    /// Try creating a second connection
    pub fn clone(&self, new_name: Option<String>) -> Result<Self> {
        let mut cfg = self.cfg.clone();
        if new_name.is_some() {
            cfg.name = new_name;
        }
        Self::new(self.cfg.clone())
    }

    /// Create new TS-Connection with an optional name
    pub fn new(config: ManagedConfig) -> Result<ManagedConnection> {
        let conn = Self::connect(&config)?;
        Ok(Self {
            conn,
            cfg: config,
            last_ping: Instant::now(),
            conn_id: None,
        })
    }

    /// Force reconnect
    pub fn force_reconnect(&mut self) -> Result<()> {
        self.conn = Self::connect(&self.cfg)?;
        self.conn_id = None;
        Ok(())
    }

    /// Returns the active connection or tries to create a new one
    pub fn get(&mut self) -> Result<&mut QueryClient> {
        if self.last_ping.elapsed() < Duration::from_secs(0) {
            return Ok(&mut self.conn);
        }
        let conn = match self.conn.ping() {
            Ok(_) => &mut self.conn,
            Err(_) => {
                self.force_reconnect()?;
                &mut self.conn
            }
        };
        self.last_ping = Instant::now();
        Ok(conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_empty() {
        let name = ManagedConnection::calc_name_retry("");
        assert!(name.len() <= MAX_LEN_NAME);
        assert!(name.len() > 0);
        dbg!(name);
    }

    #[test]
    fn test_name_fallback_normal() {
        // normal name, enough space for time-digits
        let name = ManagedConnection::calc_name_retry("ct_bot-fallback");

        assert!(name.starts_with("ct_bot-fallback"));
        assert!(name.len() <= MAX_LEN_NAME);
        assert!(name.len() > "ct_bot-fallback".len());
        dbg!(name);
    }

    #[test]
    fn test_name_fallback_underflow() {
        // don't take timeString[-1...], just timeStirng[0...] in that case
        let name = ManagedConnection::calc_name_retry("ct_bot");

        assert!(name.starts_with("ct_bot"));
        assert!(name.len() <= MAX_LEN_NAME);
        assert!(name.len() > "ct_bot".len());
        dbg!(name);
    }

    #[test]
    fn test_name_fallback_fit() {
        {
            // no space left, should make space for name
            let name_input = "1234567890123456789D";
            let name = ManagedConnection::calc_name_retry(name_input);
            dbg!(&name);
            assert!(name.starts_with(&name_input[..MAX_LEN_NAME / 2]));
            assert!(name.len() <= MAX_LEN_NAME);
        }

        // required for near-fit invariant
        assert!(MAX_LEN_NAME > 3);
        {
            // assert even for non-fit we have at least 2 random digits at the end
            let name_input = "123456789012345678";
            let name = ManagedConnection::calc_name_retry(name_input);
            dbg!(&name);
            assert!(name.starts_with(&name_input[..MAX_LEN_NAME / 2]));
            assert!(name.len() <= MAX_LEN_NAME);
        }
    }

    #[test]
    fn test_name_fallback_overflow() {
        // assert even for non-fit we have at least 2 random digits at the end
        let name_input = "1234567890123456789012345678901234567890";
        assert!(name_input.len() > MAX_LEN_NAME);
        let name = ManagedConnection::calc_name_retry(name_input);
        dbg!(&name);
        assert!(name.starts_with(&name_input[..MAX_LEN_NAME / 2]));
        assert!(name.len() <= MAX_LEN_NAME);
    }
}
