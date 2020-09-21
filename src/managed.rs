//! Managed connection module.
//! Handles reconnection and name uniqueness.
//! Wraps a normal query connection with health checks. Handles renaming and claimed names.
//! Useful if running long-lasting connections which tend to break over the wire.
//! ```rust,no_run
//! use ts3_query::*;
//! # fn main() -> Result<(),Ts3Error> {
//! let cfg = managed::ManagedConfig::new("127.0.0.1:10011",9987,"serveradmin".into(),"asdf".into())?
//!     .name("my bot".to_string());
//! let mut conn = managed::ManagedConnection::new(cfg)?;
//! // get inner connection with check for being alive
//! // then perform a command on it
//! let _ = conn.get()?.whoami(false)?;
//! # Ok(())
//! # }
//! ```

use crate::*;
use ::std::net::{SocketAddr, ToSocketAddrs};
use ::std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use snafu::{OptionExt, ResultExt};

const ERR_NAME_TAKEN: usize = 513;
/// Max name length to allow unique names
pub const MAX_LEN_NAME: usize = 20;

/// Default connection timeout
pub const DEFAULT_TIMEOUT_CONN: Duration = Duration::from_millis(1500);
/// Default timeout for sending/receiving
pub const DEFAULT_TIMEOUT_CMD: Duration = Duration::from_millis(1500);
/// Same as super::CLIENT_CONN_ID, but TS returns a different one on whoami
const KEY_CLIENT_ID_SELF: &str = "client_id";

/// Config for creating a managed connection
/// ```rust
/// # use ts3_query::managed::ManagedConfig;
/// # use ts3_query::*;
/// # fn main() -> Result<(),Ts3Error> {
/// use std::time::Duration;
/// let cfg = ManagedConfig::new("127.0.0.1:10011",9987,"serveradmin".into(),"asdf".into())?
/// .name("my test bot".to_string())
/// .connection_timeout(Duration::from_secs(1))
/// .timeout(Duration::from_secs(1));
/// # Ok(()) }
/// ```
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
    /// Create a new ManagedConfig with default values
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
            conn_timeout: DEFAULT_TIMEOUT_CONN,
            cmd_timeout: DEFAULT_TIMEOUT_CMD,
        })
    }

    /// Set name of client for connection  
    /// All names have to be shorter than MAX_LEN_NAME.
    /// This is required as you always have to leave enough space for teamspeak
    /// to allow appending a unique number. Otherwise connections will fail
    /// if the name is already claimed and too long to be made unique.  
    /// This is a limitation of the teamspeak API.
    pub fn name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    /// Set connection timeout
    pub fn connection_timeout(mut self, timeout: Duration) -> Self {
        self.conn_timeout = timeout;
        self
    }

    /// Set timeout for normal IO
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
    conn_id: Option<ClientId>,
}

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
                return Err(e);
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
    pub fn conn_id(&mut self) -> Result<ClientId> {
        Ok(match self.conn_id {
            Some(v) => v,
            None => {
                let mut res = self.get()?.whoami(false)?;
                let clid = crate::raw::int_val_parser(&mut res, KEY_CLIENT_ID_SELF)?;
                self.conn_id = Some(clid);
                clid
            }
        })
    }

    /// Try creating a second connection, based on the configs of this one.
    /// `new_name` can specifiy a different connection client name.
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

    /// Force reconnect, may be called if server returns invalid data on call.
    /// Can happen if for example the firewall just drops packages for some time.
    pub fn force_reconnect(&mut self) -> Result<()> {
        self.conn = Self::connect(&self.cfg)?;
        self.conn_id = None;
        Ok(())
    }

    /// Returns the active connection or fallbacks to reconnect
    /// Checks for connection health every 1 second between a get() call.
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
