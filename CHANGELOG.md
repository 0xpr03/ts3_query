### 0.1.6
- Add new with_timeout constructor allowing to define connection/runtime timeouts
- Upgrade snafu to 0.6
- Use backtraces in errors, can be enabled using `ts3-query = { features = "backtrace" }`
- Accept &str/String/&String in raw escape helpers
- *Breaking*: `whoami` takes a paramter for unescaping all values

### 0.1.4
- Add `raw::parse_multi_hashmap` to handle `clientlist` like commands.
- Add testing for hashmap parsers.
- Remove a safe but unrequired use of unsafe.
- Add invariant test for unescaping function.

### 0.1.3
Only doc fixes and a raw-cmd example