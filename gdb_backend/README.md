## Build dependencies
- Rust [(install here)](https://www.rust-lang.org/tools/install)
- uv [(install here)](https://github.com/astral-sh/uv)

## Building
1. Clone this directory: 
   ```
   git clone https://github.com/tweedegolf/async-inspect.git
   ```
2. Enter the `gdb_backend` directory
   ```
   cd async-inspect/gdb_backend
   ```
3. Build project and install it into a new virtual enviroment
   ```
   uv run maturin develop -r
   ```

## Running 
- The first time you will have to add the following to your `~/.config/gdb/gdbinit` to make gdb work
  with python virtual environments
  ```
  python
  # Update GDB's Python paths with the `sys.path` values of the local
  #  Python installation, whether that is brew'ed Python, a virtualenv,
  #  or another system python.
  
  # Convert GDB to interpret in Python
  import os,subprocess,sys
  # Execute a Python using the user's shell and pull out the sys.path (for site-packages)
  paths = subprocess.check_output('python -c "import os,sys;print(os.linesep.join(sys.path).strip())"',shell=True).decode("utf-8").split()
  # Extend GDB's Python's search path
  sys.path.extend(paths)
  end
  ```
1. Enter the virtual environment
   ```
   source ./.venv/bin/activate
   ```
2. Start GDB and connect to the target, for example:
   ```
   rust-gdb /path/to/binary-file
   (gdb) target remote :1337
   ```
3. Run the following GDB commands to start the TUI
   ```
   (gdb) source embassy_inspect.py
   (gdb) tui layout embassy_inspect
   (gdb) continue
   ```

> [!TIP]
> Use `(gdb) focus cmd` to be able to use the arrow keys for history in gdb again.

4. The embassy inspect TUI should now open at the top of the GDB window, you will have to use it via
   a mouse as GDB does not pass key presses along.

> [!NOTE]
> On my laptop only an external mouse can be used to click inside GDB, the trackpad does not work.

> [!TIP]
> After hitting the embassy inspect set breakpoint once it will be possible to run commands again,
> but the gdb display will not show the `(gdb)`. Just start typing and it should appear.
> 
> TODO: improve this situation
