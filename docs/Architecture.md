# Architecture

This documents intents to give a high level overview of how this project works, it should provide
a good starting point for anybody wanting to contribute. More details are also available as doc
comments at the top of most files.

## Overview
Inspect-embassy consists of a library called `inspect_embassy` implementing the parsing of the debug
data, parsing of the raw bytes from the target and displaying all this information in a TUI. And of
multiple backend implementations implementing the interaction with the user, raw terminal and the
target.

`inspect_embassy`'s entry point is the `EmbassyInspector` struct. Backend's create an instance of
this struct giving it a
[`ratatui::backend::Backend`](https://docs.rs/ratatui/latest/ratatui/backend/trait.Backend.html) 
and start the event loop. Events are parsed by the backend and given to the `EmbassyInspector` via
the `handle_event` method. `EmbassyInspector` uses the `Callback` trait to request information from
target via the backend.

### Operation
At startup the debug data gets parsed into a model containing the memory layout of all async fn,
join and select futures. The memory location of all task pools is also parsed from the debug data.

A breakpoint then gets set at the end of the poll function. Every time it of some other breakpoint
get hit the memory of every task pool get read. These bytes are then parsed based on the layout
model gotten above. All of this is then displayed in the TUI.

## `inspect_embassy` overview
### `model`
All code for parsing the debug data and then using that to parse the raw bytes from the target lives
here. Types ending in `Type` contain the layout of types in the target parsed from debug data. Types
ending in `Value` are obtained from reading bytes from the target and parsing them using the `*Type`
types.

### `ui`
Contains all the code for the ratatui TUI, `UIState` is the entry point. See its documentation for
more information.

This module (ab)uses `Result` and the `?` operator as `ratatui` has no native support for working
with mouse click's. If during drawing it is discovered the user has clicked on some UI part a
`Err(UIEvent)` is returned, allowing other parts of the UI to just use `?` to propagate it.
`EmbassyInspector` will capture this "error", call `apply_event` with the event and then redraw the
UI from scratch.
