## Ts3 Query Library

[![crates.io](https://img.shields.io/crates/v/ts3-query.svg)](https://crates.io/crates/ts3-query)
[![docs.rs](https://docs.rs/ts3-query/badge.svg)](https://docs.rs/ts3-query)
[![Build Status](https://api.travis-ci.com/0xpr03/ts3_query.svg?branch=master)](https://travis-ci.com/0xpr03/ts3_query)

Very barebone lib to connect to ts3 query interfaces and issue commands.

For examples visit the docs.

A lot of command-functions are currently not implemented, feel free to open issues or PRs adding them. (You can use the raw_command method along with its helpers to issue not implemented commands.) Any callback functionality for server events is also missing[1].



[1] Server events are very unreliable as teamspeak likes to disconnect without any notice and thus events require you to regularly check the connection health. But if you're already performing connections checks (by issuing commands) frequent enough to not miss out on changes on disconnect, you might as well switch to a polling approach and instead use the connection-check and retrieve the relevant data you need to handle "events". Furthermore server events require you to run a background task which reads incoming data and differentiate between events and issued commands while holding one reader, which is not a barebone library.
