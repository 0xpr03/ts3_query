//! Ts3 query library
//! Bare-metal without any callback support currently.
//! 
//! ```rust,no_run
//! use ts3_query::*;
//! 
//! # fn main() -> Result<(),Ts3Error> {
//! let mut client = QueryClient::new("localhost:10011")?;
//! 
//! client.login("serveradmin", "password")?;
//! client.select_server_by_port(9987)?;
//! 
//! let clients = client.get_servergroup_client_list(7)?;
//! println!("Got clients in group 7: {:?}",clients);
//! 
//! client.logout()?;
//! # Ok(())
//! # }
//! 
//! ```
use snafu::{ResultExt, Snafu};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::string::FromUtf8Error;
use std::time::Duration;

pub mod raw;
use raw::*;

#[derive(Snafu, Debug)]
pub enum Ts3Error {
    /// Error on response conversion with invalid utf8 data
    #[snafu(display("Input was invalid UTF-8: {}", source))]
    Utf8Error { source: FromUtf8Error },
    /// Catch-all IO error, contains optional context
    #[snafu(display("IO Error: {}{}", context, source))]
    Io {
        /// Context of action, empty per default.
        ///
        /// Please use a format like `"reading connection: "`
        context: &'static str,
        source: io::Error,
    },
    /// Invalid response error,
    #[snafu(display("Received invalid response: {}", data))]
    InvalidResponse { context: &'static str, data: String },
    #[snafu(display("Server error: {}", response))]
    ServerError { response: ErrorResponse },
    /// Maximum amount of response lines reached, DDOS limit prevented further data read.
    ///
    /// This will probably cause the current connection to be come invalid due to remaining data in the connection.
    #[snafu(display("Invalid response, too many lines, DDOS limit reached: {:?}", response))]
    ResponseLimit { response: Vec<String> },
}

// impl Ts3Error {
//     pub fn is_connection_error(&self) -> bool {
//         match self {
//             Io(_) => true,
//             _ => false,
//         }
//     }
// }

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
        write!(f, "Error code {}, msg:{}", self.id, self.msg)
    }
}

/// Ts3 Query client with active connection
pub struct QueryClient {
    rx: BufReader<TcpStream>,
    tx: TcpStream,
}

const MAX_TRIES: usize = 100;

type Result<T> = ::std::result::Result<T, Ts3Error>;

impl Drop for QueryClient {
    fn drop(&mut self) {
        let _ = self.logout();
    }
}

impl QueryClient {
    /// Create new query connection
    pub fn new<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        let (rx,tx) = Self::new_inner(addr)?;

        Ok(Self {
            rx,
            tx,
        })
    }

    fn new_inner<A: ToSocketAddrs>(addr: A) -> Result<(BufReader<TcpStream>, TcpStream)> {
        let stream = TcpStream::connect(addr).context(Io {
            context: "whie connecting: ",
        })?;

        stream
            .set_write_timeout(Some(Duration::new(5, 0)))
            .context(Io {
                context: "setting write timeout: ",
            })?;
        stream
            .set_read_timeout(Some(Duration::new(5, 0)))
            .context(Io {
                context: "setting read timeout: ",
            })?;
        stream.set_nodelay(true).context(Io {
            context: "setting nodelay: ",
        })?;

        let reader = BufReader::new(stream.try_clone().context(Io {
            context: "splitting connection: ",
        })?);
        Ok((reader,stream))
    }

    /// Perform a raw command, returns its response
    ///
    /// You need to escape the command properly.
    pub fn raw_command(&mut self, command: &str) -> Result<Vec<String>> {
        write!(&mut self.tx, "{}\n\r", command)?;
        let v = self.read_response()?;
        Ok(v)
    }

    /// Performs whoami
    ///
    /// Returns a hashmap of entries
    pub fn whoami(&mut self) -> Result<HashMap<String, String>> {
        write!(&mut self.tx, "whoami\n\r")?;
        let v = self.read_response()?;
        Ok(parse_hashmap(v, false))
    }

    /// Logout
    pub fn logout(&mut self) -> Result<()> {
        write!(&mut self.tx, "logout\n\r")?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Login with provided data
    /// 
    /// On drop queryclient issues a logout
    pub fn login(&mut self, user: &str, password: &str) -> Result<()> {
        write!(
            &mut self.tx,
            "login {} {}\n\r",
            escape_arg(user),
            escape_arg(password)
        )?;

        let _ = self.read_response()?;

        Ok(())
    }

    /// Select server to perform commands on, by port
    pub fn select_server_by_port(&mut self, port: u16) -> Result<()> {
        write!(&mut self.tx, "use port={}\n\r", port)?;

        let _ = self.read_response()?;
        Ok(())
    }

    /// Create file directory in channel, has to be a valid path starting with `/`
    ///
    /// Performs ftcreatedir
    pub fn create_dir(&mut self, cid: usize, path: &str) -> Result<()> {
        write!(
            &mut self.tx,
            "ftcreatedir cid={} cpw= dirname={}\n\r",
            cid,
            escape_arg(path)
        )?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Delete File/Folder in channel, acts recursive on folders
    ///
    /// Example: `/My Directory` deletes everything inside that directory.
    ///
    /// Performs ftdeletefile
    pub fn delete_file(&mut self, cid: usize, path: &str) -> Result<()> {
        write!(
            &mut self.tx,
            "ftdeletefile cid={} cpw= name={}\n\r",
            cid,
            escape_arg(path)
        )?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Low-cost connection check
    ///
    /// Performs whoami command without parsing
    pub fn ping(&mut self) -> Result<()> {
        write!(&mut self.tx, "whoami\n\r")?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Select server to perform commands on, by server id
    pub fn select_server_by_id(&mut self, sid: usize) -> Result<()> {
        write!(&mut self.tx, "use sid={}\n\r", sid)?;

        let _ = self.read_response()?;
        Ok(())
    }

    /// Performs servergroupdelclient
    pub fn server_group_del_clients(&mut self, group: usize, cldbid: &[usize]) -> Result<()> {
        if cldbid.is_empty() {
            return Ok(());
        }
        write!(
            &mut self.tx,
            "servergroupdelclient sgid={} {}\n\r",
            group,
            Self::format_cldbids(cldbid)
        )?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Performs servergroupaddclient
    pub fn server_group_add_clients(&mut self, group: usize, cldbid: &[usize]) -> Result<()> {
        if cldbid.is_empty() {
            return Ok(());
        }
        let v = Self::format_cldbids(cldbid);
        write!(
            &mut self.tx,
            "servergroupaddclient sgid={} {}\n\r",
            group, v
        )?;
        let _ = self.read_response()?;
        Ok(())
    }

    /// Turn a list of
    fn format_cldbids(it: &[usize]) -> String {
        // it.iter().format_with("|", |x, f| f(&format_args!("cldbid={}", x))).to_string()
        let mut res: Vec<u8> = Vec::new();
        let mut it = it.iter();
        if let Some(n) = it.next() {
            write!(res, "cldbid={}", n).unwrap();
        }
        for n in it {
            write!(res, "|cldbid={}", n).unwrap();
        }
        unsafe {
            // we know this is utf8 as we only added utf8 strings using fmt
            String::from_utf8_unchecked(res)
        }
    }

    /// Read response and check for error
    fn read_response(&mut self) -> Result<Vec<String>> {
        let mut result: Vec<String> = Vec::new();
        for _ in 0..MAX_TRIES {
            let mut buffer = Vec::with_capacity(20);
            // \n\r line ending
            while {
                self.rx.read_until(b'\r', &mut buffer).context(Io {
                    context: "reading response: ",
                })?;

                // check for exact '\n\r'
                buffer.get(buffer.len() - 2).map_or(false, |v| *v == b'\n')
            } {}
            // remove \n\r
            buffer.pop();
            buffer.pop();

            let buffer = String::from_utf8(buffer).context(Utf8Error)?;
            if buffer.starts_with("error ") {
                Self::check_ok(&buffer)?;
                return Ok(result);
            }
            result.push(buffer);
        }
        Err(Ts3Error::ResponseLimit { response: result })
    }

    /// Get all client database IDs for a given server group ID
    ///
    /// See servergroupclientlist
    pub fn get_servergroup_client_list(&mut self, server_group: usize) -> Result<Vec<usize>> {
        write!(
            &mut self.tx,
            "servergroupclientlist sgid={}\n\r",
            server_group
        )?;

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
                    data: msg.to_string(),
                })?;
                if id != 0 {
                    return Err(Ts3Error::ServerError {
                        response: ErrorResponse {
                            id,
                            msg: unescape_val(*msg),
                        },
                    });
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