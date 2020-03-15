### 0.1.5
- Add new with_timeout constructor allowing to define connection/runtime timeouts
- Upgrade snafu to 0.6
- Use backtraces in errors, can be enabled using `ts3-query = { features = "backtrace" }`

### 0.1.4
- Add `raw::parse_multi_hashmap` to handle `clientlist` like commands.
- Add testing for hashmap parsers.
- Remove a safe but unrequired use of unsafe.
- Add invariant test for unescaping function.

### 0.1.3
Only doc fixes and a raw-cmd example