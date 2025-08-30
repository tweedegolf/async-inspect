Embassy inspect consists of a library called `embassy_inspect` implementing the parsing of the debug
data, parsing of the raw bytes from the target and displaying all this information in a TUI.
The library gets started by a backend and uses the `Callback` trait to call back to it.

These backends also need to provide a
[`ratatui::backend::Backend`](https://docs.rs/ratatui/latest/ratatui/backend/trait.Backend.html) to
`embassy_inspect` which will then be drawn to.

