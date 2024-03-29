### 0.3.2

- Add channel name edit function

### 0.3.1

- Add channel delete and create functions

### 0.3.0
- Add proper types for channel & servergroup IDs, changes some parameter types
- Accept both String and &str on multiple places
- All result parsing has now an optional value to allow for empty values
- Add move_client function
- Add poke_client function
- Add kick_client function
- Add send_message function
- Add server_groups function
- Add online_clients function
- Add online_clients_full function with every flag
- Add channellist function
- Add channellist_full function with every flag
- Add target option to update_description function
- Add server error code descriptions
- Renamed get_servergroup_client_list to servergroup_client_cldbids
- Export default DoS bytes/lines limit constants

### 0.2.3
- Fix update_description function.

### 0.2.2
- Add managed feature and module. Introducing a wrapper for long lived connectons.

### 0.2.1
- Add raw interface example for client name retrieval
- Fix crates.io category to include teamspeak

### 0.2.0
- Fix underflow on read of closed connection. This comes with an adjustable bytes-per-line limit.
- Add new with_timeout constructor allowing to define connection/runtime timeouts
- Upgrade snafu to 0.6
- Use backtraces in errors, can be enabled using `ts3-query = { features = "backtrace" }`
- Accept &str/String/&String in raw escape helpers
- *Breaking*: `whoami` takes a paramter for unescaping all values

### 0.1.5
- Remove println on QueryClient::new, sorry

### 0.1.4
- Add `raw::parse_multi_hashmap` to handle `clientlist` like commands.
- Add testing for hashmap parsers.
- Remove a safe but unrequired use of unsafe.
- Add invariant test for unescaping function.

### 0.1.3
Only doc fixes and a raw-cmd example