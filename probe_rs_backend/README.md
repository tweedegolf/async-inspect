## Build dependencies
- Rust [(install here)](https://www.rust-lang.org/tools/install)

## Building
1. Clone this directory: 
   ```
   git clone https://github.com/tweedegolf/async-inspect.git
   ```
2. Enter the `probe_rs_backend` directory
   ```
   cd async-inspect/probe_rs_backend
   ```
3. Build project and install it into a new virtual enviroment
   ```
   cargo build -r
   ```

## Running 
2. Run with the same arguments you would use for probe-rs and the path to the elf file on the chip
   to debug
   ```
   cargo run -r -- --chip nRF52840_xxAA /path/to/elf-file
   ```
