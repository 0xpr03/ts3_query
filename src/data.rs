use std::collections::HashMap;
use crate::Result;
use crate::raw::*;

pub type ServerGroupID = i32;
pub type ChannelId = i32;
/// Temporary, per connection ID of a client, reused upon disconnect.  
/// Not to be confused with a client database, myteamspeak or identity ID.
pub type ClientId = u16;
/// Server interal ID for client, not it's Identity / MyTeamspeak ID.
pub type ClientDBId = i32;
pub type ChannelGroupId = i32;

/// Server Group returned from `server_group_list`. Field names are according to the query protocol.
#[derive(Debug)]
pub struct ServerGroup {
    /// Identifier for this server group
    pub sgid: ServerGroupID,
    pub name: String,
    /// `type` use `r#type` to specify in rust
    pub r#type: i32,
    pub iconid: i32,
    // ?!
    pub savedb: i32,
}

impl ServerGroup {
    /// Create struct from raw line-data assuming no unescaping was performed
    pub(crate) fn from_raw(mut data: HashMap<String,String>) -> Result<Self> {
        let sgid = int_val_parser(&mut data, "sgid")?;
        let name: String = string_val_parser(&mut data, "name")?;
        let r#type: i32 = int_val_parser(&mut data, "type")?;
        let iconid: i32 = int_val_parser(&mut data, "iconid")?;
        let savedb: i32 = int_val_parser(&mut data, "savedb")?;

        Ok(ServerGroup{sgid,name,r#type,iconid,savedb})
    }
}

#[derive(Debug)]
pub struct OnlineClient {
    pub clid: ClientId,
    pub cid: ChannelId,
    pub client_database_id: ClientDBId,
    pub client_nickname: String,
    pub client_type: i8,
}

impl OnlineClient {
    pub(crate) fn from_raw(mut data: HashMap<String, String>) -> Result<Self> {
        let clid = int_val_parser(&mut data, "clid")?;
        let cid = int_val_parser(&mut data, "cid")?;
        let client_database_id = int_val_parser(&mut data, "client_database_id")?;
        let client_nickname: String = string_val_parser(&mut data, "client_nickname")?;
        let client_type = int_val_parser(&mut data, "client_type")?;

        Ok(OnlineClient {
            clid,
            cid,
            client_database_id,
            client_nickname,
            client_type,
        })
    }
}

#[derive(Debug)]
pub struct OnlineClientFull {
    pub clid: ClientId,
    pub cid: ChannelId,
    pub client_database_id: ClientDBId,
    pub client_nickname: String,
    pub client_type: i8,
    pub client_away: bool,
    pub client_away_message: Option<String>,
    pub client_flag_talking: bool,
    pub client_input_muted: bool,
    pub client_output_muted: bool,
    pub client_input_hardware: bool,
    pub client_output_hardware: bool,
    pub client_talk_power: i32,
    pub client_is_talker: bool,
    pub client_is_priority_speaker: bool,
    pub client_is_recording: bool,
    pub client_is_channel_commander: bool,
    pub client_unique_identifier: String,
    pub client_servergroups: Vec<ServerGroupID>,
    pub client_channel_group_id: ChannelGroupId,
    pub client_channel_group_inherited_channel_id: ChannelGroupId,
    pub client_version: String,
    pub client_platform: String,
    pub client_idle_time: i32,
    pub client_created: i64,
    pub client_lastconnected: i64,
    pub client_country: String,
    pub connection_client_ip: String,
    pub client_badges: Option<String>,
}

impl OnlineClientFull {
    // fn from_raw(mut data: HashMap<String, String>) -> Result<Self> {
    //     let clid = raw::int_val_parser(&mut data, "clid")?;
    //     let cid = raw::int_val_parser(&mut data, "cid")?;
    //     let client_database_id = raw::int_val_parser(&mut data, "client_database_id")?;
    //     let client_nickname: String = raw::string_val_parser(&mut data, "client_nickname")?;
    //     let client_type = raw::int_val_parser(&mut data, "client_type")?;

    //     Ok(OnlineClient {
    //         clid,
    //         cid,
    //         client_database_id,
    //         client_nickname,
    //         client_type,
    //     })
    // }
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