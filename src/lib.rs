//! Ts3 query library  
//! Small, bare-metal ts query lib without any callback support currently.
//!
//! A connectivity checking wrapper is available under [managed](managed) when enabling its feature.
//!
//! # Examples
//! Simple auth + clients of a server group
//! ```rust,no_run
//! use ts3_query::*;
//!
//! # fn main() -> Result<(),Ts3Error> {
//! let mut client = QueryClient::new("localhost:10011")?;
//!
//! client.login("serveradmin", "password")?;
//! client.select_server_by_port(9987)?;
//!
//! let group_clients = client.get_servergroup_client_list(7)?;
//! println!("Got clients in group 7: {:?}",group_clients);
//!
//! client.logout()?;
//! # Ok(())
//! # }
//!
//! ```
//!
//! Using the raw interface for setting client descriptions.
//! ```rust,no_run
//! use ts3_query::*;
//!
//! # fn main() -> Result<(),Ts3Error> {
//! let mut client = QueryClient::new("localhost:10011")?;
//!
//! client.login("serveradmin", "password")?;
//! client.select_server_by_port(9987)?;
//!
//! // escape things like string args, not required for clid
//! // as it's not user input/special chars in this case
//! let cmd = format!("clientedit clid={} client_description={}",
//!  7, raw::escape_arg("Some Description!")
//! );
//! // we don't expect any value returned
//! let _ = client.raw_command(&cmd)?;
//!
//! client.logout()?;
//! # Ok(())
//! # }
//! ```
//!
//! Raw interface example retrieving online client names
//! ```rust,no_run
//! use ts3_query::*;
//! use std::collections::HashSet;
//!
//! # fn main() -> Result<(),Ts3Error> {
//! let mut client = QueryClient::new("localhost:10011")?;
//!
//! client.login("serveradmin", "password")?;
//! client.select_server_by_port(9987)?;
//!
//! let res = raw::parse_multi_hashmap(client.raw_command("clientlist")?, false);
//! let names = res
//!     .into_iter()
//!     .map(|e| e.get("client_nickname").map(raw::unescape_val)
//!      // may want to catch this in a real application
//!         .unwrap())
//!     .collect::<HashSet<String>>();
//! println!("{:?}",names);
//! client.logout()?;
//! # Ok(())
//! # }
//! ```
#![cfg_attr(docsrs, feature(doc_cfg))]
use snafu::{Backtrace, OptionExt, ResultExt, Snafu};
use std::collections::HashMap;
use std::fmt::{Debug, Write as FmtWrite};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::string::FromUtf8Error;
use std::time::Duration;

#[cfg_attr(docsrs, doc(cfg(feature = "managed")))]
#[cfg(feature = "managed")]
pub mod managed;
pub mod raw;
use io::Read;
use raw::*;
use std::fmt;

pub type ServerGroupID = i32;
pub type ChannelId = i32;
/// Temporary, per connection ID of a client, reused upon disconnect.  
/// Not to be confused with a client database, myteamspeak or identity ID.
pub type ClientId = u16;

/// Target for message sending
pub enum MessageTarget {
    /// Send to client
    Client(ClientId),
    /// Send to current channel of this client. You have to join the channel you want to send a message to.
    Channel,
    /// Send to whole server
    Server,
}

impl fmt::Display for MessageTarget {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Client(id) => write!(f, "targetmode=1 target={}", id),
            Self::Channel => write!(f, "targetmode=2"),
            Self::Server => write!(f, "targetmode=3"),
        }
    }
}

#[derive(Snafu, Debug)]
pub enum Ts3Error {
    /// Error on response conversion with invalid utf8 data
    #[snafu(display("Input was invalid UTF-8: {}", source))]
    Utf8Error { source: FromUtf8Error },
    /// Catch-all IO error, contains optional context
    #[snafu(display("IO Error: {}{}, kind: {:?}", context, source,source.kind()))]
    Io {
        /// Context of action, empty per default.
        ///
        /// Please use a format like `"reading connection: "`
        context: &'static str,
        source: io::Error,
    },
    /// Reached EOF reading response, server closed connection / timeout.
    #[snafu(display("IO Error: Connection closed"))]
    ConnectionClosed { backtrace: Backtrace },
    #[snafu(display("No valid socket address provided."))]
    InvalidSocketAddress { backtrace: Backtrace },
    /// Invalid response error. Server returned unexpected data.
    #[snafu(display("Received invalid response, {}{:?}", context, data))]
    InvalidResponse {
        /// Context of action, empty per default.
        ///
        /// Please use a format like `"expected XY, got: "`
        context: &'static str,
        data: String,
    },
    #[cfg(feature = "managed")]
    #[snafu(display("Got invalid int response {}: {}", data, source))]
    InvalidIntResponse {
        data: String,
        source: std::num::ParseIntError,
        backtrace: Backtrace,
    },
    /// TS3-Server error response
    #[snafu(display("Server responded with error: {}", response))]
    ServerError {
        response: ErrorResponse,
        backtrace: Backtrace,
    },
    /// Maximum amount of response bytes/lines reached, DDOS limit prevented further data read.
    ///
    /// This will probably cause the current connection to become invalid due to remaining data in the connection.
    #[snafu(display("Invalid response, DDOS limit reached: {:?}", response))]
    ResponseLimit {
        response: Vec<String>,
        backtrace: Backtrace,
    },
    /// Invalid name length. Client-Name is longer than allowed!
    #[cfg(feature = "managed")]
    #[snafu(display("Invalid name length: {} max: {}!", length, expected))]
    InvalidNameLength { length: usize, expected: usize },
    /// No client ID found!
    #[snafu(display("Expected entry for key {}, found none!", key))]
    NoValueResponse {
        key: &'static str,
        backtrace: Backtrace,
    },
}

impl Ts3Error {
    /// Returns true if the error is of kind ServerError
    pub fn is_error_response(&self) -> bool {
        match self {
            Ts3Error::ServerError { .. } => true,
            _ => false,
        }
    }
    /// Returns the [`ErrorResponse`](ErrorResponse) if existing.
    pub fn error_response(&self) -> Option<&ErrorResponse> {
        match self {
            Ts3Error::ServerError { response, .. } => Some(response),
            _ => None,
        }
    }
}

impl From<io::Error> for Ts3Error {
    fn from(error: io::Error) -> Self {
        Ts3Error::Io {
            context: "",
            source: error,
        }
    }
}

/// Server error response
#[derive(Debug)]
pub struct ErrorResponse {
    /// Error ID
    pub id: usize,
    /// Error message
    pub msg: String,
}

impl std::fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Error code {}, msg: {}", self.id, self.msg)
    }
}

/// Ts3 Query client with active connection
pub struct QueryClient {
    rx: BufReader<TcpStream>,
    tx: TcpStream,
    limit_lines: usize,
    limit_lines_bytes: u64,
}

/// Default DoS limit for read lines
pub const LIMIT_READ_LINES: usize = 100;
/// Default DoS limit for read bytes per line
pub const LIMIT_LINE_BYTES: u64 = 64_000;

type Result<T> = ::std::result::Result<T, Ts3Error>;

impl Drop for QueryClient {
    fn drop(&mut self) {
        self.quit();
        let _ = self.tx.shutdown(Shutdown::Both);
    }
}

impl QueryClient {
    /// Create new query connection
    pub fn new<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        let (rx, tx) = Self::new_inner(addr, None, None)?;

        Ok(Self {
            rx,
            tx,
            limit_lines: LIMIT_READ_LINES,
            limit_lines_bytes: LIMIT_LINE_BYTES,
        })
    }

    /// Create new query connection with timeouts
    ///
    /// `t_connect` is used for connection, `timeout` for read/write operations
    pub fn with_timeout<A: ToSocketAddrs>(
        addr: A,
        t_connect: Option<Duration>,
        timeout: Option<Duration>,
    ) -> Result<Self> {
        let (rx, tx) = Self::new_inner(addr, timeout, t_connect)?;

        Ok(Self {
            rx,
            tx,
            limit_lines: LIMIT_READ_LINES,
            limit_lines_bytes: LIMIT_LINE_BYTES,
        })
    }

    /// Set new maximum amount of lines to read per response, until DoS protection triggers.
    pub fn limit_lines(&mut self, limit: usize) {
        self.limit_lines = limit;
    }

    /// Set new maximum amount of bytes per line to read until DoS protection triggers.  
    /// You may need to increase this for backup/restore of instances.
    pub fn limit_line_bytes(&mut self, limit: u64) {
        self.limit_lines_bytes = limit;
    }

    /// Rename this client, performs `clientupdate client_nickname` escaping the name
    pub fn rename<T: AsRef<str>>(&mut self, name: T) -> Result<()> {
        writeln!(
            &mut self.tx,
            "clientupdate client_nickname={}",
            escape_arg(name)
        )?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Update client description. If target is none updates this clients description.
    ///
    /// Performs `clientupdate CLIENT_DESCRIPTION` or `clientedit clid=` with `CLIENT_DESCRIPTION` if target is set.
    pub fn update_description<T: AsRef<str>>(
        &mut self,
        descr: T,
        target: Option<ClientId>,
    ) -> Result<()> {
        if let Some(clid) = target {
            writeln!(
                &mut self.tx,
                "clientedit clid={} CLIENT_DESCRIPTION={}",
                clid,
                escape_arg(descr)
            )?;
        } else {
            writeln!(
                &mut self.tx,
                "clientupdate CLIENT_DESCRIPTION={}",
                escape_arg(descr)
            )?;
        }
        let _ = self.read_response()?;
        Ok(())
    }

    /// Poke a client.
    ///
    /// Performs `clientpoke`
    pub fn poke_client<T: AsRef<str>>(&mut self, client: ClientId, msg: T) -> Result<()> {
        writeln!(
            &mut self.tx,
            "clientpoke clid={} msg={}",
            client,
            msg.as_ref()
        )?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Send chat message
    pub fn send_message<T: AsRef<str>>(&mut self, target: MessageTarget, msg: T) -> Result<()> {
        writeln!(
            &mut self.tx,
            "sendtextmessage {} msg={}",
            target,
            escape_arg(msg)
        )?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Send quit command, does not close the socket, not to be exposed
    fn quit(&mut self) {
        let _ = writeln!(&mut self.tx, "quit");
    }

    /// Inner new-function that handles greeting etc
    fn new_inner<A: ToSocketAddrs>(
        addr: A,
        timeout: Option<Duration>,
        conn_timeout: Option<Duration>,
    ) -> Result<(BufReader<TcpStream>, TcpStream)> {
        let addr = addr
            .to_socket_addrs()
            .context(Io {
                context: "invalid socket address",
            })?
            .next()
            .context(InvalidSocketAddress {})?;
        let stream = if let Some(dur) = conn_timeout {
            TcpStream::connect_timeout(&addr, dur).context(Io {
                context: "while connecting: ",
            })?
        } else {
            TcpStream::connect(addr).context(Io {
                context: "while connecting: ",
            })?
        };

        stream.set_write_timeout(timeout).context(Io {
            context: "setting write timeout: ",
        })?;
        stream.set_read_timeout(timeout).context(Io {
            context: "setting read timeout: ",
        })?;

        stream.set_nodelay(true).context(Io {
            context: "setting nodelay: ",
        })?;

        let mut reader = BufReader::new(stream.try_clone().context(Io {
            context: "splitting connection: ",
        })?);

        // read server type token
        let mut buffer = Vec::new();
        reader.read_until(b'\r', &mut buffer).context(Io {
            context: "reading response: ",
        })?;

        buffer.clear();
        if let Err(e) = reader.read_until(b'\r', &mut buffer) {
            use std::io::ErrorKind::*;
            match e.kind() {
                TimedOut | WouldBlock => (), // ignore no greeting
                _ => return Err(e.into()),
            }
        }

        Ok((reader, stream))
    }

    /// Perform a raw command, returns its response as raw value. (No unescaping is performed.)
    ///
    /// You need to escape the command properly.
    pub fn raw_command<T: AsRef<str>>(&mut self, command: T) -> Result<Vec<String>> {
        writeln!(&mut self.tx, "{}", command.as_ref())?;
        let v = self.read_response()?;
        Ok(v)
    }

    /// Performs `whoami`
    ///
    /// Returns a hashmap of entries. Values are unescaped if set.
    pub fn whoami(&mut self, unescape: bool) -> Result<HashMap<String, String>> {
        writeln!(&mut self.tx, "whoami")?;
        let v = self.read_response()?;
        Ok(parse_hashmap(v, unescape))
    }

    /// Logout
    pub fn logout(&mut self) -> Result<()> {
        writeln!(&mut self.tx, "logout")?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Login with provided data
    ///
    /// On drop queryclient issues a logout
    pub fn login<T: AsRef<str>, S: AsRef<str>>(&mut self, user: T, password: S) -> Result<()> {
        writeln!(
            &mut self.tx,
            "login {} {}",
            escape_arg(user),
            escape_arg(password)
        )?;

        let _ = self.read_response()?;

        Ok(())
    }

    /// Select server to perform commands on, by port
    ///
    /// Performs `use port`
    pub fn select_server_by_port(&mut self, port: u16) -> Result<()> {
        writeln!(&mut self.tx, "use port={}", port)?;

        let _ = self.read_response()?;
        Ok(())
    }

    /// Move client to channel with optional channel password
    ///
    /// Performs `clientmove`
    pub fn move_client(
        &mut self,
        client: ClientId,
        channel: ChannelId,
        password: Option<&str>,
    ) -> Result<()> {
        let pw_arg = if let Some(pw) = password {
            format!("cpw={}", raw::escape_arg(pw).as_str())
        } else {
            String::new()
        };
        writeln!(
            &mut self.tx,
            "clientmove clid={} cid={} {}",
            client, channel, pw_arg
        )?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Kick client with specified message from channel/server. Message can't be longer than 40 characters.
    ///
    /// Performs `clientkick`
    pub fn kick_client(
        &mut self,
        client: ClientId,
        server: bool,
        message: Option<&str>,
    ) -> Result<()> {
        let msg_arg = if let Some(pw) = message {
            format!("reasonmsg={}", raw::escape_arg(pw).as_str())
        } else {
            String::new()
        };
        let rid = if server { 5 } else { 4 };
        writeln!(
            &mut self.tx,
            "clientkick clid={} reasonid={} {}",
            client, rid, msg_arg
        )?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Create file directory in channel, has to be a valid path starting with `/`
    ///
    /// Performs `ftcreatedir`
    pub fn create_dir<T: AsRef<str>>(&mut self, channel: ChannelId, path: T) -> Result<()> {
        writeln!(
            &mut self.tx,
            "ftcreatedir cid={} cpw= dirname={}",
            channel,
            escape_arg(path)
        )?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Delete File/Folder in channel, acts recursive on folders
    ///
    /// Example: `/My Directory` deletes everything inside that directory.
    ///
    /// Performs `ftdeletefile`
    pub fn delete_file<T: AsRef<str>>(&mut self, channel: ChannelId, path: T) -> Result<()> {
        writeln!(
            &mut self.tx,
            "ftdeletefile cid={} cpw= name={}",
            channel,
            escape_arg(path)
        )?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Low-cost connection check
    ///
    /// Performs `whoami` command without parsing
    pub fn ping(&mut self) -> Result<()> {
        writeln!(&mut self.tx, "whoami")?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Select server to perform commands on, by server id.
    ///
    /// Performs `use sid`
    pub fn select_server_by_id(&mut self, sid: usize) -> Result<()> {
        writeln!(&mut self.tx, "use sid={}", sid)?;

        let _ = self.read_response()?;
        Ok(())
    }

    /// Performs `servergroupdelclient`  
    /// Removes all client-db-ids in `cldbid` from the specified `group` id.
    pub fn server_group_del_clients(
        &mut self,
        group: ServerGroupID,
        cldbid: &[usize],
    ) -> Result<()> {
        if cldbid.is_empty() {
            return Ok(());
        }
        writeln!(
            &mut self.tx,
            "servergroupdelclient sgid={} {}",
            group,
            Self::format_cldbids(cldbid)
        )?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Performs `servergroupaddclient`  
    /// Ads all specified `cldbid` clients to `group`.
    pub fn server_group_add_clients(
        &mut self,
        group: ServerGroupID,
        cldbid: &[usize],
    ) -> Result<()> {
        if cldbid.is_empty() {
            return Ok(());
        }
        let v = Self::format_cldbids(cldbid);
        writeln!(&mut self.tx, "servergroupaddclient sgid={} {}", group, v)?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Turn a list of client-db-ids into a list of cldbid=X
    fn format_cldbids(it: &[usize]) -> String {
        // would need itertools for format_with

        let mut res = String::new();
        let mut it = it.iter();
        if let Some(n) = it.next() {
            write!(res, "cldbid={}", n).unwrap();
        }
        for n in it {
            write!(res, "|cldbid={}", n).unwrap();
        }
        res
    }

    /// Read response and check error line
    fn read_response(&mut self) -> Result<Vec<String>> {
        let mut result: Vec<String> = Vec::new();
        let mut lr = (&mut self.rx).take(self.limit_lines_bytes);
        for _ in 0..self.limit_lines {
            let mut buffer = Vec::new();
            // damn cargo fmt..
            if lr.read_until(b'\r', &mut buffer).context(Io {
                context: "reading response: ",
            })? == 0
            {
                return ConnectionClosed {}.fail();
            }
            // we read until \r or max-read limit
            if buffer.ends_with(&[b'\r']) {
                buffer.pop();
                if buffer.ends_with(&[b'\n']) {
                    buffer.pop();
                }
            } else if lr.limit() == 0 {
                return ResponseLimit { response: result }.fail();
            } else {
                return InvalidResponse {
                    context: "expected \\r delimiter, got: ",
                    data: String::from_utf8_lossy(&buffer),
                }
                .fail();
            }

            if buffer.len() > 0 {
                let line = String::from_utf8(buffer).context(Utf8Error)?;
                #[cfg(feature = "debug_response")]
                println!("Read: {:?}", &line);
                if line.starts_with("error ") {
                    Self::check_ok(&line)?;
                    return Ok(result);
                }
                result.push(line);
            }
            lr.set_limit(LIMIT_LINE_BYTES);
        }
        ResponseLimit { response: result }.fail()
    }

    /// Get a list of client-DB-IDs for a given server group ID
    ///
    /// See `servergroupclientlist`
    pub fn get_servergroup_client_list(&mut self, group: ServerGroupID) -> Result<Vec<usize>> {
        writeln!(&mut self.tx, "servergroupclientlist sgid={}", group)?;

        let resp = self.read_response()?;
        if let Some(line) = resp.get(0) {
            let data: Vec<usize> = line
                .split('|')
                .map(|e| {
                    if let Some(cldbid) = e.split('=').collect::<Vec<_>>().get(1) {
                        Ok(cldbid
                            .parse::<usize>()
                            .map_err(|_| Ts3Error::InvalidResponse {
                                context: "expected usize, got ",
                                data: line.to_string(),
                            })?)
                    } else {
                        Err(Ts3Error::InvalidResponse {
                            context: "expected data of cldbid=1, got ",
                            data: line.to_string(),
                        })
                    }
                })
                .collect::<Result<Vec<usize>>>()?;
            Ok(data)
        } else {
            Ok(Vec::new())
        }
    }

    /// Check if error line is ok
    fn check_ok(msg: &str) -> Result<()> {
        let result: Vec<&str> = msg.split(' ').collect();
        #[cfg(debug)]
        {
            // should only be invoked on `error` lines, sanity check
            assert_eq!(
                "check_ok invoked on non-error line",
                result.get(0),
                Some(&"error")
            );
        }
        if let (Some(id), Some(msg)) = (result.get(1), result.get(2)) {
            let split_id: Vec<&str> = id.split('=').collect();
            let split_msg: Vec<&str> = msg.split('=').collect();
            if let (Some(id), Some(msg)) = (split_id.get(1), split_msg.get(1)) {
                let id = id.parse::<usize>().map_err(|_| Ts3Error::InvalidResponse {
                    context: "expected usize, got ",
                    data: (*msg).to_string(), // clippy lint
                })?;
                if id != 0 {
                    return ServerError {
                        response: ErrorResponse {
                            id,
                            msg: unescape_val(*msg),
                        },
                    }
                    .fail();
                } else {
                    return Ok(());
                }
            }
        }
        Err(Ts3Error::InvalidResponse {
            context: "expected id and msg, got ",
            data: msg.to_string(),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_format_cldbids() {
        let ids = vec![0, 1, 2, 3];
        assert_eq!(
            "cldbid=0|cldbid=1|cldbid=2|cldbid=3",
            QueryClient::format_cldbids(&ids)
        );
        assert_eq!("", QueryClient::format_cldbids(&[]));
        assert_eq!("cldbid=0", QueryClient::format_cldbids(&ids[0..1]));
    }
}
