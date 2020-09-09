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

impl ErrorResponse {
    // courtesy of https://yat.qa/resources/server-error-codes/
    /// Returns error name if existing
    pub fn error_name(&self) -> Option<&'static str> {
        match self.id {
            0 => Some("unknown error code"),
            1 => Some("undefined error"),
            2 => Some("not implemented"),
            5 => Some("library time limit reached"),
            256 => Some("command not found"),
            257 => Some("unable to bind network port"),
            258 => Some("no network port available"),
            512 => Some("invalid clientID"),
            513 => Some("nickname is already in use"),
            514 => Some("invalid error code"),
            515 => Some("max clients protocol limit reached"),
            516 => Some("invalid client type"),
            517 => Some("already subscribed"),
            518 => Some("not logged in"),
            519 => Some("could not validate client identity"),
            520 => Some("invalid loginname or password"),
            521 => Some("too many clones already connected"),
            522 => Some("client version outdated, please update"),
            523 => Some("client is online"),
            524 => Some("client is flooding"),
            525 => Some("client is modified"),
            526 => Some("can not verify client at this moment"),
            527 => Some("client is not permitted to log in"),
            528 => Some("client is not subscribed to the channel"),
            768 => Some("invalid channelID"),
            769 => Some("max channels protocol limit reached"),
            770 => Some("already member of channel"),
            771 => Some("channel name is already in use"),
            772 => Some("channel not empty"),
            773 => Some("can not delete default channel"),
            774 => Some("default channel requires permanent"),
            775 => Some("invalid channel flags"),
            776 => Some("permanent channel can not be child of non permanent channel"),
            777 => Some("channel maxclient reached"),
            778 => Some("channel maxfamily reached"),
            779 => Some("invalid channel order"),
            780 => Some("channel does not support filetransfers"),
            781 => Some("invalid channel password"),
            782 => Some("channel is private channel"),
            783 => Some("invalid security hash supplied by client"),
            1024 => Some("invalid serverID"),
            1025 => Some("server is running"),
            1026 => Some("server is shutting down"),
            1027 => Some("server maxclient reached"),
            1028 => Some("invalid server password"),
            1029 => Some("deployment active"),
            1030 => Some("unable to stop own server in your connection class"),
            1031 => Some("server is virtual"),
            1032 => Some("server wrong machineID"),
            1033 => Some("server is not running"),
            1034 => Some("server is booting up"),
            1035 => Some("server got an invalid status for this operation"),
            1036 => Some("server modal quit"),
            1037 => Some("server version is too old for command"),
            1040 => Some("server blacklisted"),
            1280 => Some("database error"),
            1281 => Some("database empty result set"),
            1282 => Some("database duplicate entry"),
            1283 => Some("database no modifications"),
            1284 => Some("database invalid constraint"),
            1285 => Some("database reinvoke command"),
            1536 => Some("invalid quote"),
            1537 => Some("invalid parameter count"),
            1538 => Some("invalid parameter"),
            1539 => Some("parameter not found"),
            1540 => Some("convert error"),
            1541 => Some("invalid parameter size"),
            1542 => Some("missing required parameter"),
            1543 => Some("invalid checksum"),
            1792 => Some("virtual server got a critical error"),
            1793 => Some("Connection lost"),
            1794 => Some("not connected"),
            1795 => Some("no cached connection info"),
            1796 => Some("currently not possible"),
            1797 => Some("failed connection initialization"),
            1798 => Some("could not resolve hostname"),
            1799 => Some("invalid server connection handler ID"),
            1800 => Some("could not initialize Input Manager"),
            1801 => Some("client library not initialized"),
            1802 => Some("server library not initialized"),
            1803 => Some("too many whisper targets"),
            1804 => Some("no whisper targets found"),
            2048 => Some("invalid file name"),
            2049 => Some("invalid file permissions"),
            2050 => Some("file already exists"),
            2051 => Some("file not found"),
            2052 => Some("file input/output error"),
            2053 => Some("invalid file transfer ID"),
            2054 => Some("invalid file path"),
            2055 => Some("no files available"),
            2056 => Some("overwrite excludes resume"),
            2057 => Some("invalid file size"),
            2058 => Some("file already in use"),
            2059 => Some("could not open file transfer connection"),
            2060 => Some("no space left on device (disk full?)"),
            2061 => Some("file exceeds file system's maximum file size"),
            2062 => Some("file transfer connection timeout"),
            2063 => Some("lost file transfer connection"),
            2064 => Some("file exceeds supplied file size"),
            2065 => Some("file transfer complete"),
            2066 => Some("file transfer canceled"),
            2067 => Some("file transfer interrupted"),
            2068 => Some("file transfer server quota exceeded"),
            2069 => Some("file transfer client quota exceeded"),
            2070 => Some("file transfer reset"),
            2071 => Some("file transfer limit reached"),
            2304 => Some("preprocessor disabled"),
            2305 => Some("internal preprocessor"),
            2306 => Some("internal encoder"),
            2307 => Some("internal playback"),
            2308 => Some("no capture device available"),
            2309 => Some("no playback device available"),
            2310 => Some("could not open capture device"),
            2311 => Some("could not open playback device"),
            2312 => Some("ServerConnectionHandler has a device registered"),
            2313 => Some("invalid capture device"),
            2314 => Some("invalid clayback device"),
            2315 => Some("invalid wave file"),
            2316 => Some("wave file type not supported"),
            2317 => Some("could not open wave file"),
            2318 => Some("internal capture"),
            2319 => Some("device still in use"),
            2320 => Some("device already registerred"),
            2321 => Some("device not registered/known"),
            2322 => Some("unsupported frequency"),
            2323 => Some("invalid channel count"),
            2324 => Some("read error in wave"),
            2325 => Some("sound need more data"),
            2326 => Some("sound device was busy"),
            2327 => Some("there is no sound data for this period"),
            2328 => Some("Channelmask set bits count (speakers) is not the same as channel (count)"),
            2560 => Some("invalid group ID"),
            2561 => Some("duplicate entry"),
            2562 => Some("invalid permission ID"),
            2563 => Some("empty result set"),
            2564 => Some("access to default group is forbidden"),
            2565 => Some("invalid size"),
            2566 => Some("invalid value"),
            2567 => Some("group is not empty"),
            2568 => Some("insufficient client permissions"),
            2569 => Some("insufficient group modify power"),
            2570 => Some("insufficient permission modify power"),
            2571 => Some("template group is currently used"),
            2572 => Some("permission error"),
            2816 => Some("virtualserver limit reached"),
            2817 => Some("max slot limit reached"),
            2818 => Some("license file not found"),
            2819 => Some("license date not ok"),
            2820 => Some("unable to connect to accounting server"),
            2821 => Some("unknown accounting error"),
            2822 => Some("accounting server error"),
            2823 => Some("instance limit reached"),
            2824 => Some("instance check error"),
            2825 => Some("license file invalid"),
            2826 => Some("virtualserver is running elsewhere"),
            2827 => Some("virtualserver running in same instance already"),
            2828 => Some("virtualserver already started"),
            2829 => Some("virtualserver not started"),
            3072 => Some("invalid message id"),
            3328 => Some("invalid ban id"),
            3329 => Some("connection failed, you are banned"),
            3330 => Some("rename failed, new name is banned"),
            3331 => Some("flood ban"),
            3584 => Some("unable to initialize tts"),
            3840 => Some("invalid privilege key"),
            4352 => Some("invalid password"),
            4353 => Some("invalid request"),
            4354 => Some("no (more) slots available"),
            4355 => Some("pool missing"),
            4356 => Some("pool unknown"),
            4357 => Some("unknown ip location (perhaps LAN ip?)"),
            4358 => Some("internal error (tried exceeded)"),
            4359 => Some("too many slots requested"),
            4360 => Some("too many reserved"),
            4361 => Some("could not connect to provisioning server"),
            4368 => Some("authentication server not connected"),
            4369 => Some("authentication data too large"),
            4370 => Some("already initialized"),
            4371 => Some("not initialized"),
            4372 => Some("already connecting"),
            4373 => Some("already connected"),
            4375 => Some("io_error"),
            _ => None,
        }
    }
}

impl std::fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(descr) = self.error_name() {
            writeln!(f, "Error {}: {}, msg: {}", self.id,descr, self.msg)
        } else {
            writeln!(f, "Unknown Error code {}, msg: {}", self.id, self.msg)
        }        
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

            if !buffer.is_empty() {
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
    pub fn servergroup_client_cldbids(&mut self, group: ServerGroupID) -> Result<Vec<usize>> {
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
